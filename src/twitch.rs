//! Twitch-luik — channel-points-redeem → Hytale-whitelist (Rust-port van het
//! vroegere `tale/bot/twitch_bridge.py`).
//!
//! Een kijker doet een channel-points-redeem "Hytale-ticket (24u)" en typt (de 1e
//! keer) zijn exacte Hytale-naam. We ankeren op de ONVERANDERLIJKE Twitch-user-id en
//! bewaren de grant in market's bestaande `hytale_whitelist`-tabel onder de pseudo-id
//! `twitch:<user_id>`. De tale-bot leest die tabel read-only en whitelistet de naam op
//! de Hytale-server (`reconcile_market`/`enforce_whitelist`) — dit luik raakt de
//! Hytale-FIFO dus NIET meer aan.
//!
//!   * 1e keer      → naam op goed vertrouwen registreren en VASTZETTEN → 24u.
//!   * volgende keer → getypte tekst negeren, gewoon 24u erbij (stapelt, reset niet).
//!   * lege/ongeldige naam → redemption CANCELED ⇒ punten terug + uitleg in de chat.
//!
//! Naast de **dagpas**-reward is er een optionele **permanente-pas**-reward: dezelfde
//! naam-vastzet-flow, maar de grant is permanent (`expires = NULL`). Hij draait enkel als
//! de user een reward-titel invult (`twitch_perma_reward_title`) — anders bestaat hij niet.
//!
//! De reward(s) worden door DEZE app aangemaakt/beheerd (via Helix), want enkel de app die
//! de reward maakte mag redemptions fulfillen/annuleren (anders 403, geen refund).

use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

use crate::config::Config;
use crate::db::{self, DbPool};

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

/// Verloopmoment als lokale-ish HH:MM (best-effort, UTC) voor de chatbevestiging.
fn fmt_hm(expires: f64) -> String {
    let secs = expires as i64;
    let mins = (secs / 60) % 60;
    let hours = (secs / 3600) % 24;
    format!("{hours:02}:{mins:02}")
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
/// → 48u pas i.p.v. 24u. Houdt de laatste `cap` id's bij (FIFO-eviction).
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
    reward_id: String,
    /// Reward-id van de permanente-pas-reward; leeg ⇒ perma-redeem uit.
    perma_reward_id: String,
    pass_hours: u32,
    announce: bool,
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

    async fn set_redemption_status(&self, redemption_id: &str, fulfilled: bool) {
        let status = if fulfilled { "FULFILLED" } else { "CANCELED" };
        if self.mock {
            tracing::info!("[mock] redemption {redemption_id} → {status} (Helix overgeslagen)");
            return;
        }
        let url = format!(
            "{HELIX}/helix/channel_points/custom_rewards/redemptions?broadcaster_id={}&reward_id={}&id={}",
            self.broadcaster_id, self.reward_id, redemption_id
        );
        if let Err(e) = self
            .helix(reqwest::Method::PATCH, &url, Some(&json!({ "status": status })))
            .await
        {
            tracing::warn!("kon redemption-status niet zetten ({status}): {e}");
        }
    }

    async fn chat(&self, msg: &str) {
        if !self.announce {
            return;
        }
        if self.mock {
            tracing::info!("[mock] chat: {msg}");
            return;
        }
        let url = format!("{HELIX}/helix/chat/messages");
        let body = json!({
            "broadcaster_id": self.broadcaster_id,
            "sender_id": self.broadcaster_id,
            "message": msg,
        });
        if let Err(e) = self.helix(reqwest::Method::POST, &url, Some(&body)).await {
            tracing::warn!("kon Twitch-chatbericht niet sturen: {e}");
        }
    }
}

// --- De kern: reageren op een redeem ---------------------------------------
/// Welke pas hoort bij het reward-id van deze redemption? Pure functie zodat de
/// branch-keuze los te testen is. Onbekend/leeg → Day (veilige val: een tijdelijke pas
/// die vanzelf verloopt, nooit per ongeluk permanent).
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum PassKind {
    Day,
    Perma,
}

fn pass_kind_for(reward_id: &str, day_id: &str, perma_id: &str) -> PassKind {
    if !perma_id.is_empty() && reward_id == perma_id {
        PassKind::Perma
    } else {
        let _ = day_id; // day is de default; day_id enkel voor de leesbaarheid/symmetrie
        PassKind::Day
    }
}

async fn on_redeem(ctx: &Ctx, event: &Value) {
    let tid = event.get("user_id").and_then(|x| x.as_str()).unwrap_or("");
    let login = event.get("user_login").and_then(|x| x.as_str()).unwrap_or("");
    let redemption_id = event.get("id").and_then(|x| x.as_str()).unwrap_or("");
    let reward_id = event
        .get("reward")
        .and_then(|r| r.get("id"))
        .and_then(|x| x.as_str())
        .unwrap_or("");
    let user_input = event
        .get("user_input")
        .and_then(|x| x.as_str())
        .unwrap_or("");
    if tid.is_empty() || redemption_id.is_empty() {
        tracing::warn!("Twitch-redeem zonder user_id/id — genegeerd");
        return;
    }
    let kind = pass_kind_for(reward_id, &ctx.reward_id, &ctx.perma_reward_id);
    let uid = format!("twitch:{tid}");

    // Naam vastzetten: bestaat er al een naam voor deze Twitch-id, dan die gebruiken
    // (getypte tekst negeren). Anders de getypte naam opschonen en vastzetten.
    let (name, first_time) = match db::get_whitelist_name(&ctx.pool, &uid) {
        Some(n) => (n, false),
        None => match clean_name(user_input) {
            Some(n) => (n, true),
            None => {
                ctx.set_redemption_status(redemption_id, false).await; // punten terug
                ctx.chat(&format!(
                    "@{login} geen geldige Hytale-naam ingevuld — je punten zijn teruggegeven. \
                     Redeem opnieuw en typ enkel je exacte Hytale-naam."
                ))
                .await;
                tracing::info!("Twitch-redeem geweigerd (lege/ongeldige naam): {login}");
                // Logboek: geweigerde redeem (punten teruggegeven) — voor de audittrail.
                db::log_event(
                    &ctx.pool,
                    now_epoch(),
                    &db::LogEntry::new("twitch", "rejected")
                        .actor(&uid, login)
                        .detail(format!("invalid Hytale name: '{user_input}' — refunded")),
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
            ctx.set_redemption_status(redemption_id, true).await;

            // Logboek: permanente Twitch-pas toegekend — bindt kijker aan Hytale-naam.
            db::log_event(
                &ctx.pool,
                now,
                &db::LogEntry::new("twitch", "whitelist-perma")
                    .actor(&uid, &name)
                    .detail(format!("{login} → {name} · permanent")),
            );

            ctx.chat(&format!(
                "@{login} ✅ permanente toegang voor Hytale-naam '{name}'{reg}. Veel plezier!"
            ))
            .await;
            tracing::info!(
                "Twitch-pas (PERMANENT): {name} ({login}){reg} — grant in hytale_whitelist als {uid}"
            );
        }
        PassKind::Day => {
            let add_secs = ctx.pass_hours as f64 * 3600.0;
            let expires = db::grant_day_whitelist(&ctx.pool, &uid, &name, add_secs, now);
            ctx.set_redemption_status(redemption_id, true).await;

            // Logboek: Twitch-dagpas toegekend (whitelist-grant) — bindt kijker aan Hytale-naam.
            db::log_event(
                &ctx.pool,
                now,
                &db::LogEntry::new("twitch", "whitelist")
                    .actor(&uid, &name)
                    .detail(format!("{login} → {name} · {}h", ctx.pass_hours)),
            );

            ctx.chat(&format!(
                "@{login} ✅ {}u toegang voor Hytale-naam '{name}'{reg}. Actief tot ~{}. Veel plezier!",
                ctx.pass_hours,
                fmt_hm(expires)
            ))
            .await;
            tracing::info!(
                "Twitch-pas: {name} ({login}){reg} — {}u, grant in hytale_whitelist als {uid}",
                ctx.pass_hours
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
            reward_id: "mock_reward".into(),
            // In mock-modus stuurt de CLI de reward-id niet betrouwbaar mee → on_redeem valt
            // terug op Day. Een perma-mocktest zet de reward-id expliciet gelijk aan deze.
            perma_reward_id: if cfg.twitch_perma_enabled() {
                "mock_perma_reward".into()
            } else {
                String::new()
            },
            pass_hours: cfg.twitch_pass_hours(),
            announce: cfg.twitch_announce(),
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
        reward_id: String::new(),
        perma_reward_id: String::new(),
        pass_hours: cfg.twitch_pass_hours(),
        announce: cfg.twitch_announce(),
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

    // Dagpas-reward: zoeken/aanmaken/kost-syncen.
    let reward_title = cfg.twitch_reward_title.clone();
    let reward_id = ensure_reward(&ctx, &broadcaster_id, &reward_title, cfg.twitch_reward_cost()).await?;

    // Permanente-pas-reward: enkel als de user een titel invulde (anders bewust geen
    // tweede reward + geen verzonnen speler-zichtbare tekst).
    let perma_reward_id = if cfg.twitch_perma_enabled() {
        let title = cfg.twitch_perma_reward_title.clone();
        ensure_reward(&ctx, &broadcaster_id, &title, cfg.twitch_perma_reward_cost()).await?
    } else {
        String::new()
    };

    tracing::info!(
        "Twitch-luik actief — kanaal={broadcaster_login}, dagpas-reward='{reward_title}', pas={}u, \
         perma-reward={}, chat={}",
        ctx.pass_hours,
        if perma_reward_id.is_empty() {
            "uit".to_string()
        } else {
            format!("'{}'", cfg.twitch_perma_reward_title)
        },
        if ctx.announce { "aan" } else { "uit" }
    );

    Ok(Ctx { broadcaster_id, reward_id, perma_reward_id, ..ctx })
}

/// Vind de door-deze-app-beheerbare reward met deze titel, of maak ze aan. Synchroniseert
/// de kost met de config (onze app beheert 'm → grijs in de Twitch-UI). Retourneert het
/// reward-id.
async fn ensure_reward(
    ctx: &Ctx,
    broadcaster_id: &str,
    title: &str,
    cost: u32,
) -> Result<String, String> {
    let list_url = format!(
        "{HELIX}/helix/channel_points/custom_rewards?broadcaster_id={broadcaster_id}&only_manageable_rewards=true"
    );
    let rewards = ctx.helix(reqwest::Method::GET, &list_url, None).await?;
    let existing = rewards["data"]
        .as_array()
        .and_then(|arr| arr.iter().find(|r| r["title"].as_str() == Some(title)));

    let id = if let Some(r) = existing {
        let id = r["id"].as_str().unwrap_or("").to_string();
        let cur_cost = r["cost"].as_u64().unwrap_or(0) as u32;
        tracing::info!("Twitch-reward gevonden: '{title}' ({cur_cost} punten, id={id})");
        if cur_cost != cost {
            let upd = format!(
                "{HELIX}/helix/channel_points/custom_rewards?broadcaster_id={broadcaster_id}&id={id}"
            );
            match ctx
                .helix(reqwest::Method::PATCH, &upd, Some(&json!({ "cost": cost })))
                .await
            {
                Ok(_) => tracing::info!("Twitch-reward '{title}' kost bijgewerkt: {cur_cost} → {cost}"),
                Err(e) => tracing::warn!("kon reward-kost '{title}' niet bijwerken: {e}"),
            }
        }
        id
    } else {
        let create_url =
            format!("{HELIX}/helix/channel_points/custom_rewards?broadcaster_id={broadcaster_id}");
        let body = json!({
            "title": title,
            "cost": cost,
            "prompt": "Typ je exacte Hytale-naam (enkel de 1e keer belangrijk; wordt daarna vastgezet).",
            "is_user_input_required": true,
        });
        let created = ctx.helix(reqwest::Method::POST, &create_url, Some(&body)).await?;
        let id = created["data"][0]["id"].as_str().unwrap_or("").to_string();
        tracing::info!("Twitch-reward aangemaakt: '{title}' ({cost} punten, id={id})");
        id
    };
    if id.is_empty() {
        return Err(format!("geen reward-id voor '{title}' (aanmaken/zoeken faalde)"));
    }
    Ok(id)
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
                    // tweede whitelist-grant (= 48u i.p.v. 24u) geven.
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
    // Eén abonnement per (niet-lege) reward — zo krijgen we enkel ónze redemptions binnen,
    // en on_redeem weet aan de reward-id welke pas (dag/permanent) het is.
    for reward_id in [ctx.reward_id.as_str(), ctx.perma_reward_id.as_str()] {
        if reward_id.is_empty() {
            continue;
        }
        let body = json!({
            "type": "channel.channel_points_custom_reward_redemption.add",
            "version": "1",
            "condition": {
                "broadcaster_user_id": ctx.broadcaster_id,
                "reward_id": reward_id,
            },
            "transport": { "method": "websocket", "session_id": session_id },
        });
        ctx.helix(reqwest::Method::POST, &url, Some(&body)).await?;
        tracing::info!("Twitch EventSub: geabonneerd op reward-redemptions (reward_id={reward_id})");
    }
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
    use super::{is_loopback_ws, pass_kind_for, PassKind, Seen, SEEN_CAP};

    /// De reward-id bepaalt dag vs. permanent; onbekend/leeg → veilige val Day.
    #[test]
    fn pass_kind_routing() {
        // Perma enkel bij een exacte, niet-lege match op de perma-reward-id.
        assert_eq!(pass_kind_for("perma", "day", "perma"), PassKind::Perma);
        assert_eq!(pass_kind_for("day", "day", "perma"), PassKind::Day);
        // Onbekende reward → Day (verloopt vanzelf; nooit per ongeluk permanent).
        assert_eq!(pass_kind_for("xyz", "day", "perma"), PassKind::Day);
        // Perma uitgeschakeld (lege perma-id): nooit Perma, ook niet bij een lege reward-id.
        assert_eq!(pass_kind_for("", "day", ""), PassKind::Day);
        assert_eq!(pass_kind_for("day", "day", ""), PassKind::Day);
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
