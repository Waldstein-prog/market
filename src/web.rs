//! Axum-site.
//! - `/`            persoonlijke pagina: login-knop, of (na Discord-login) je
//!                  coin-overzicht als Flowerborn, anders de regels-pagina;
//! - `/login`       start de Discord OAuth2-flow;
//! - `/auth/callback` wisselt de code om, haalt de identiteit op, zet een sessie;
//! - `/logout`      sessie wissen;
//! - `/admin`       de Fase-I rol-toggle (intern beheertool, ongewijzigd).
use axum::{
    Json, Router,
    extract::{Query, State},
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{COOKIE, SET_COOKIE},
    },
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use rand::Rng;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::Config;
use crate::db::{self, DbPool};
use crate::discord_rest::Discord;

const SESSION_MAX_AGE: i64 = 90 * 24 * 3600; // ~90 dagen: voelt als "één keer inloggen"
const MEADOW: &str = "#6b9b52";

#[derive(Clone)]
struct AppState {
    cfg: Config,
    dc: Arc<Discord>,
    pool: DbPool,
    http: reqwest::Client,
}

type JsonResp = (StatusCode, Json<Value>);

pub async fn serve(cfg: Config, pool: DbPool) {
    let dc = Arc::new(Discord::new(cfg.bot_token.clone(), cfg.guild_id.clone()));
    let state = AppState {
        cfg,
        dc,
        pool,
        http: reqwest::Client::new(),
    };

    let app = Router::new()
        .route("/", get(index))
        .route("/login", get(login))
        .route("/auth/callback", get(callback))
        .route("/logout", get(logout))
        .route("/admin", get(admin))
        .route("/api/status", get(api_status))
        .route("/api/toggle", post(api_toggle))
        .route("/healthz", get(|| async { "ok" }))
        .with_state(state);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 8700));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("kan poort 8700 niet binden");
    tracing::info!("Web-server luistert op http://0.0.0.0:8700");
    axum::serve(listener, app).await.expect("web-server crashte");
}

// --- helpers ------------------------------------------------------------

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

/// Willekeurige hex-token (sessie-id / CSRF-state).
fn rand_token() -> String {
    let mut rng = rand::thread_rng();
    (0..32)
        .map(|_| char::from_digit(rng.gen_range(0..16), 16).unwrap())
        .collect()
}

/// Percent-encode voor query-waarden (RFC 3986 unreserved blijft staan).
fn pct(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Waarde van één cookie uit de `Cookie`-header.
fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers.get(COOKIE)?.to_str().ok()?.split(';').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k.trim() == name).then(|| v.trim().to_string())
    })
}

fn set_cookie(pair: &str) -> HeaderMap {
    let mut h = HeaderMap::new();
    if let Ok(v) = HeaderValue::from_str(pair) {
        h.insert(SET_COOKIE, v);
    }
    h
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn shell(title: &str, body: &str) -> String {
    format!(
        r#"<!doctype html><html lang="nl"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title}</title><style>
:root{{color-scheme:light dark}}
*{{box-sizing:border-box}}
body{{margin:0;min-height:100vh;display:flex;flex-direction:column;
  font:16px/1.5 system-ui,sans-serif;background:#0e1510;color:#e8f0e4}}
.topbar{{background:{MEADOW};color:#0e1510;font-weight:700;font-size:1.1rem;
  letter-spacing:.02em;padding:.7rem 1.25rem;
  box-shadow:0 2px 10px rgba(0,0,0,.35)}}
.content{{flex:1;display:grid;place-items:center;padding:1rem}}
.card{{background:#182319;border:1px solid #2c3d2a;border-radius:18px;
  padding:2rem 2.25rem;max-width:26rem;width:calc(100% - 2rem);
  box-shadow:0 10px 30px rgba(0,0,0,.35)}}
h1{{margin:.2rem 0 1rem;font-size:1.35rem}}
.coins{{font-size:2.4rem;font-weight:700;color:{MEADOW};margin:.4rem 0}}
.muted{{color:#9db095;font-size:.9rem}}
a.btn,button.btn{{display:inline-block;margin-top:1rem;padding:.7rem 1.15rem;
  border:0;border-radius:12px;background:{MEADOW};color:#0e1510;font-weight:600;
  text-decoration:none;cursor:pointer;font-size:1rem}}
a.link{{color:{MEADOW}}}
</style></head><body><header class="topbar">🌼 Meadow Market</header>
<div class="content"><div class="card">{body}</div></div></body></html>"#
    )
}

// --- pagina's -----------------------------------------------------------

async fn index(State(st): State<AppState>, headers: HeaderMap) -> Html<String> {
    let session = cookie(&headers, "session").and_then(|t| db::get_session(&st.pool, &t));

    let body = match session {
        None => login_body(&st.cfg),
        Some((uid, name)) => {
            let is_flowerborn = st
                .dc
                .has_role(&uid, &st.cfg.role_id)
                .await
                .ok()
                .flatten()
                .unwrap_or(false);
            if is_flowerborn {
                let coins = db::get_coins(&st.pool, &uid);
                account_body(&name, &uid, coins)
            } else {
                rules_body(&name)
            }
        }
    };
    Html(shell("Meadow Market", &body))
}

fn login_body(cfg: &Config) -> String {
    if !cfg.oauth_ready() {
        return "<h1>🌼 Meadow Market</h1><p class=\"muted\">OAuth is nog niet \
            geconfigureerd (client_id/secret ontbreken in secrets.json).</p>"
            .to_string();
    }
    "<h1>🌼 Welkom bij Meadow Market</h1>\
     <p class=\"muted\">Log in met Discord om je coins en inventory te zien. \
     Enkel Flowerborns hebben een account.</p>\
     <a class=\"btn\" href=\"/login\">Inloggen met Discord</a>"
        .to_string()
}

fn account_body(name: &str, uid: &str, coins: i64) -> String {
    format!(
        "<h1>🌼 Hallo, {name}</h1>\
         <p class=\"muted\">Je saldo op dit moment</p>\
         <div class=\"coins\">🪙 {coins}</div>\
         <p class=\"muted\">Discord-ID: {uid}<br>\
         (Inventory &amp; market komen hier later.)</p>\
         <a class=\"link\" href=\"/logout\">Uitloggen</a>",
        name = esc(name),
        uid = esc(uid),
        coins = coins
    )
}

fn rules_body(name: &str) -> String {
    format!(
        "<h1>🌼 Hoi, {name}</h1>\
         <p>Je bent ingelogd, maar je hebt (nog) niet de <b>Flowerborn</b>-rol. \
         Een account op Meadow Market is enkel voor Flowerborns.</p>\
         <p class=\"muted\">Follow the rules: verdien de Flowerborn-rol in de \
         Discord-server, dan verschijnt hier je coin-overzicht.</p>\
         <a class=\"link\" href=\"/logout\">Uitloggen</a>",
        name = esc(name)
    )
}

fn err_page(msg: &str) -> Response {
    let body = format!(
        "<h1>🌼 Er ging iets mis</h1><p>{}</p><a class=\"link\" href=\"/\">Terug</a>",
        esc(msg)
    );
    (StatusCode::BAD_REQUEST, Html(shell("Meadow Market", &body))).into_response()
}

// --- OAuth2-flow --------------------------------------------------------

async fn login(State(st): State<AppState>) -> Response {
    if !st.cfg.oauth_ready() {
        return err_page("OAuth is nog niet geconfigureerd op de server.");
    }
    let state = rand_token();
    let redirect = st.cfg.oauth_redirect();
    let url = format!(
        "https://discord.com/oauth2/authorize?response_type=code\
         &client_id={}&redirect_uri={}&scope=identify&state={}",
        pct(&st.cfg.client_id),
        pct(&redirect),
        pct(&state),
    );
    let c = format!("oauth_state={state}; HttpOnly; SameSite=Lax; Path=/; Max-Age=600");
    (set_cookie(&c), Redirect::to(&url)).into_response()
}

#[derive(Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
}

async fn callback(
    State(st): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<CallbackQuery>,
) -> Response {
    let (Some(code), Some(state)) = (q.code, q.state) else {
        return err_page("Login werd afgebroken of geweigerd.");
    };
    // CSRF: de state moet overeenkomen met de cookie die we bij /login zetten.
    match cookie(&headers, "oauth_state") {
        Some(c) if c == state => {}
        _ => return err_page("Ongeldige of verlopen login-state. Probeer opnieuw."),
    }

    let redirect = st.cfg.oauth_redirect();
    let form = [
        ("client_id", st.cfg.client_id.as_str()),
        ("client_secret", st.cfg.client_secret.as_str()),
        ("grant_type", "authorization_code"),
        ("code", code.as_str()),
        ("redirect_uri", redirect.as_str()),
    ];
    let token: Value = match st
        .http
        .post("https://discord.com/api/v10/oauth2/token")
        .form(&form)
        .send()
        .await
        .and_then(|r| r.error_for_status())
    {
        Ok(r) => match r.json().await {
            Ok(v) => v,
            Err(e) => return err_page(&format!("Token-antwoord onleesbaar: {e}")),
        },
        Err(e) => return err_page(&format!("Token-uitwisseling mislukt: {e}")),
    };
    let access = token["access_token"].as_str().unwrap_or_default();
    if access.is_empty() {
        return err_page("Geen access_token van Discord ontvangen.");
    }

    let me: Value = match st
        .http
        .get("https://discord.com/api/v10/users/@me")
        .header("Authorization", format!("Bearer {access}"))
        .send()
        .await
        .and_then(|r| r.error_for_status())
    {
        Ok(r) => match r.json().await {
            Ok(v) => v,
            Err(e) => return err_page(&format!("Profiel-antwoord onleesbaar: {e}")),
        },
        Err(e) => return err_page(&format!("Profiel ophalen mislukt: {e}")),
    };
    let uid = me["id"].as_str().unwrap_or_default().to_string();
    if uid.is_empty() {
        return err_page("Geen Discord-gebruiker-ID ontvangen.");
    }
    let name = me["global_name"]
        .as_str()
        .filter(|s| !s.is_empty())
        .or_else(|| me["username"].as_str())
        .unwrap_or("onbekend")
        .to_string();

    let sess = rand_token();
    db::create_session(&st.pool, &sess, &uid, &name, now_secs());
    tracing::info!("Login: {name} ({uid})");
    let c = format!(
        "session={sess}; HttpOnly; SameSite=Lax; Path=/; Max-Age={SESSION_MAX_AGE}"
    );
    (set_cookie(&c), Redirect::to("/")).into_response()
}

async fn logout(State(st): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(t) = cookie(&headers, "session") {
        db::delete_session(&st.pool, &t);
    }
    let c = "session=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0";
    (set_cookie(c), Redirect::to("/")).into_response()
}

// --- /admin : Fase-I rol-toggle (ongewijzigd) ---------------------------

async fn admin(State(st): State<AppState>) -> Html<String> {
    let tmpl = include_str!("../templates/index.html");
    let css = include_str!("../static/style.css");
    let pinned_json = serde_json::to_string(&st.cfg.user_id).unwrap_or_else(|_| "\"\"".into());
    let label_json = serde_json::to_string(&st.cfg.role_label).unwrap_or_else(|_| "\"\"".into());
    let html = tmpl
        .replace("{{STYLE}}", css)
        .replace("{{ROLE_LABEL}}", &st.cfg.role_label)
        .replace("{{PINNED_USER_JSON}}", &pinned_json)
        .replace("{{ROLE_LABEL_JSON}}", &label_json);
    Html(html)
}

fn is_digits(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

#[derive(Deserialize)]
struct StatusQuery {
    user_id: String,
}

async fn api_status(State(st): State<AppState>, Query(q): Query<StatusQuery>) -> JsonResp {
    let uid = q.user_id.trim().to_string();
    if !is_digits(&uid) {
        return bad("Geef een geldig Discord user-ID (cijfers).");
    }
    match st.dc.has_role(&uid, &st.cfg.role_id).await {
        Ok(Some(has)) => (StatusCode::OK, Json(json!({"ok": true, "has_role": has}))),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({"ok": false, "error": "Die gebruiker is geen lid van de guild."})),
        ),
        Err(e) => bad(&e),
    }
}

#[derive(Deserialize)]
struct ToggleBody {
    user_id: String,
    enable: bool,
}

async fn api_toggle(State(st): State<AppState>, Json(b): Json<ToggleBody>) -> JsonResp {
    let uid = b.user_id.trim().to_string();
    if !is_digits(&uid) {
        return bad("Geef een geldig Discord user-ID (cijfers).");
    }
    if let Err(e) = st.dc.set_role(&uid, &st.cfg.role_id, b.enable).await {
        return bad(&e);
    }
    match st.dc.has_role(&uid, &st.cfg.role_id).await {
        Ok(Some(has)) => (StatusCode::OK, Json(json!({"ok": true, "has_role": has}))),
        Ok(None) => (StatusCode::OK, Json(json!({"ok": true, "has_role": false}))),
        Err(e) => bad(&e),
    }
}

fn bad(msg: &str) -> JsonResp {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({"ok": false, "error": msg})),
    )
}
