//! Meadow Market — één binary die de Discord-bot (serenity/poise) en de
//! Axum-site concurrent draait, met een gedeelde SQLite-DB.
mod bot;
mod config;
mod db;
mod discord_rest;
mod pass_ledger;
mod playtime;
mod settings;
mod twitch;
mod web;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,serenity=warn,tracing=warn".into()),
        )
        .init();

    let cfg = config::load();
    if cfg.bot_token.is_empty() {
        eprintln!("Geen bot_token in secrets.json/env — afgebroken.");
        std::process::exit(1);
    }

    let pool = db::init_pool("coins.db");

    // Twitch-luik (channel-points-redeem → whitelist-grant): start onafhankelijk van de
    // Discord-gateway, want het is er orthogonaal aan. Alleen als expliciet geconfigureerd
    // (`twitch_ready()`), dus een gewone lokale instance zonder [twitch]-config doet niets.
    // NB: draai niet twee instances met dezelfde Twitch-creds tegelijk (dubbele EventSub).
    if cfg.twitch_ready() {
        let tw_cfg = cfg.clone();
        let tw_pool = pool.clone();
        tokio::spawn(async move {
            twitch::run(tw_pool, tw_cfg).await;
        });
    }

    // Resterende speeltijd per pas komt van de tale-kant (passes.json) — die houdt de klok
    // bij, want alleen de server weet wie er online is. Wij lezen enkel mee, zodat de site
    // dezelfde tijd toont als het spel.
    tokio::spawn(async move {
        pass_ledger::run().await;
    });

    // Totale speeltijd per speler (playtime.json) — dezelfde afspraak: tale telt, wij tonen.
    tokio::spawn(async move {
        playtime::run().await;
    });

    // Web-only modus (lokaal testen): sla de bot-gateway over zodat een tweede
    // instance de live bot niet dubbel op de gateway zet (→ dubbele coins).
    let web_only = std::env::var("MARKET_WEB_ONLY").is_ok_and(|v| v != "0" && !v.is_empty());
    if web_only {
        tracing::warn!("MARKET_WEB_ONLY: enkel de web-server draait, geen bot-gateway.");
        web::serve(cfg, pool).await;
        return;
    }

    // Web-server draait concurrent met de bot-gateway (gedeelde DB-pool).
    let web_cfg = cfg.clone();
    let web_pool = pool.clone();
    let web_task = tokio::spawn(async move {
        web::serve(web_cfg, web_pool).await;
    });

    // De bot blokkeert tot de gateway sluit; valt hij weg, dan stoppen we ook de web-taak.
    if let Err(e) = bot::run(pool, cfg).await {
        eprintln!("Bot gestopt met fout: {e}");
    }
    web_task.abort();
}
