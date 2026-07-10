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
const COOLDOWN: f64 = 30.0; // seconden tussen twee toekenningen per lid
const MIN_COINS: i64 = 1;
const MAX_COINS: i64 = 3;
const DEV_FEEDBACK: bool = false; // per bericht coins/cooldown terugkoppelen (dev-only)
const LEADERBOARD_SIZE: i64 = 10;
const PREFIX: &str = "!"; // command-prefix; commando's leveren geen coins op
// --- daily-beloning (embed-knop) ----------------------------------------
const DAILY_COOLDOWN: f64 = 24.0 * 3600.0; // 24u tussen twee claims
const DAILY_MIN: i64 = 1; // makkelijk op te schroeven als de daily groter mag
const DAILY_MAX: i64 = 3;
const DAILY_CUSTOM_ID: &str = "daily_claim"; // moet matchen met de embed-knop
const COINS_CHANNEL: &str = "coins"; // publiek kanaal voor "X earned N coins today."
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
    // Commando's (!coins e.d.) zijn immuun: geen coins, cooldown onaangeroerd.
    if msg.content.starts_with(PREFIX) {
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
        serenity::FullEvent::InteractionCreate { interaction } => {
            if let serenity::Interaction::Component(mc) = interaction {
                if mc.data.custom_id == DAILY_CUSTOM_ID {
                    handle_daily(ctx, mc, data).await?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// Klik op de "Daily"-knop in de embed: éénmaal per 24u coins, met een
/// ephemeral bevestiging voor de speler + een publiek regeltje in #coins.
async fn handle_daily(
    ctx: &serenity::Context,
    mc: &serenity::ComponentInteraction,
    data: &Data,
) -> Result<(), Error> {
    let now = now_secs();
    let uid = mc.user.id.to_string();
    let name = mc
        .user
        .global_name
        .clone()
        .unwrap_or_else(|| mc.user.name.clone());

    let elapsed = now - db::get_last_daily(&data.pool, &uid);
    if elapsed < DAILY_COOLDOWN {
        let left = DAILY_COOLDOWN - elapsed;
        let hrs = (left / 3600.0).floor() as i64;
        let mins = ((left % 3600.0) / 60.0).floor() as i64;
        respond_ephemeral(
            ctx,
            mc,
            &format!("🎁 You already claimed your daily. Come back in **{hrs}h {mins}m**."),
        )
        .await?;
        return Ok(());
    }

    let amount = rand::thread_rng().gen_range(DAILY_MIN..=DAILY_MAX);
    let total = db::award_daily(&data.pool, &uid, &name, amount, now);
    let unit = if amount == 1 { "coin" } else { "coins" };
    tracing::info!("daily: {name} +{amount} (totaal {total})");

    respond_ephemeral(
        ctx,
        mc,
        &format!("🎁 You earned **{amount}** {unit} today! Your balance is now **{total}** 🪙"),
    )
    .await?;

    // Publiek regeltje in het #coins-kanaal (indien aanwezig in de guild).
    if let Some(chan) = find_coins_channel(ctx, data).await {
        let _ = chan
            .say(&ctx.http, format!("{name} earned {amount} {unit} today."))
            .await;
    }
    Ok(())
}

/// Antwoord op een component-interactie met een privé (ephemeral) bericht.
async fn respond_ephemeral(
    ctx: &serenity::Context,
    mc: &serenity::ComponentInteraction,
    text: &str,
) -> Result<(), Error> {
    mc.create_response(
        &ctx.http,
        serenity::CreateInteractionResponse::Message(
            serenity::CreateInteractionResponseMessage::new()
                .ephemeral(true)
                .content(text),
        ),
    )
    .await?;
    Ok(())
}

/// Zoek het tekstkanaal met de naam `COINS_CHANNEL` in de geconfigureerde guild.
async fn find_coins_channel(ctx: &serenity::Context, data: &Data) -> Option<serenity::ChannelId> {
    let raw: u64 = data.cfg.guild_id.parse().ok()?;
    if raw == 0 {
        return None;
    }
    let gid = serenity::GuildId::new(raw);
    let channels = gid.channels(&ctx.http).await.ok()?;
    channels
        .into_iter()
        .find(|(_, ch)| ch.name == COINS_CHANNEL && ch.kind == serenity::ChannelType::Text)
        .map(|(id, _)| id)
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
                prefix: Some(PREFIX.to_string()),
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
