//! Discord coin-bot (serenity + poise).
//! - bij CacheReady: logt de guild-ledenlijst;
//! - elk bericht → random 1–3 coins met cooldown per lid (persistent in SQLite);
//! - `!coins` → embed-leaderboard aflopend op coins.
//!
//! Tijdens dev (DEV_FEEDBACK) antwoordt de bot op elk bericht met de coins/cooldown.
use poise::serenity_prelude as serenity;
use rand::Rng;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::Config;
use crate::db::{self, DbPool};

// --- dev-instellingen (later aanpassen) ---------------------------------
const COOLDOWN: f64 = 10.0; // seconden tussen twee toekenningen per lid (prod: 30)
const MIN_COINS: i64 = 1;
const MAX_COINS: i64 = 3;
const DEV_FEEDBACK: bool = true; // per bericht coins/cooldown terugkoppelen; later false
const LEADERBOARD_SIZE: i64 = 10;
// ------------------------------------------------------------------------

pub struct Data {
    pub pool: DbPool,
    pub cfg: Config,
}
type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

/// `!coins` — leaderboard-embed.
#[poise::command(prefix_command)]
pub async fn coins(ctx: Context<'_>) -> Result<(), Error> {
    tracing::info!("!coins opgevraagd door {}", ctx.author().name);
    let rows = db::leaderboard(&ctx.data().pool, LEADERBOARD_SIZE);
    let desc = if rows.is_empty() {
        "Nog niemand heeft coins. Stuur een bericht om er te verdienen!".to_string()
    } else {
        rows.iter()
            .enumerate()
            .map(|(i, (u, c))| {
                let rank = match i {
                    0 => "🥇".to_string(),
                    1 => "🥈".to_string(),
                    2 => "🥉".to_string(),
                    n => format!("**{}.**", n + 1),
                };
                format!("{rank} {u} — **{c}** coins")
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let embed = serenity::CreateEmbed::new()
        .title("🪙 Meadow Market — Coin Leaderboard")
        .description(desc)
        .colour(0x6B_9B_52);
    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}

async fn handle_message(
    ctx: &serenity::Context,
    msg: &serenity::Message,
    data: &Data,
) -> Result<(), Error> {
    if msg.author.bot {
        return Ok(());
    }
    // enkel binnen de geconfigureerde guild (indien gezet)
    if !data.cfg.guild_id.is_empty() {
        match msg.guild_id {
            Some(g) if g.to_string() == data.cfg.guild_id => {}
            _ => return Ok(()),
        }
    }

    let now = now_secs();
    let uid = msg.author.id.to_string();
    let elapsed = now - db::get_last_award(&data.pool, &uid);
    let name = msg
        .author
        .global_name
        .clone()
        .unwrap_or_else(|| msg.author.name.clone());

    if elapsed >= COOLDOWN {
        let amount = rand::thread_rng().gen_range(MIN_COINS..=MAX_COINS);
        let total = db::award(&data.pool, &uid, &name, amount, now);
        tracing::info!("{name}: +{amount} coins (totaal {total})");
        if DEV_FEEDBACK {
            msg.reply(ctx, format!("🪙 +{amount} coins! Totaal: **{total}**"))
                .await?;
        }
    } else {
        let remaining = (COOLDOWN - elapsed) as i64 + 1;
        tracing::info!("{name}: cooldown, nog {remaining}s");
        if DEV_FEEDBACK {
            msg.reply(
                ctx,
                format!("⏳ Cooldown — nog **{remaining}s** tot je weer coins krijgt."),
            )
            .await?;
        }
    }
    Ok(())
}

async fn event_handler(
    ctx: &serenity::Context,
    event: &serenity::FullEvent,
    _framework: poise::FrameworkContext<'_, Data, Error>,
    data: &Data,
) -> Result<(), Error> {
    match event {
        serenity::FullEvent::Ready { data_about_bot } => {
            tracing::info!(
                "Ingelogd als {} ({})",
                data_about_bot.user.name,
                data_about_bot.user.id
            );
        }
        serenity::FullEvent::CacheReady { guilds } => {
            for gid in guilds {
                // Leden actief ophalen via HTTP: de cache is bij CacheReady nog niet
                // gevuld met members (chunking loopt nog).
                let gname = ctx
                    .cache
                    .guild(*gid)
                    .map(|g| g.name.clone())
                    .unwrap_or_else(|| gid.to_string());
                match gid.members(&ctx.http, None, None).await {
                    Ok(members) => {
                        let humans: Vec<_> = members.iter().filter(|m| !m.user.bot).collect();
                        tracing::info!("Guild '{}' — {} menselijke leden:", gname, humans.len());
                        for m in &humans {
                            tracing::info!("    - {} ({})", m.display_name(), m.user.id);
                        }
                    }
                    Err(e) => tracing::warn!("kan leden niet ophalen voor {gname}: {e}"),
                }
            }
        }
        serenity::FullEvent::Message { new_message } => {
            handle_message(ctx, new_message, data).await?;
        }
        _ => {}
    }
    Ok(())
}

pub async fn run(pool: DbPool, cfg: Config) -> Result<(), Error> {
    let token = cfg.bot_token.clone();
    let intents = serenity::GatewayIntents::GUILDS
        | serenity::GatewayIntents::GUILD_MESSAGES
        | serenity::GatewayIntents::MESSAGE_CONTENT
        | serenity::GatewayIntents::GUILD_MEMBERS;

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![coins()],
            prefix_options: poise::PrefixFrameworkOptions {
                prefix: Some("!".to_string()),
                ..Default::default()
            },
            event_handler: |ctx, event, framework, data| {
                Box::pin(event_handler(ctx, event, framework, data))
            },
            ..Default::default()
        })
        .setup(move |_ctx, _ready, _framework| {
            Box::pin(async move { Ok(Data { pool, cfg }) })
        })
        .build();

    let mut client = serenity::ClientBuilder::new(&token, intents)
        .framework(framework)
        .await?;
    client.start().await?;
    Ok(())
}
