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
//!   * volgende keer → getypte tekst negeren, gewoon tijd erbij (stapelt, reset niet).
//!   * lege/ongeldige naam → geen grant, wél een `twitch/rejected`-regel in het logboek.
//!
//! ## De streamer bezit de reward, wij niet (omslag 2026-08-03)
//! Vroeger maakte deze app de reward zelf aan via Helix. Nu maakt de **streamer** ze aan
//! in haar eigen dashboard (met "kijker moet tekst invullen" aan) en herkent market ze aan
//! de **titel** uit `settings` (Manage → ⚙ Settings). Gevolgen, bewust aanvaard:
//!
//!   * We kunnen redemptions **niet** meer fulfillen of annuleren — Helix laat dat enkel toe
//!     aan de app die de reward maakte (anders 403). **Terugbetalen gebeurt dus manueel** in
//!     de Twitch-wachtrij; het logboek zegt wanneer dat nodig is.
//!   * We abonneren **breed** (alle redemptions van het kanaal) en filteren zelf op titel.
//!     Zo werkt een hernoemde of pas aangemaakte reward meteen, zonder herstart.
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

const HELIX: &str = "https://helix.twitch.tv";
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

/// Wijst een EventSub-URL naar een loopback-adres? (→ de lokale Twitch CLI-mock.)
fn is_loopback_ws(url: &str) -> bool {
    url.contains("127.0.0.1") || url.contains("localhost") || url.contains("[::1]")
}

/// Titels vergelijken zoals een mens ze bedoelt: spaties eromheen en hoofdletters
/// mogen niet uitmaken. De streamer typt de titel twee keer over (in Twitch en in de
/// settings) — een verschil in kapitaal mag de pas niet stil laten mislukken.
fn title_matches(a: &str, b: &str) -> bool {
    !b.trim().is_empty() && a.trim().eq_ignore_ascii_case(b.trim())
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
    /// De reward-titels, de pasduur en de whisper-tekst staan bewust NIET hier: die
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

fn pass_kind_for(title: &str, day_title: &str, perma_title: &str) -> Option<PassKind> {
    // Perma eerst: staan beide titels per ongeluk gelijk, dan is permanent geven
    // erger dan een paar uur geven — maar de keuze moet vastliggen, niet toevallig zijn.
    if title_matches(title, perma_title) {
        Some(PassKind::Perma)
    } else if title_matches(title, day_title) {
        Some(PassKind::Day)
    } else {
        None
    }
}

async fn on_redeem(ctx: &Ctx, event: &Value) {
    let tid = event.get("user_id").and_then(|x| x.as_str()).unwrap_or("");
    let login = event.get("user_login").and_then(|x| x.as_str()).unwrap_or("");
    let title = event
        .get("reward")
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

    // Titels vers uit de settings: hernoemt de streamer haar reward, dan volstaat het
    // veld op de Settings-pagina aanpassen.
    let day_title = settings::str_of(&ctx.pool, "twitch_reward_title");
    let perma_title = settings::str_of(&ctx.pool, "twitch_perma_reward_title");
    let Some(kind) = pass_kind_for(title, &day_title, &perma_title) else {
        // Elke andere beloning van het kanaal komt hier ook binnen; dat is geen fout.
        // Wel loggen wát er langskwam: bij een titelverschil is dit de enige aanwijzing.
        tracing::info!(
            "Twitch-redeem '{title}' van {login} genegeerd — komt niet overeen met de \
             ingestelde reward-titel(s)"
        );
        return;
    };
    let uid = format!("twitch:{tid}");

    // Naam vastzetten: bestaat er al een naam voor deze Twitch-id, dan die gebruiken
    // (getypte tekst negeren). Anders de getypte naam opschonen en vastzetten.
    let (name, first_time) = match db::get_whitelist_name(&ctx.pool, &uid) {
        Some(n) => (n, false),
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
    let day = settings::str_of(&ctx.pool, "twitch_reward_title");
    let perma = settings::str_of(&ctx.pool, "twitch_perma_reward_title");
    tracing::info!(
        "Twitch-luik actief — kanaal={broadcaster_login}, reward-titel={}, perma-titel={}, pas={}u",
        if day.is_empty() { "(leeg — redeems worden genegeerd)".into() } else { format!("'{day}'") },
        if perma.is_empty() { "(uit)".into() } else { format!("'{perma}'") },
        settings::i64_of(&ctx.pool, "twitch_pass_hours"),
    );
    // Diagnose-hulp: wélke rewards het kanaal heeft. Een titel die net niet klopt is
    // anders enkel te zien aan redeems die stil genegeerd worden.
    log_channel_rewards(&ctx).await;

    Ok(ctx)
}

/// Log de titels van de channel-points-rewards van het kanaal. Puur informatief: we
/// maken en beheren ze niet meer, maar zo staat in de log waar de titel op moet lijken.
/// Mislukt de call (scope/rechten), dan is dat geen reden om het luik niet te starten.
async fn log_channel_rewards(ctx: &Ctx) {
    let url = format!(
        "{HELIX}/helix/channel_points/custom_rewards?broadcaster_id={}",
        ctx.broadcaster_id
    );
    match ctx.helix(reqwest::Method::GET, &url, None).await {
        Ok(v) => {
            let titles: Vec<String> = v["data"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|r| r["title"].as_str())
                        .map(|t| format!("'{t}'"))
                        .collect()
                })
                .unwrap_or_default();
            if titles.is_empty() {
                tracing::warn!(
                    "Twitch: het kanaal heeft (nog) geen channel-points-rewards — de streamer \
                     moet ze zelf aanmaken, met 'kijker moet tekst invullen' aan."
                );
            } else {
                tracing::info!("Twitch-rewards op het kanaal: {}", titles.join(", "));
            }
        }
        Err(e) => tracing::warn!("kon de reward-lijst niet ophalen (enkel diagnose): {e}"),
    }
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
    // laten het weg, want we kénnen de id van de streamer haar reward niet — zij maakt ze
    // aan, niet wij. We krijgen dus álle redemptions van het kanaal binnen en `on_redeem`
    // filtert op titel. Bijkomend voordeel: een reward die pas ná de start wordt aangemaakt
    // of hernoemd werkt meteen, zonder herstart.
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
        cap_whisper, fill_template, is_loopback_ws, pass_kind_for, PassKind, Seen, SEEN_CAP,
    };

    /// De reward-TITEL bepaalt dag vs. permanent vs. "niet van ons". Een lege
    /// ingestelde titel matcht nooit — dat is precies hoe je zo'n redeem uitzet.
    #[test]
    fn pass_kind_routing() {
        let day = "Hytale pass";
        let perma = "Hytale forever";
        assert_eq!(pass_kind_for(day, day, perma), Some(PassKind::Day));
        assert_eq!(pass_kind_for(perma, day, perma), Some(PassKind::Perma));
        // Elke andere beloning van het kanaal → niets doen.
        assert_eq!(pass_kind_for("Song request", day, perma), None);
        // Hoofdletters/spaties mogen niet uitmaken: de titel wordt twee keer overgetypt.
        assert_eq!(pass_kind_for("  hytale PASS ", day, perma), Some(PassKind::Day));
        // Perma-titel leeg = die redeem bestaat niet; een lege reward-titel matcht nooit.
        assert_eq!(pass_kind_for(perma, day, ""), None);
        assert_eq!(pass_kind_for("", day, perma), None);
        // Beide instellingen leeg ⇒ market doet niets, ook niet bij een lege titel.
        assert_eq!(pass_kind_for("", "", ""), None);
        assert_eq!(pass_kind_for("Anything", "", ""), None);
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
