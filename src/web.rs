//! Axum-site: de kale rol-toggle (Fase I), nu in Rust.
use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::Html,
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

use crate::config::Config;
use crate::discord_rest::Discord;

#[derive(Clone)]
struct AppState {
    cfg: Config,
    dc: Arc<Discord>,
}

type JsonResp = (StatusCode, Json<Value>);

pub async fn serve(cfg: Config) {
    let dc = Arc::new(Discord::new(cfg.bot_token.clone(), cfg.guild_id.clone()));
    let state = AppState { cfg, dc };

    let app = Router::new()
        .route("/", get(index))
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

async fn index(State(st): State<AppState>) -> Html<String> {
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
