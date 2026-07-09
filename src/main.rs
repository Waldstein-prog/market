//! Meadow Market — één binary die de Discord-bot (serenity/poise) en de
//! Axum-site concurrent draait, met een gedeelde SQLite-DB.
mod bot;
mod config;
mod db;
mod discord_rest;
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

    // Web-server draait concurrent met de bot-gateway.
    let web_cfg = cfg.clone();
    let web_task = tokio::spawn(async move {
        web::serve(web_cfg).await;
    });

    // De bot blokkeert tot de gateway sluit; valt hij weg, dan stoppen we ook de web-taak.
    if let Err(e) = bot::run(pool, cfg).await {
        eprintln!("Bot gestopt met fout: {e}");
    }
    web_task.abort();
}
