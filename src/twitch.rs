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
//! De reward wordt door DEZE app aangemaakt/beheerd (via Helix), want enkel de app die
//! de reward maakte mag redemptions fulfillen/annuleren (anders 403, geen refund).

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

// --- Gedeelde runtime-context ----------------------------------------------
struct Ctx {
    http: reqwest::Client,
    app_id: String,
    app_secret: String,
    tokens_file: PathBuf,
    tok: Mutex<Tokens>,
    broadcaster_id: String,
    reward_id: String,
    pass_hours: u32,
    announce: bool,
    pool: DbPool,
    /// Mock-modus (Twitch CLI EventSub-mock): sla alle echte Helix/token-calls over.
    mock: bool,
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
async fn on_redeem(ctx: &Ctx, event: &Value) {
    let tid = event.get("user_id").and_then(|x| x.as_str()).unwrap_or("");
    let login = event.get("user_login").and_then(|x| x.as_str()).unwrap_or("");
    let redemption_id = event.get("id").and_then(|x| x.as_str()).unwrap_or("");
    let user_input = event
        .get("user_input")
        .and_then(|x| x.as_str())
        .unwrap_or("");
    if tid.is_empty() || redemption_id.is_empty() {
        tracing::warn!("Twitch-redeem zonder user_id/id — genegeerd");
        return;
    }
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
    let add_secs = ctx.pass_hours as f64 * 3600.0;
    let expires = db::grant_day_whitelist(&ctx.pool, &uid, &name, add_secs, now);
    ctx.set_redemption_status(redemption_id, true).await;

    // Logboek: Twitch-pas toegekend (whitelist-grant) — bindt kijker aan Hytale-naam.
    db::log_event(
        &ctx.pool,
        now,
        &db::LogEntry::new("twitch", "whitelist")
            .actor(&uid, &name)
            .detail(format!("{login} → {name} · {}h", ctx.pass_hours)),
    );

    let reg = if first_time { " (naam nu vastgezet)" } else { "" };
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
            pass_hours: cfg.twitch_pass_hours(),
            announce: cfg.twitch_announce(),
            pool,
            mock: true,
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
        pass_hours: cfg.twitch_pass_hours(),
        announce: cfg.twitch_announce(),
        pool,
        mock: false,
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

    // Reward zoeken tussen de door-deze-app-beheerbare rewards; anders aanmaken.
    let reward_title = cfg.twitch_reward_title.clone();
    let reward_cost = cfg.twitch_reward_cost();
    let list_url = format!(
        "{HELIX}/helix/channel_points/custom_rewards?broadcaster_id={broadcaster_id}&only_manageable_rewards=true"
    );
    let rewards = ctx.helix(reqwest::Method::GET, &list_url, None).await?;
    let existing = rewards["data"]
        .as_array()
        .and_then(|arr| arr.iter().find(|r| r["title"].as_str() == Some(&reward_title)));

    let reward_id = if let Some(r) = existing {
        let id = r["id"].as_str().unwrap_or("").to_string();
        let cur_cost = r["cost"].as_u64().unwrap_or(0) as u32;
        tracing::info!("Twitch-reward gevonden: '{reward_title}' ({cur_cost} punten, id={id})");
        // Onze app beheert de kost (grijs in de UI) → synchroniseren met de config/omgeving.
        if cur_cost != reward_cost {
            let upd = format!(
                "{HELIX}/helix/channel_points/custom_rewards?broadcaster_id={broadcaster_id}&id={id}"
            );
            match ctx
                .helix(reqwest::Method::PATCH, &upd, Some(&json!({ "cost": reward_cost })))
                .await
            {
                Ok(_) => tracing::info!("Twitch-reward kost bijgewerkt: {cur_cost} → {reward_cost}"),
                Err(e) => tracing::warn!("kon reward-kost niet bijwerken: {e}"),
            }
        }
        id
    } else {
        let create_url =
            format!("{HELIX}/helix/channel_points/custom_rewards?broadcaster_id={broadcaster_id}");
        let body = json!({
            "title": reward_title,
            "cost": reward_cost,
            "prompt": "Typ je exacte Hytale-naam (enkel de 1e keer belangrijk; wordt daarna vastgezet).",
            "is_user_input_required": true,
        });
        let created = ctx.helix(reqwest::Method::POST, &create_url, Some(&body)).await?;
        let id = created["data"][0]["id"].as_str().unwrap_or("").to_string();
        tracing::info!("Twitch-reward aangemaakt: '{reward_title}' ({reward_cost} punten, id={id})");
        id
    };
    if reward_id.is_empty() {
        return Err("geen reward-id (aanmaken/zoeken faalde)".into());
    }

    tracing::info!(
        "Twitch-luik actief — kanaal={broadcaster_login}, reward='{reward_title}', pas={}u, chat={}",
        ctx.pass_hours,
        if ctx.announce { "aan" } else { "uit" }
    );

    Ok(Ctx { broadcaster_id, reward_id, ..ctx })
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

    while let Some(msg) = read.next().await {
        let msg = msg.map_err(|e| format!("ws-lees fout: {e}"))?;
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
                    on_redeem(ctx, &v["payload"]["event"]).await;
                }
            }
            "revocation" => {
                tracing::warn!("Twitch EventSub: abonnement ingetrokken: {}", text);
            }
            _ => {} // session_keepalive e.d. negeren
        }
    }
    Ok(None)
}

async fn subscribe_redemptions(ctx: &Ctx, session_id: &str) -> Result<(), String> {
    let url = format!("{HELIX}/helix/eventsub/subscriptions");
    let body = json!({
        "type": "channel.channel_points_custom_reward_redemption.add",
        "version": "1",
        "condition": {
            "broadcaster_user_id": ctx.broadcaster_id,
            "reward_id": ctx.reward_id,
        },
        "transport": { "method": "websocket", "session_id": session_id },
    });
    ctx.helix(reqwest::Method::POST, &url, Some(&body)).await?;
    tracing::info!("Twitch EventSub: geabonneerd op reward-redemptions");
    Ok(())
}

/// Entrypoint: gestart vanuit main als `cfg.twitch_ready()`. Zelf-herstellend:
/// verbindt opnieuw met backoff en ververst het token periodiek.
pub async fn run(pool: DbPool, cfg: Config) {
    // Test-hook: wijs naar de Twitch CLI EventSub-mock (bv. ws://127.0.0.1:8080/ws).
    // Gezet ⇒ mock-modus (geen echte Helix/token nodig).
    let start_url = std::env::var("TWITCH_EVENTSUB_URL").unwrap_or_else(|_| EVENTSUB_WS.to_string());
    let mock = std::env::var("TWITCH_EVENTSUB_URL").is_ok();

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
