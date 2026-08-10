//! Twitch-luik — channel-points-redeem → Hytale-whitelist (Rust-port van het
//! vroegere `tale/bot/twitch_bridge.py`).
//!
//! Een kijker doet een channel-points-redeem en typt (de 1e keer) zijn exacte
//! Hytale-naam. We ankeren op de ONVERANDERLIJKE Twitch-user-id en bewaren de grant in
//! market's bestaande `hytale_whitelist`-tabel onder de pseudo-id `twitch:<user_id>`.
//! De tale-bot leest die tabel read-only en whitelistet de naam op de Hytale-server
//! (`reconcile_market`/`enforce_whitelist`) — dit luik raakt de Hytale-FIFO dus NIET aan.
//!
//!   * 1e keer      → naam op goed vertrouwen registreren en VASTZETTEN.
//!   * volgende keer → **dezelfde** naam (of niets ingevuld): tijd erbij, stapelt, reset niet.
//!   * volgende keer met een **andere** naam → geen tijd, wél een whisper + een
//!     `twitch/name_mismatch`-regel; Faybelle betaalt de punten manueel terug.
//!   * lege/ongeldige naam bij de 1e keer → geen grant, wél een `twitch/rejected`-regel.
//!
//! ## De streamer bezit de reward, wij niet (omslag 2026-08-03)
//! Vroeger maakte deze app de reward zelf aan via Helix. Nu maakt de **streamer** ze aan
//! in haar eigen dashboard (met "kijker moet tekst invullen" aan) en herkent market ze aan
//! de **id** uit `settings` (Manage → ⚙ Settings). Gevolgen, bewust aanvaard:
//!
//!   * We kunnen redemptions **niet** meer fulfillen of annuleren — Helix laat dat enkel toe
//!     aan de app die de reward maakte (anders 403). **Terugbetalen gebeurt dus manueel** in
//!     de Twitch-wachtrij; het logboek zegt wanneer dat nodig is.
//!   * We abonneren **breed** (alle redemptions van het kanaal) en filteren zelf op id.
//!     Zo werkt een pas aangemaakte reward meteen na een keuze in Settings, zonder herstart.
//!
//! ## Waarom id en niet titel (2026-08-10)
//! Tot 2026-08-04 herkenden we de reward aan haar **titel**. Die dag zette Faybelle er een
//! emoji voor ('Meadowland Pass' → '🎫Meadowland Pass') en daarmee viel elke pas-redeem stil
//! in de "niet van ons"-tak: vier redeems (3× easycomes55, 1× heijicat), geen pas, geen
//! whisper, punten weg. Een reward-id verandert nooit — ook niet bij hernoemen — dus daar
//! matchen we sindsdien op. De titel dient enkel nog om de keuzelijst leesbaar te maken.
//! Zo'n id staat nergens in het Twitch-dashboard, vandaar de keuzelijst i.p.v. een tekstveld.
//!
//! Bevestiging gaat als **whisper** (Twitch-DM) naar de kijker — daar past ook het
//! serveradres in, want zonder adres geraakt hij er niet op. De tekst zelf staat in de
//! settings: speler-zichtbare tekst levert de user.

use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

use crate::config::Config;
use crate::db::{self, DbPool};
use crate::settings;

/// Basis van de Helix-API. Let op: dat is `api.twitch.tv`, niet `helix.twitch.tv` —
/// die laatste hostnaam bestaat niet en geeft een DNS-fout. Het pad `/helix/...`
/// komt er per call achter.
const HELIX: &str = "https://api.twitch.tv";
const TOKEN_URL: &str = "https://id.twitch.tv/oauth2/token";
const EVENTSUB_WS: &str = "wss://eventsub.wss.twitch.tv/ws";
const TOKENS_FILE: &str = "twitch_tokens.json";
/// Proactief het user-token verversen (Twitch-tokens leven ~4u).
const REFRESH_EVERY: Duration = Duration::from_secs(3 * 3600);

fn now_epoch() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

// --- Naam-hygiëne -----------------------------------------------------------
/// Maak een geldige Hytale-naam of None. Gebruikt dezelfde regel als de rest van
/// market (`^[A-Za-z0-9_]{1,32}$`), zodat een via Twitch vastgezette naam ook echt
/// door de tale-bot-reconcile gewhitelist wordt (spaties/control-tekens → geweigerd →
/// refund, i.p.v. een grant die stil nooit whitelistet).
fn clean_name(raw: &str) -> Option<String> {
    let name = raw.trim();
    if crate::web::valid_hytale_name(name) {
        Some(name.to_string())
    } else {
        None
    }
}

/// Typte de kijker bij een volgende redeem een **andere** Hytale-naam dan de naam die al aan
/// zijn Twitch-account vastzit?
///
/// Hoofdletters en spaties eromheen tellen niet mee — dat is dezelfde speler die z'n eigen naam
/// net anders intikt, en die mag zijn tijd gewoon krijgen. Een **leeg** invoerveld is evenmin
/// een conflict: dan heeft hij niets nieuws beweerd en blijft de vastgezette naam gelden.
fn name_conflicts(registered: &str, typed: &str) -> bool {
    let typed = typed.trim();
    !typed.is_empty() && !typed.eq_ignore_ascii_case(registered.trim())
}

/// Wijst een EventSub-URL naar een loopback-adres? (→ de lokale Twitch CLI-mock.)
fn is_loopback_ws(url: &str) -> bool {
    url.contains("127.0.0.1") || url.contains("localhost") || url.contains("[::1]")
}

// --- De rewards van het kanaal ----------------------------------------------
/// Waar de laatst opgehaalde reward-lijst ligt (`kv`). De Settings-pagina tekent haar
/// keuzelijst hieruit: het web-luik heeft geen Twitch-token, en een Helix-call bij elk
/// paginabezoek zou de streamer op Twitch laten wachten. Faalt het ophalen, dan blijft
/// de vorige lijst staan — beter een dag oude lijst dan een leeg keuzemenu.
pub const REWARDS_CACHE_KEY: &str = "twitch_rewards";
/// Marker voor de eenmalige overgang titel → id. Eenmalig omdat "niets gekozen" een
/// geldige keuze is: zonder deze marker zou een leeggemaakte keuzelijst bij de
/// volgende herstart weer gevuld worden vanuit de oude titel-instelling.
const MIGRATED_KEY: &str = "twitch_reward_id_migrated";
/// De lijst blijft vers zonder herstart: maakt de streamer een nieuwe reward aan, dan
/// staat ze binnen dit interval in de keuzelijst. Eén Helix-call, dus goedkoop.
const REWARDS_EVERY: Duration = Duration::from_secs(300);

/// Eén channel-points-reward van het kanaal: de id waar we op matchen, en de titel
/// waaraan een mens ze herkent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reward {
    pub id: String,
    pub title: String,
}

fn parse_rewards(v: &Value) -> Vec<Reward> {
    v.as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|r| {
                    let id = r["id"].as_str()?.trim();
                    if id.is_empty() {
                        return None;
                    }
                    Some(Reward {
                        id: id.to_string(),
                        title: r["title"].as_str().unwrap_or("").trim().to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// De rewards zoals ze bij de laatste geslaagde ophaling waren. Voor de Settings-pagina.
pub fn cached_rewards(pool: &DbPool) -> Vec<Reward> {
    db::kv_get(pool, REWARDS_CACHE_KEY)
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .map(|v| parse_rewards(&v))
        .unwrap_or_default()
}

fn store_rewards(pool: &DbPool, rewards: &[Reward]) {
    let v: Vec<Value> = rewards
        .iter()
        .map(|r| json!({ "id": r.id, "title": r.title }))
        .collect();
    db::kv_set(pool, REWARDS_CACHE_KEY, &Value::Array(v).to_string());
}

/// Id's vergelijken. Een lege ingestelde id matcht nooit — dat is precies hoe je zo'n
/// redeem uitzet. Hoofdletter-ongevoelig, want zo'n id kan ook geplakt zijn.
fn id_matches(a: &str, b: &str) -> bool {
    !b.trim().is_empty() && a.trim().eq_ignore_ascii_case(b.trim())
}

/// Titel herleiden tot wat er echt staat: enkel letters en cijfers, kleingeschreven.
/// Zo valt '🎫Meadowland Pass' samen met 'Meadowland Pass' — precies het verschil dat
/// op 2026-08-04 vier redeems kostte. Enkel gebruikt om de oude titel-instelling
/// éénmalig aan een id te koppelen, nooit om een redeem te beoordelen.
fn norm_title(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

/// Welke reward bedoelde de oude titel-instelling? Eerst letterlijk (op hoofdletters
/// en spaties na), daarna op de herleide titel — maar dan enkel als **precies één**
/// reward past. Twee kandidaten betekent gokken, en een verkeerde koppeling deelt
/// stil passen uit op de verkeerde reward.
fn find_by_title<'a>(rewards: &'a [Reward], title: &str) -> Option<&'a Reward> {
    let want = title.trim();
    if want.is_empty() {
        return None;
    }
    if let Some(r) = rewards.iter().find(|r| r.title.eq_ignore_ascii_case(want)) {
        return Some(r);
    }
    let want = norm_title(want);
    if want.is_empty() {
        return None;
    }
    let mut hits = rewards.iter().filter(|r| norm_title(&r.title) == want);
    match (hits.next(), hits.next()) {
        (Some(r), None) => Some(r),
        _ => None,
    }
}

/// Vul de door de user geschreven whisper-tekst in. Onbekende accolades blijven
/// gewoon staan — het is háár tekst, wij knippen er niets uit.
fn fill_template(tpl: &str, name: &str, hours: Option<u32>) -> String {
    let mut out = tpl.replace("{naam}", name);
    if let Some(h) = hours {
        out = out.replace("{uren}", &h.to_string());
    }
    out
}

/// Twitch weigert een whisper langer dan 500 tekens aan iemand die jou nog nooit
/// geschreven heeft. Kap op een tekengrens (niet midden in een UTF-8-teken).
fn cap_whisper(msg: &str) -> &str {
    const MAX: usize = 500;
    match msg.char_indices().nth(MAX) {
        Some((idx, _)) => &msg[..idx],
        None => msg,
    }
}

// --- Tokenopslag ------------------------------------------------------------
#[derive(Clone)]
struct Tokens {
    access: String,
    refresh: String,
}

fn load_tokens(path: &PathBuf) -> Result<Tokens, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("tokens-bestand {} ontbreekt of onleesbaar: {e}", path.display()))?;
    let v: Value = serde_json::from_str(&raw).map_err(|e| format!("tokens-json stuk: {e}"))?;
    let refresh = v
        .get("refresh_token")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    if refresh.is_empty() {
        return Err(format!(
            "{} bevat geen refresh_token — maak het eenmalig aan met de handmatige OAuth-stappen.",
            path.display()
        ));
    }
    let access = v
        .get("access_token")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    Ok(Tokens { access, refresh })
}

/// Atomisch wegschrijven (.tmp + rename), zodat een crash het bestand niet corrumpeert.
fn save_tokens(path: &PathBuf, t: &Tokens) {
    let body = json!({ "access_token": t.access, "refresh_token": t.refresh }).to_string();
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, body).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

/// Begrensde set van reeds verwerkte EventSub-`message_id`'s, voor de-duplicatie. Twitch levert
/// **at-least-once**: dezelfde notificatie kan meer dan eens binnenkomen (met hetzelfde
/// `message_id`). Zonder dedup zou een redelivery `grant_day_whitelist` een tweede keer stapelen
/// → dubbele pasduur. Houdt de laatste `cap` id's bij (FIFO-eviction).
#[derive(Default)]
struct Seen {
    set: HashSet<String>,
    order: VecDeque<String>,
}

impl Seen {
    /// `true` als `id` nieuw was (en nu onthouden); `false` als het al gezien was (→ dubbel).
    fn insert_new(&mut self, id: &str, cap: usize) -> bool {
        if id.is_empty() || self.set.contains(id) {
            return id.is_empty(); // lege id: niet dedupen (behandel als "nieuw"), maar niet bewaren
        }
        self.set.insert(id.to_string());
        self.order.push_back(id.to_string());
        if self.order.len() > cap {
            if let Some(old) = self.order.pop_front() {
                self.set.remove(&old);
            }
        }
        true
    }
}

const SEEN_CAP: usize = 512;

// --- Gedeelde runtime-context ----------------------------------------------
struct Ctx {
    http: reqwest::Client,
    app_id: String,
    app_secret: String,
    tokens_file: PathBuf,
    tok: Mutex<Tokens>,
    broadcaster_id: String,
    /// De gekozen rewards, de pasduur en de whisper-tekst staan bewust NIET hier: die
    /// worden per redeem vers uit `settings` gelezen, zodat een wijziging op de
    /// Settings-pagina meteen geldt (geen herstart).
    pool: DbPool,
    /// Mock-modus (Twitch CLI EventSub-mock): sla alle echte Helix/token-calls over.
    mock: bool,
    /// Reeds verwerkte EventSub-message-id's (dedup tegen at-least-once redelivery).
    seen: Mutex<Seen>,
}

impl Ctx {
    fn access(&self) -> String {
        self.tok.lock().unwrap().access.clone()
    }

    /// Vernieuw het user-token via de refresh_token en persisteer.
    async fn refresh(&self) -> Result<(), String> {
        let refresh = self.tok.lock().unwrap().refresh.clone();
        let resp = self
            .http
            .post(TOKEN_URL)
            .form(&[
                ("client_id", self.app_id.as_str()),
                ("client_secret", self.app_secret.as_str()),
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh.as_str()),
            ])
            .send()
            .await
            .map_err(|e| format!("token-refresh netwerkfout: {e}"))?;
        if !resp.status().is_success() {
            let s = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("token-refresh faalde ({s}): {body}"));
        }
        let v: Value = resp
            .json()
            .await
            .map_err(|e| format!("token-refresh json: {e}"))?;
        let access = v
            .get("access_token")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let new_refresh = v
            .get("refresh_token")
            .and_then(|x| x.as_str())
            .unwrap_or(&refresh)
            .to_string();
        if access.is_empty() {
            return Err("token-refresh gaf geen access_token".into());
        }
        let t = Tokens { access, refresh: new_refresh };
        save_tokens(&self.tokens_file, &t);
        *self.tok.lock().unwrap() = t;
        Ok(())
    }

    /// GET/POST/PATCH-helper met Client-Id + Bearer. Bij 401 één keer verversen en
    /// opnieuw proberen.
    async fn helix(
        &self,
        method: reqwest::Method,
        url: &str,
        body: Option<&Value>,
    ) -> Result<Value, String> {
        for attempt in 0..2 {
            let mut req = self
                .http
                .request(method.clone(), url)
                .header("Client-Id", &self.app_id)
                .bearer_auth(self.access());
            if let Some(b) = body {
                req = req.json(b);
            }
            let resp = req.send().await.map_err(|e| format!("helix netwerkfout: {e}"))?;
            let status = resp.status();
            if status == reqwest::StatusCode::UNAUTHORIZED && attempt == 0 {
                self.refresh().await?;
                continue;
            }
            let text = resp.text().await.unwrap_or_default();
            if !status.is_success() {
                return Err(format!("helix {url} → {status}: {text}"));
            }
            if text.is_empty() {
                return Ok(Value::Null);
            }
            return serde_json::from_str(&text).map_err(|e| format!("helix json: {e}"));
        }
        Err("helix: onbereikbaar na refresh".into())
    }

    /// Privébericht (whisper) naar de kijker — dát is waar het serveradres in staat.
    /// Faalt dit, dan is de pas al toegekend; we loggen enkel, want de toegang zelf
    /// mag niet afhangen van of Twitch het bericht doorlaat.
    ///
    /// Twitch-eisen (docs): scope `user:manage:whispers`, en het **zendende account
    /// moet een geverifieerd telefoonnummer** hebben (anders 401). Een 403 betekent
    /// dat de kijker whispers van vreemden blokkeert — daar kunnen wij niets aan doen.
    async fn whisper(&self, to_user_id: &str, msg: &str) {
        if msg.trim().is_empty() || to_user_id.is_empty() {
            return; // lege tekst = de user heeft het veld (nog) niet ingevuld ⇒ geen bericht
        }
        if self.mock {
            tracing::info!("[mock] whisper → {to_user_id}: {msg}");
            return;
        }
        let url = format!(
            "{HELIX}/helix/whispers?from_user_id={}&to_user_id={to_user_id}",
            self.broadcaster_id
        );
        let body = json!({ "message": cap_whisper(msg) });
        match self.helix(reqwest::Method::POST, &url, Some(&body)).await {
            Ok(_) => tracing::info!("Twitch-whisper verstuurd naar {to_user_id}"),
            Err(e) => tracing::warn!(
                "kon Twitch-whisper niet sturen naar {to_user_id}: {e} \
                 (401 = zendend account zonder geverifieerd telefoonnummer of scope \
                 'user:manage:whispers' ontbreekt; 403 = de kijker laat geen whispers toe)"
            ),
        }
    }
}

// --- De kern: reageren op een redeem ---------------------------------------
/// Welke pas hoort bij deze redemption? We krijgen álle redemptions van het kanaal
/// binnen, dus `None` (= "niet van ons") is de normale uitkomst voor elke andere
/// beloning die de streamer aanbiedt. Pure functie zodat de keuze los te testen is.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum PassKind {
    Day,
    Perma,
}

fn pass_kind_for(reward_id: &str, day_id: &str, perma_id: &str) -> Option<PassKind> {
    // Perma eerst: staan beide instellingen per ongeluk op dezelfde reward, dan is
    // permanent geven erger dan een paar uur geven — maar de keuze moet vastliggen,
    // niet toevallig zijn.
    if id_matches(reward_id, perma_id) {
        Some(PassKind::Perma)
    } else if id_matches(reward_id, day_id) {
        Some(PassKind::Day)
    } else {
        None
    }
}

async fn on_redeem(ctx: &Ctx, event: &Value) {
    let tid = event.get("user_id").and_then(|x| x.as_str()).unwrap_or("");
    let login = event.get("user_login").and_then(|x| x.as_str()).unwrap_or("");
    let reward = event.get("reward");
    let reward_id = reward
        .and_then(|r| r.get("id"))
        .and_then(|x| x.as_str())
        .unwrap_or("");
    // De titel doet niet meer mee aan de beslissing; ze staat er enkel voor de log,
    // zodat een mens ziet wélke reward er langskwam.
    let title = reward
        .and_then(|r| r.get("title"))
        .and_then(|x| x.as_str())
        .unwrap_or("");
    let user_input = event
        .get("user_input")
        .and_then(|x| x.as_str())
        .unwrap_or("");
    if tid.is_empty() {
        tracing::warn!("Twitch-redeem zonder user_id — genegeerd");
        return;
    }

    // Vers uit de settings: kiest de streamer een andere reward, dan geldt dat meteen.
    let day_id = settings::str_of(&ctx.pool, "twitch_reward_id");
    let perma_id = settings::str_of(&ctx.pool, "twitch_perma_reward_id");
    let Some(kind) = pass_kind_for(reward_id, &day_id, &perma_id) else {
        // Elke andere beloning van het kanaal komt hier ook binnen; dat is geen fout.
        // Wel loggen wát er langskwam, mét id: is de gekozen reward per ongeluk
        // gewist en opnieuw aangemaakt, dan is dit de enige aanwijzing.
        tracing::info!(
            "Twitch-redeem '{title}' ({reward_id}) van {login} genegeerd — niet de \
             ingestelde reward"
        );
        return;
    };
    let uid = format!("twitch:{tid}");

    // Naam vastzetten: bestaat er al een naam voor deze Twitch-id, dan blijft die gelden.
    // Anders de getypte naam opschonen en vastzetten.
    let (name, first_time) = match db::get_whitelist_name(&ctx.pool, &uid) {
        Some(n) => {
            // Tweede redeem met een ANDERE naam: niets toekennen. De naam ligt na de eerste
            // keer vast (tegen doorgeven aan derden), dus stilzwijgend de tijd op de oude
            // naam zetten zou de kijker laten betalen voor iets wat hij niet vroeg. Hij
            // krijgt een bericht en Faybelle betaalt de punten manueel terug.
            if name_conflicts(&n, user_input) {
                tracing::info!(
                    "Twitch-redeem geweigerd (andere naam): {login} staat geregistreerd als \
                     '{n}' maar typte '{user_input}' — manuele terugbetaling nodig"
                );
                db::log_event(
                    &ctx.pool,
                    now_epoch(),
                    &db::LogEntry::new("twitch", "name_mismatch").actor(&uid, &n).detail(format!(
                        "typed '{user_input}' but is registered as '{n}' — refund manually in Twitch"
                    )),
                );
                let tpl = settings::str_of(&ctx.pool, "twitch_mismatch_whisper_text");
                ctx.whisper(tid, &fill_template(&tpl, &n, None)).await;
                return;
            }
            (n, false)
        }
        None => match clean_name(user_input) {
            Some(n) => (n, true),
            None => {
                // GEEN automatische refund meer: de reward is van de streamer, dus Helix
                // laat ons de redemption niet annuleren. Deze logregel is het signaal om
                // in de Twitch-wachtrij manueel terug te betalen.
                tracing::info!(
                    "Twitch-redeem geweigerd (lege/ongeldige naam): {login} typte '{user_input}' \
                     — manuele terugbetaling nodig"
                );
                db::log_event(
                    &ctx.pool,
                    now_epoch(),
                    &db::LogEntry::new("twitch", "rejected")
                        .actor(&uid, login)
                        .detail(format!(
                            "invalid Hytale name: '{user_input}' — refund manually in Twitch"
                        )),
                );
                return;
            }
        },
    };

    let now = now_epoch();
    let reg = if first_time { " (naam nu vastgezet)" } else { "" };

    match kind {
        PassKind::Perma => {
            db::grant_perma_whitelist(&ctx.pool, &uid, &name);

            // Logboek: permanente Twitch-pas toegekend — bindt kijker aan Hytale-naam.
            db::log_event(
                &ctx.pool,
                now,
                &db::LogEntry::new("twitch", "whitelist-perma")
                    .actor(&uid, &name)
                    .detail(format!("{login} → {name} · permanent")),
            );

            let tpl = settings::str_of(&ctx.pool, "twitch_perma_whisper_text");
            ctx.whisper(tid, &fill_template(&tpl, &name, None)).await;
            tracing::info!(
                "Twitch-pas (PERMANENT): {name} ({login}){reg} — grant in hytale_whitelist als {uid}"
            );
        }
        PassKind::Day => {
            let hours = settings::i64_of(&ctx.pool, "twitch_pass_hours").max(1) as u32;
            let add_secs = hours as f64 * 3600.0;
            db::grant_day_whitelist(&ctx.pool, &uid, &name, add_secs, now);

            // Logboek: Twitch-pas toegekend (whitelist-grant) — bindt kijker aan Hytale-naam.
            db::log_event(
                &ctx.pool,
                now,
                &db::LogEntry::new("twitch", "whitelist")
                    .actor(&uid, &name)
                    .detail(format!("{login} → {name} · {hours}h")),
            );

            let tpl = settings::str_of(&ctx.pool, "twitch_whisper_text");
            ctx.whisper(tid, &fill_template(&tpl, &name, Some(hours))).await;
            tracing::info!(
                "Twitch-pas: {name} ({login}){reg} — {hours}u, grant in hytale_whitelist als {uid}"
            );
        }
    }
}

// --- Opstarten: token, broadcaster, reward ----------------------------------
async fn bootstrap(cfg: &Config, pool: DbPool, mock: bool) -> Result<Ctx, String> {
    let tokens_file = PathBuf::from(TOKENS_FILE);
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| format!("http-client: {e}"))?;

    // Mock-modus (Twitch CLI EventSub-mock): geen token, geen Helix — de EventSub-kern
    // (parsing, on_redeem, grant) draait wél echt tegen de lokale coins.db.
    if mock {
        tracing::warn!("Twitch MOCK-modus actief — bootstrap-Helix + token overgeslagen.");
        return Ok(Ctx {
            http,
            app_id: cfg.twitch_app_id.clone(),
            app_secret: cfg.twitch_app_secret.clone(),
            tokens_file,
            tok: Mutex::new(Tokens { access: String::new(), refresh: String::new() }),
            broadcaster_id: "mock_broadcaster".into(),
            pool,
            mock: true,
            seen: Mutex::new(Seen::default()),
        });
    }

    let tokens = load_tokens(&tokens_file)?;
    let ctx = Ctx {
        http,
        app_id: cfg.twitch_app_id.clone(),
        app_secret: cfg.twitch_app_secret.clone(),
        tokens_file,
        tok: Mutex::new(tokens),
        broadcaster_id: String::new(),
        pool,
        mock: false,
        seen: Mutex::new(Seen::default()),
    };
    // Meteen verversen zodat we met een gegarandeerd geldig token starten.
    ctx.refresh().await?;

    // Wie zijn we? (het token is van de broadcaster)
    let me = ctx
        .helix(reqwest::Method::GET, &format!("{HELIX}/helix/users"), None)
        .await?;
    let broadcaster_id = me["data"][0]["id"].as_str().unwrap_or("").to_string();
    let broadcaster_login = me["data"][0]["login"].as_str().unwrap_or("").to_string();
    if broadcaster_id.is_empty() {
        return Err("kon broadcaster-id niet ophalen (get_users leeg)".into());
    }

    let ctx = Ctx { broadcaster_id, ..ctx };
    // Eerst de lijst ophalen: de eenmalige titel→id-overgang hieronder heeft ze nodig,
    // en de startregel moet de titel van de gekozen reward kunnen tonen.
    refresh_rewards(&ctx, true).await;
    let rewards = cached_rewards(&ctx.pool);
    adopt_reward_ids(&ctx.pool, &rewards);
    tracing::info!(
        "Twitch-luik actief — kanaal={broadcaster_login}, reward={}, perma-reward={}, pas={}u",
        describe_choice(&ctx.pool, "twitch_reward_id", &rewards, "(niets gekozen — redeems worden genegeerd)"),
        describe_choice(&ctx.pool, "twitch_perma_reward_id", &rewards, "(uit)"),
        settings::i64_of(&ctx.pool, "twitch_pass_hours"),
    );

    Ok(ctx)
}

/// Hoe de gekozen reward in de log komt: met haar huidige titel, zodat een mens kan
/// nakijken of dat de bedoelde is. Staat de ingestelde id niet meer tussen de rewards
/// van het kanaal, dan zeggen we dat expliciet — dat is de enige stille faalmodus die
/// overblijft nu we op id matchen (reward gewist en opnieuw aangemaakt).
fn describe_choice(pool: &DbPool, key: &str, rewards: &[Reward], empty: &str) -> String {
    let id = settings::str_of(pool, key);
    if id.is_empty() {
        return empty.to_string();
    }
    match rewards.iter().find(|r| r.id.eq_ignore_ascii_case(&id)) {
        Some(r) => format!("'{}' ({id})", r.title),
        None if rewards.is_empty() => format!("{id} (reward-lijst niet beschikbaar)"),
        None => format!("{id} ⚠️ STAAT NIET MEER TUSSEN DE REWARDS VAN HET KANAAL"),
    }
}

/// Haal de channel-points-rewards op en leg ze in de cache voor de Settings-pagina.
/// Mislukt de call (scope/rechten/netwerk), dan blijft de vorige lijst staan en is dat
/// geen reden om het luik niet te starten — de matching hangt aan de opgeslagen id, niet
/// aan deze lijst. Loggen doen we enkel bij de start en bij een wijziging: dit draait om
/// de vijf minuten en een onveranderde lijst is geen nieuws.
async fn refresh_rewards(ctx: &Ctx, first: bool) {
    let url = format!(
        "{HELIX}/helix/channel_points/custom_rewards?broadcaster_id={}",
        ctx.broadcaster_id
    );
    let v = match ctx.helix(reqwest::Method::GET, &url, None).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("kon de reward-lijst niet ophalen: {e}");
            return;
        }
    };
    let rewards = parse_rewards(&v["data"]);
    if rewards.is_empty() {
        // Niet de cache leegmaken: een leeg antwoord is even vaak "geen rechten" als
        // "geen rewards", en een leeggemaakte cache betekent een leeg keuzemenu.
        tracing::warn!(
            "Twitch: geen channel-points-rewards ontvangen — de streamer moet ze zelf \
             aanmaken, met 'kijker moet tekst invullen' aan."
        );
        return;
    }
    let changed = cached_rewards(&ctx.pool) != rewards;
    store_rewards(&ctx.pool, &rewards);
    if first || changed {
        let titles: Vec<String> = rewards.iter().map(|r| format!("'{}'", r.title)).collect();
        tracing::info!("Twitch-rewards op het kanaal: {}", titles.join(", "));
        // Enkel bij een wijziging opnieuw waarschuwen: hernoemen is ongevaarlijk
        // geworden, maar wissen-en-heraanmaken geeft een nieuwe id.
        for key in ["twitch_reward_id", "twitch_perma_reward_id"] {
            let id = settings::str_of(&ctx.pool, key);
            if !id.is_empty() && !rewards.iter().any(|r| r.id.eq_ignore_ascii_case(&id)) {
                tracing::warn!(
                    "Twitch: de ingestelde reward ({key} = {id}) staat niet tussen de rewards \
                     van het kanaal — redeems ervan worden genegeerd. Kies ze opnieuw in \
                     Manage → ⚙ Settings."
                );
            }
        }
    }
}

/// Eenmalige overgang van de oude titel-instelling naar een id (2026-08-10). Draait
/// enkel zolang de marker niet staat, want "niets gekozen" is een geldige keuze: zonder
/// die marker zou een bewust leeggemaakte keuzelijst bij elke herstart terugspringen.
/// Zonder reward-lijst gebeurt er niets — dan is de overgang gewoon nog niet aan de beurt.
fn adopt_reward_ids(pool: &DbPool, rewards: &[Reward]) {
    if rewards.is_empty() || db::kv_get(pool, MIGRATED_KEY).is_some() {
        return;
    }
    for (id_key, title_key) in [
        ("twitch_reward_id", "twitch_reward_title"),
        ("twitch_perma_reward_id", "twitch_perma_reward_title"),
    ] {
        if !settings::str_of(pool, id_key).is_empty() {
            continue; // al gekozen — de keuze van de streamer wint
        }
        // De oude sleutel heeft geen Spec meer, dus rechtstreeks uit de tabel.
        let old = db::setting_get(pool, title_key).unwrap_or_default();
        if old.trim().is_empty() {
            continue;
        }
        match find_by_title(rewards, &old) {
            Some(r) => {
                settings::set(pool, id_key, &r.id);
                tracing::info!(
                    "Twitch: {id_key} overgenomen uit de oude titel-instelling — '{old}' is nu \
                     '{}' ({})",
                    r.title,
                    r.id
                );
            }
            None => tracing::warn!(
                "Twitch: geen reward gevonden voor de oude titel '{old}' ({title_key}) — kies de \
                 juiste reward in Manage → ⚙ Settings."
            ),
        }
    }
    db::kv_set(pool, MIGRATED_KEY, "1");
}

// --- EventSub-WebSocket-loop ------------------------------------------------
/// Verbind, abonneer op de reward-redemption en verwerk notificaties tot de socket
/// sluit of een reconnect gevraagd wordt. Retourneert een optionele reconnect-URL.
async fn ws_session(ctx: &Ctx, url: &str) -> Result<Option<String>, String> {
    let (ws, _) = tokio_tungstenite::connect_async(url)
        .await
        .map_err(|e| format!("ws-connect faalde: {e}"))?;
    let (mut write, mut read) = ws.split();
    let mut subscribed = false;
    // Read-deadline: Twitch stuurt periodiek `session_keepalive`. Komt er binnen het keepalive-
    // venster (uit `session_welcome`, Twitch-default ~10s) niets binnen, dan is de verbinding dood
    // (bv. half-open TCP) → reconnecten i.p.v. eeuwig blijven hangen. +5s speling voor jitter.
    // Ruime startwaarde tot het welcome de echte timeout aanreikt.
    let mut keepalive = Duration::from_secs(30);

    loop {
        let msg = match tokio::time::timeout(keepalive + Duration::from_secs(5), read.next()).await {
            Ok(Some(m)) => m.map_err(|e| format!("ws-lees fout: {e}"))?,
            Ok(None) => return Ok(None), // stream netjes gesloten
            Err(_) => {
                tracing::warn!(
                    "Twitch EventSub: geen keepalive binnen {}s — herverbinden",
                    keepalive.as_secs() + 5
                );
                return Ok(None);
            }
        };
        let text = match msg {
            Message::Text(t) => t.to_string(),
            Message::Ping(p) => {
                let _ = write.send(Message::Pong(p)).await;
                continue;
            }
            Message::Close(_) => return Ok(None),
            _ => continue,
        };
        let v: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let mtype = v["metadata"]["message_type"].as_str().unwrap_or("");
        match mtype {
            "session_welcome" => {
                let session_id = v["payload"]["session"]["id"].as_str().unwrap_or("");
                if session_id.is_empty() {
                    return Err("session_welcome zonder session-id".into());
                }
                // Echte keepalive-timeout overnemen zodat onze read-deadline klopt.
                if let Some(k) = v["payload"]["session"]["keepalive_timeout_seconds"].as_u64() {
                    keepalive = Duration::from_secs(k.max(1));
                }
                if ctx.mock {
                    tracing::info!("[mock] EventSub verbonden — session-id: {session_id}");
                } else if !subscribed {
                    subscribe_redemptions(ctx, session_id).await?;
                    subscribed = true;
                }
            }
            "session_reconnect" => {
                let new_url = v["payload"]["session"]["reconnect_url"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                tracing::info!("Twitch EventSub: reconnect gevraagd");
                return Ok(if new_url.is_empty() { None } else { Some(new_url) });
            }
            "notification" => {
                let sub_type = v["metadata"]["subscription_type"].as_str().unwrap_or("");
                if sub_type == "channel.channel_points_custom_reward_redemption.add" {
                    // Dedup op message_id: Twitch levert at-least-once, een redelivery mag geen
                    // tweede whitelist-grant (= dubbele pasduur) geven.
                    let mid = v["metadata"]["message_id"].as_str().unwrap_or("");
                    let fresh = ctx.seen.lock().unwrap().insert_new(mid, SEEN_CAP);
                    if fresh {
                        on_redeem(ctx, &v["payload"]["event"]).await;
                    } else {
                        tracing::info!("Twitch EventSub: dubbele notificatie genegeerd (message_id={mid})");
                    }
                }
            }
            "revocation" => {
                tracing::warn!("Twitch EventSub: abonnement ingetrokken: {}", text);
            }
            _ => {} // session_keepalive e.d. resetten enkel de read-deadline
        }
    }
}

async fn subscribe_redemptions(ctx: &Ctx, session_id: &str) -> Result<(), String> {
    let url = format!("{HELIX}/helix/eventsub/subscriptions");
    // Eén breed abonnement: `reward_id` in de condition is optioneel (Twitch-docs) en we
    // laten het bewust weg, ook al kennen we de id nu wél. Twee redenen: een andere keuze
    // in ⚙ Settings geldt dan meteen i.p.v. na een herstart, en we blijven élke redeem
    // zien — dus ook eentje die we negeren. Die logregel was op 2026-08-04 het enige
    // spoor van de kapotte koppeling; met een strak abonnement was er niets geweest.
    let body = json!({
        "type": "channel.channel_points_custom_reward_redemption.add",
        "version": "1",
        "condition": { "broadcaster_user_id": ctx.broadcaster_id },
        "transport": { "method": "websocket", "session_id": session_id },
    });
    ctx.helix(reqwest::Method::POST, &url, Some(&body)).await?;
    tracing::info!("Twitch EventSub: geabonneerd op alle reward-redemptions van het kanaal");
    Ok(())
}

/// Entrypoint: gestart vanuit main als `cfg.twitch_ready()`. Zelf-herstellend:
/// verbindt opnieuw met backoff en ververst het token periodiek.
pub async fn run(pool: DbPool, cfg: Config) {
    // Test-hook: wijs naar de Twitch CLI EventSub-mock (bv. ws://127.0.0.1:8080/ws).
    // Gezet ⇒ mock-modus (geen echte Helix/token nodig).
    let url_override = std::env::var("TWITCH_EVENTSUB_URL").ok();
    let start_url = url_override.clone().unwrap_or_else(|| EVENTSUB_WS.to_string());
    // Mock-modus enkel bij een EXPLICIETE vlag, of als de override een loopback-adres is (de
    // Twitch CLI EventSub-mock draait op ws://127.0.0.1). Zo forceert een override naar een écht/
    // alternatief endpoint géén mock meer — dat schreef anders wél echte grants naar coins.db maar
    // fulfillde de redemptions nooit (punten niet verrekend).
    let mock = std::env::var("MARKET_TWITCH_MOCK")
        .map(|v| v != "0" && !v.is_empty())
        .unwrap_or(false)
        || url_override.as_deref().map(is_loopback_ws).unwrap_or(false);

    let ctx = match bootstrap(&cfg, pool, mock).await {
        Ok(c) => std::sync::Arc::new(c),
        Err(e) => {
            tracing::error!("Twitch-luik start niet: {e}");
            return;
        }
    };

    // Achtergrond: token proactief verversen zodat lange sessies geldig blijven.
    // In mock-modus is er geen token → overslaan.
    if !ctx.mock {
        let ctx = ctx.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(REFRESH_EVERY).await;
                if let Err(e) = ctx.refresh().await {
                    tracing::warn!("periodieke Twitch-token-refresh faalde: {e}");
                }
            }
        });
    }

    // Achtergrond: de reward-lijst vers houden voor de keuzelijst in ⚙ Settings.
    // Zonder dit zou een reward die de streamer vandaag aanmaakt pas na een deploy
    // te kiezen zijn.
    if !ctx.mock {
        let ctx = ctx.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(REWARDS_EVERY).await;
                refresh_rewards(&ctx, false).await;
            }
        });
    }

    let mut url = start_url.clone();
    let mut backoff = 1u64;
    loop {
        match ws_session(&ctx, &url).await {
            Ok(Some(next)) => {
                url = next; // reconnect naar de aangereikte URL, geen backoff
                backoff = 1;
            }
            Ok(None) => {
                url = start_url.clone();
                tracing::warn!("Twitch EventSub-socket sloot — herverbinden over {backoff}s");
                tokio::time::sleep(Duration::from_secs(backoff)).await;
                backoff = (backoff * 2).min(60);
            }
            Err(e) => {
                url = start_url.clone();
                tracing::warn!("Twitch EventSub-fout: {e} — herverbinden over {backoff}s");
                tokio::time::sleep(Duration::from_secs(backoff)).await;
                backoff = (backoff * 2).min(60);
            }
        }
    }
}

#[cfg(test)]
mod dedup {
    use super::{
        cap_whisper, fill_template, find_by_title, is_loopback_ws, name_conflicts, parse_rewards,
        pass_kind_for, PassKind, Reward, Seen, EVENTSUB_WS, HELIX, SEEN_CAP, TOKEN_URL,
    };
    use serde_json::json;

    fn rewards() -> Vec<Reward> {
        vec![
            Reward { id: "aaa-1".into(), title: "🎫Meadowland Pass".into() },
            Reward { id: "bbb-2".into(), title: "🌼Grow a Flower".into() },
            Reward { id: "ccc-3".into(), title: "Meadowland Forever".into() },
        ]
    }

    /// De naam ligt na de eerste redeem vast. Typt de kijker later een àndere naam, dan mag er
    /// geen tijd toegekend worden — maar "anders getypt" is niet hetzelfde als "andere speler".
    #[test]
    fn afwijkende_naam_bij_een_volgende_redeem() {
        // Dezelfde naam, anders getikt: gewoon doorgaan.
        assert!(!name_conflicts("Waldstein", "Waldstein"));
        assert!(!name_conflicts("Waldstein", "  waldstein "));
        assert!(!name_conflicts("Waldstein", "WALDSTEIN"));
        // Niets ingevuld = niets beweerd; de vastgezette naam blijft gelden.
        assert!(!name_conflicts("Waldstein", ""));
        assert!(!name_conflicts("Waldstein", "   "));
        // Een écht andere naam: weigeren.
        assert!(name_conflicts("Waldstein", "Faybelle"));
        assert!(name_conflicts("Waldstein", "Waldstein2"));
        // Ook rommel is een afwijking — dan heeft hij iets anders bedoeld dan wat vastligt.
        assert!(name_conflicts("Waldstein", "geef mij tijd"));
    }

    /// Regressie 2026-08-04: de basis stond op `helix.twitch.tv` — een hostnaam die
    /// niet bestaat, dus élke Helix-call faalde met een DNS-fout. De mock-e2e zag dat
    /// nooit, want die wijst de basis naar loopback. Vandaar deze test op de echte
    /// constanten: de Helix-API woont op `api.twitch.tv`, met `/helix` in het pad.
    #[test]
    fn helix_base_is_the_real_api_host() {
        assert_eq!(HELIX, "https://api.twitch.tv");
        assert_eq!(format!("{HELIX}/helix/users"), "https://api.twitch.tv/helix/users");
        assert_eq!(TOKEN_URL, "https://id.twitch.tv/oauth2/token");
        assert_eq!(EVENTSUB_WS, "wss://eventsub.wss.twitch.tv/ws");
    }

    /// De reward-ID bepaalt dag vs. permanent vs. "niet van ons". Een lege
    /// ingestelde id matcht nooit — dat is precies hoe je zo'n redeem uitzet.
    #[test]
    fn pass_kind_routing() {
        let day = "aaa-1";
        let perma = "ccc-3";
        assert_eq!(pass_kind_for(day, day, perma), Some(PassKind::Day));
        assert_eq!(pass_kind_for(perma, day, perma), Some(PassKind::Perma));
        // Elke andere beloning van het kanaal → niets doen.
        assert_eq!(pass_kind_for("bbb-2", day, perma), None);
        // Spaties eromheen en kapitaal mogen niet uitmaken (geplakte id).
        assert_eq!(pass_kind_for("  AAA-1 ", day, perma), Some(PassKind::Day));
        // Perma-id leeg = die redeem bestaat niet; een lege reward-id matcht nooit.
        assert_eq!(pass_kind_for(perma, day, ""), None);
        assert_eq!(pass_kind_for("", day, perma), None);
        // Beide instellingen leeg ⇒ market doet niets, ook niet bij een lege id.
        assert_eq!(pass_kind_for("", "", ""), None);
        assert_eq!(pass_kind_for("whatever", "", ""), None);
    }

    /// Regressie 2026-08-04: de reward heette 'Meadowland Pass' en kreeg er een emoji
    /// voor. Op de titel matchen brak daarmee stil; op de id matchen niet. Dit is
    /// precies dat scenario.
    #[test]
    fn hernoemen_breekt_de_koppeling_niet() {
        let gekozen = "aaa-1"; // ooit gekozen toen ze nog 'Meadowland Pass' heette
        // De redeem komt binnen met de NIEUWE titel, maar dezelfde id.
        assert_eq!(pass_kind_for("aaa-1", gekozen, ""), Some(PassKind::Day));
        // En een andere reward van hetzelfde kanaal blijft buiten schot.
        assert_eq!(pass_kind_for("bbb-2", gekozen, ""), None);
    }

    /// De eenmalige overgang titel → id. Een emoji vooraan mag de koppeling niet
    /// tegenhouden, maar gokken bij twijfel evenmin.
    #[test]
    fn oude_titel_koppelen_aan_een_reward() {
        let rw = rewards();
        // Letterlijk (op kapitaal na).
        assert_eq!(find_by_title(&rw, "meadowland forever").map(|r| &*r.id), Some("ccc-3"));
        // Dit is de echte prod-situatie: ingesteld zonder emoji, reward mét.
        assert_eq!(find_by_title(&rw, "Meadowland Pass").map(|r| &*r.id), Some("aaa-1"));
        // Niets ingesteld, of een titel die nergens op slaat → geen koppeling.
        assert_eq!(find_by_title(&rw, "  "), None);
        assert_eq!(find_by_title(&rw, "Song request"), None);
        // Twee kandidaten na het strippen ⇒ liever niets dan de verkeerde reward.
        let dubbel = vec![
            Reward { id: "x".into(), title: "🎫Pass".into() },
            Reward { id: "y".into(), title: "Pass!".into() },
        ];
        assert_eq!(find_by_title(&dubbel, "pass"), None);
    }

    /// De échte lijst van faybelle___ (uit de prod-log van 2026-08-04) tegen de titel
    /// die er die dag in de settings stond. Dit is de situatie die de overgang moet
    /// oplossen, dus staat ze hier letterlijk.
    #[test]
    fn prod_lijst_koppelt_de_pas_reward_eenduidig() {
        let echt: Vec<Reward> = [
            "⬆️Stand up!",
            "Craft a Fay Orb",
            "👋New here!",
            "🍝Feed The Lady!",
            "💧Drink Dear!",
            "🎫Meadowland Pass",
            "Partyblower!!",
            "👓Wear glasses",
            "Fireworks!",
            "🥇First!",
            "👀BirthLang",
            "WIP (do not redeem) Buy a Drink",
            "🌼Grow a Flower",
        ]
        .iter()
        .enumerate()
        .map(|(i, t)| Reward { id: format!("id-{i}"), title: (*t).into() })
        .collect();

        assert_eq!(find_by_title(&echt, "Meadowland Pass").map(|r| &*r.id), Some("id-5"));
        // De perma-instelling stond leeg en moet leeg blijven.
        assert_eq!(find_by_title(&echt, ""), None);
    }

    /// De eenmalige overgang tegen een echte DB — dit is wat de prod-instelling van
    /// 2026-08-04 weer aan de praat krijgt, zonder dat iemand een id moet opzoeken.
    #[test]
    fn overgang_titel_naar_id_gebeurt_precies_een_keer() {
        use crate::db;
        let p = std::env::temp_dir().join(format!("market-tw-{}-adopt.db", std::process::id()));
        let _ = std::fs::remove_file(&p);
        let pool = db::init_pool(p.to_str().unwrap());

        // De prod-toestand: titel zonder emoji, reward mét.
        db::setting_set(&pool, "twitch_reward_title", "Meadowland Pass");
        super::adopt_reward_ids(&pool, &rewards());
        assert_eq!(crate::settings::str_of(&pool, "twitch_reward_id"), "aaa-1");
        // De perma-titel stond leeg → niets gekozen, en dat blijft zo.
        assert_eq!(crate::settings::str_of(&pool, "twitch_perma_reward_id"), "");

        // Zet Faybelle de keuze bewust terug op "niets", dan mag de oude titel ze niet
        // opnieuw invullen bij de volgende herstart.
        crate::settings::set(&pool, "twitch_reward_id", "");
        super::adopt_reward_ids(&pool, &rewards());
        assert_eq!(crate::settings::str_of(&pool, "twitch_reward_id"), "");

        let _ = std::fs::remove_file(&p);
    }

    /// Zonder reward-lijst (Helix onbereikbaar bij de start) is de overgang gewoon nog
    /// niet aan de beurt — ze mag haar enige kans niet opgebruiken.
    #[test]
    fn overgang_wacht_op_een_reward_lijst() {
        use crate::db;
        let p = std::env::temp_dir().join(format!("market-tw-{}-wait.db", std::process::id()));
        let _ = std::fs::remove_file(&p);
        let pool = db::init_pool(p.to_str().unwrap());

        db::setting_set(&pool, "twitch_reward_title", "Meadowland Pass");
        super::adopt_reward_ids(&pool, &[]);
        assert_eq!(crate::settings::str_of(&pool, "twitch_reward_id"), "");
        // Lijst binnen ⇒ alsnog.
        super::adopt_reward_ids(&pool, &rewards());
        assert_eq!(crate::settings::str_of(&pool, "twitch_reward_id"), "aaa-1");

        let _ = std::fs::remove_file(&p);
    }

    /// De reward-lijst uit Helix wordt gelezen zoals ze binnenkomt; een rij zonder
    /// bruikbare id is geen keuze en valt weg.
    #[test]
    fn reward_lijst_lezen() {
        let v = json!([
            { "id": "aaa-1", "title": "🎫Meadowland Pass" },
            { "id": "", "title": "kapot" },
            { "title": "geen id" },
            { "id": "bbb-2" }
        ]);
        let got = parse_rewards(&v);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0], Reward { id: "aaa-1".into(), title: "🎫Meadowland Pass".into() });
        // Een titelloze reward blijft kiesbaar — de id is wat telt.
        assert_eq!(got[1], Reward { id: "bbb-2".into(), title: String::new() });
        assert!(parse_rewards(&json!({})).is_empty());
    }

    /// De whisper-tekst is van de user; wij vullen enkel de plaatshouders in.
    #[test]
    fn whisper_template_fills_placeholders() {
        let tpl = "Hoi {naam}, je mag {uren} uur op 1.2.3.4:5520 — {uren} uur!";
        assert_eq!(
            fill_template(tpl, "Waldstein", Some(2)),
            "Hoi Waldstein, je mag 2 uur op 1.2.3.4:5520 — 2 uur!"
        );
        // Permanent: er is geen urental, dus {uren} blijft staan i.p.v. iets te verzinnen.
        assert_eq!(fill_template("{naam}: {uren}", "X", None), "X: {uren}");
        // Onbekende accolades blijven ongemoeid — het is de tekst van de user.
        assert_eq!(fill_template("{onbekend} {naam}", "X", None), "{onbekend} X");
    }

    /// Kappen op 500 tekens gebeurt op een tekengrens (geen kapot UTF-8).
    #[test]
    fn whisper_capped_on_char_boundary() {
        let short = "kort bericht";
        assert_eq!(cap_whisper(short), short);
        let long: String = "é".repeat(600);
        let capped = cap_whisper(&long);
        assert_eq!(capped.chars().count(), 500);
        assert!(long.starts_with(capped));
    }

    /// Mock-detectie: enkel een loopback-URL telt als de lokale Twitch-mock (footgun-fix).
    #[test]
    fn loopback_ws_detection() {
        assert!(is_loopback_ws("ws://127.0.0.1:8080/ws"));
        assert!(is_loopback_ws("ws://localhost:8080/ws"));
        assert!(!is_loopback_ws("wss://eventsub.wss.twitch.tv/ws"));
    }

    /// #5 — een reeds geziene message_id is een dubbel; een lege id wordt niet gededupt.
    #[test]
    fn seen_deduplicates_message_ids() {
        let mut s = Seen::default();
        assert!(s.insert_new("m1", SEEN_CAP), "eerste keer m1 = nieuw");
        assert!(!s.insert_new("m1", SEEN_CAP), "tweede keer m1 = dubbel");
        assert!(s.insert_new("m2", SEEN_CAP), "andere id = nieuw");
        // Lege id: nooit dedupen (altijd verwerken), en niet bewaren.
        assert!(s.insert_new("", SEEN_CAP), "lege id = altijd verwerken");
        assert!(s.insert_new("", SEEN_CAP), "lege id blijft verwerken");
    }

    /// De set is begrensd (FIFO-eviction): de oudste id valt eruit voorbij de cap.
    #[test]
    fn seen_evicts_oldest_beyond_cap() {
        let mut s = Seen::default();
        let cap = 3;
        for id in ["a", "b", "c"] {
            assert!(s.insert_new(id, cap));
        }
        assert!(s.insert_new("d", cap), "d nieuw → a wordt geëvict");
        assert!(s.insert_new("a", cap), "a was geëvict → weer als nieuw gezien");
        assert!(!s.insert_new("d", cap), "d nog vers → dubbel");
    }
}
