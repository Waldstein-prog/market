//! Discord coin-bot (serenity + poise).
//! - bij CacheReady: logt de guild-ledenlijst;
//! - elk bericht → random 1–3 coins met cooldown per lid (persistent in SQLite);
//! - `!coins` → embed-leaderboard aflopend op coins.
//!
//! Tijdens dev (DEV_FEEDBACK) antwoordt de bot op elk bericht met de coins/cooldown.
use poise::serenity_prelude as serenity;
use rand::Rng;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::Config;
use crate::db::{self, DbPool};

// --- dev-instellingen (later aanpassen) ---------------------------------
const COOLDOWN: f64 = 30.0; // seconden tussen twee toekenningen per lid
const MIN_COINS: i64 = 1;
const MAX_COINS: i64 = 3;
const DEV_FEEDBACK: bool = false; // per bericht coins/cooldown terugkoppelen (dev-only)
const TEST_CHANNEL_ID: u64 = 1253362520530489397; // dev-only: enkel hier coins per bericht (0 = overal)
const LEADERBOARD_SIZE: i64 = 10;
const PREFIX: &str = "!"; // command-prefix; commando's leveren geen coins op
// --- daily-beloning (embed-knop) ----------------------------------------
const DAILY_COOLDOWN: f64 = 24.0 * 3600.0; // 24u tussen twee claims
// Streak-daily: dag 1 = random in [BASE_MIN, BASE_MAX]. Elke opeenvolgende dag
// verhoogt de ondergrens met MIN_STEP en de bovengrens met MAX_STEP. Een dag
// overslaan reset naar dag 1. Na dag STREAK_CAP stopt de verhoging.
const DAILY_BASE_MIN: i64 = 10;
const DAILY_BASE_MAX: i64 = 100;
const DAILY_MIN_STEP: i64 = 1;
const DAILY_MAX_STEP: i64 = 5;
const DAILY_STREAK_CAP: i64 = 200;
const DAILY_CUSTOM_ID: &str = "daily_claim"; // moet matchen met de embed-knop
const COINS_CHANNEL: &str = "coins"; // publiek kanaal voor "X earned N coins today."
// --- treasure chest -----------------------------------------------------
// Chatten ≥ CHEST_DISTINCT_USERS verschillende mensen binnen CHEST_WINDOW in
// hetzelfde (test)kanaal → er verschijnt een chest met een knop. Klikken = meedoen;
// CHEST_POP_DELAY later popt hij en wint één random klikker CHEST_PRIZE coin(s).
const CHEST_ENABLED: bool = true;
const CHEST_DISTINCT_USERS: usize = 2; // TEST-waarde (weinig testers); prod = 3
const CHEST_WINDOW: f64 = 10.0 * 60.0; // venster voor de "verschillende chatters"-telling
const CHEST_POP_DELAY: u64 = 3 * 60; // seconden tussen verschijnen en poppen
const CHEST_CHANNEL_COOLDOWN: f64 = 30.0 * 60.0; // rust per kanaal na een chest (anti-spam)
const CHEST_PRIZE: i64 = 1; // prijs in coins (voorlopig 1)
const CHEST_CUSTOM_ID: &str = "chest_open"; // knop custom_id
// ------------------------------------------------------------------------

pub struct Data {
    pub pool: DbPool,
    pub cfg: Config,
    // Gedeelde treasure-chest-staat (interne mutability; korte sync-secties, nooit
    // over een await vastgehouden).
    chest: Arc<Mutex<ChestTracker>>,
}
type Error = Box<dyn std::error::Error + Send + Sync>;

/// Eén lopende (nog niet gepopte) chest: het kanaal + de deelnemers die klikten.
struct Chest {
    channel_id: u64,
    joiners: Vec<(String, String)>, // (uid, weergavenaam), ontdubbeld op uid
}

/// Per-guild gedeelde chest-boekhouding.
#[derive(Default)]
struct ChestTracker {
    // per kanaal: recente (uid, ts) om "N verschillende chatters binnen het venster" te tellen
    recent: HashMap<u64, Vec<(String, f64)>>,
    // per kanaal: tot wanneer er geen nieuwe chest mag verschijnen (cooldown)
    cooldown_until: HashMap<u64, f64>,
    // kanalen met een chest die nog moet poppen (voorkomt dubbele spawns)
    active: HashSet<u64>,
    // per chest-bericht (message_id) de lopende chest
    chests: HashMap<u64, Chest>,
}
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
    // Ruim het commando-bericht op (properder kanaal). Vereist Manage Messages;
    // faalt het (geen recht), dan tonen we gewoon toch het leaderboard.
    if let poise::Context::Prefix(pctx) = ctx {
        if let Err(e) = pctx.msg.delete(ctx.serenity_context()).await {
            tracing::warn!("kan !coins-bericht niet verwijderen: {e}");
        }
    }
    let rows = db::leaderboard(&ctx.data().pool, LEADERBOARD_SIZE);
    let desc = if rows.is_empty() {
        "No one has coins yet. Send a message to earn some!".to_string()
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
    // dev-test: coins per bericht enkel in het testkanaal (indien gezet)
    if TEST_CHANNEL_ID != 0 && msg.channel_id.get() != TEST_CHANNEL_ID {
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
            msg.reply(ctx, format!("🪙 +{amount} coins! Total: **{total}**"))
                .await?;
        }
    } else {
        let remaining = (COOLDOWN - elapsed) as i64 + 1;
        tracing::info!("{name}: cooldown, nog {remaining}s");
        if DEV_FEEDBACK {
            msg.reply(
                ctx,
                format!("⏳ Cooldown — **{remaining}s** until you can earn coins again."),
            )
            .await?;
        }
    }

    // Elk (geldig) bericht telt mee voor de treasure-chest-detectie, ook tijdens
    // de coin-cooldown.
    maybe_spawn_chest(ctx, msg, data).await?;
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
                } else if mc.data.custom_id == CHEST_CUSTOM_ID {
                    handle_chest_click(ctx, mc, data).await?;
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

    let last = db::get_last_daily(&data.pool, &uid);
    let elapsed = now - last;
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

    // Streak: op tijd (binnen 48u sinds de vorige claim) → +1 dag, anders reset
    // naar dag 1. Eerste claim (last == 0) = dag 1. Gecapt op DAILY_STREAK_CAP.
    let streak = if last <= 0.0 || elapsed >= 2.0 * DAILY_COOLDOWN {
        1
    } else {
        (db::get_daily_streak(&data.pool, &uid) + 1).min(DAILY_STREAK_CAP)
    };
    // Dag N: ondergrens/bovengrens schuiven mee met de streak.
    let step = streak - 1;
    let lo = DAILY_BASE_MIN + step * DAILY_MIN_STEP;
    let hi = DAILY_BASE_MAX + step * DAILY_MAX_STEP;
    let amount = rand::thread_rng().gen_range(lo..=hi);
    let total = db::award_daily(&data.pool, &uid, &name, amount, streak, now);
    let day_word = if streak == 1 { "day" } else { "days" };
    tracing::info!("daily: {name} +{amount} (streak {streak}, totaal {total})");

    respond_ephemeral(
        ctx,
        mc,
        &format!(
            "🔥 **{name}** checked in for **{streak}** {day_word}! \
             You got **{amount}** Meadowcoins today! Balance: **{total}** 🪙"
        ),
    )
    .await?;

    // Publiek regeltje in het #coins-kanaal (indien aanwezig in de guild).
    if let Some(chan) = find_coins_channel(ctx, data).await {
        let _ = chan
            .say(
                &ctx.http,
                format!("{name} checked in for {streak} {day_word} and earned {amount} Meadowcoins!"),
            )
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

/// Registreer de chatter en spawn — bij ≥ CHEST_DISTINCT_USERS verschillende
/// chatters binnen CHEST_WINDOW — een treasure chest (met knop) in het kanaal.
/// Wordt enkel voor geldige (test)kanaal-berichten aangeroepen.
async fn maybe_spawn_chest(
    ctx: &serenity::Context,
    msg: &serenity::Message,
    data: &Data,
) -> Result<(), Error> {
    if !CHEST_ENABLED {
        return Ok(());
    }
    let now = now_secs();
    let chan = msg.channel_id.get();
    let uid = msg.author.id.to_string();

    // Beslis onder de lock: registreer de chatter, prune het venster, tel distinct.
    let spawn = {
        let mut t = data.chest.lock().unwrap();
        let on_cd = t.cooldown_until.get(&chan).is_some_and(|&u| u > now);
        let active = t.active.contains(&chan);
        let distinct = {
            let v = t.recent.entry(chan).or_default();
            v.retain(|(_, ts)| now - *ts < CHEST_WINDOW); // verlopen entries weg
            v.retain(|(u, _)| u != &uid); // oude entry van deze uid weg (verse ts erbij)
            v.push((uid.clone(), now));
            v.iter().map(|(u, _)| u.as_str()).collect::<HashSet<_>>().len()
        };
        let go = !on_cd && !active && distinct >= CHEST_DISTINCT_USERS;
        if go {
            if let Some(v) = t.recent.get_mut(&chan) {
                v.clear(); // venster resetten zodat het niet blijft hertriggeren
            }
            t.active.insert(chan); // meteen reserveren → geen dubbele spawn
        }
        go
    };
    if !spawn {
        return Ok(());
    }

    // Stuur het chest-bericht met de "Open"-knop.
    let button = serenity::CreateButton::new(CHEST_CUSTOM_ID)
        .emoji('🎁')
        .label("Try your luck")
        .style(serenity::ButtonStyle::Success);
    let embed = serenity::CreateEmbed::new()
        .title("🎁 A treasure chest appeared!")
        .description(
            "You lot got the channel buzzing. Click **Try your luck** to hop in — \
             it pops in 3 minutes and one lucky opener wins a prize!",
        )
        .colour(0xF1_C4_0F);
    let builder = serenity::CreateMessage::new()
        .embed(embed)
        .components(vec![serenity::CreateActionRow::Buttons(vec![button])]);
    let sent = match msg.channel_id.send_message(&ctx.http, builder).await {
        Ok(m) => m,
        Err(e) => {
            // Zending mislukt → reservering vrijgeven zodat een volgende poging kan.
            data.chest.lock().unwrap().active.remove(&chan);
            return Err(e.into());
        }
    };
    let msg_id = sent.id.get();

    // Registreer de lopende chest en plan de pop.
    data.chest.lock().unwrap().chests.insert(
        msg_id,
        Chest {
            channel_id: chan,
            joiners: Vec::new(),
        },
    );
    tracing::info!("treasure chest gespawned in kanaal {chan} (bericht {msg_id})");

    let http = ctx.http.clone();
    let pool = data.pool.clone();
    let tracker = data.chest.clone();
    let channel_id = msg.channel_id;
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(CHEST_POP_DELAY)).await;
        pop_chest(http, pool, tracker, channel_id, msg_id).await;
    });
    Ok(())
}

/// Klik op een treasure chest = meedoen aan de trekking (één inschrijving per lid).
async fn handle_chest_click(
    ctx: &serenity::Context,
    mc: &serenity::ComponentInteraction,
    data: &Data,
) -> Result<(), Error> {
    let msg_id = mc.message.id.get();
    let uid = mc.user.id.to_string();
    let name = mc
        .user
        .global_name
        .clone()
        .unwrap_or_else(|| mc.user.name.clone());

    // None = chest bestaat niet (meer); Some(0) = zat er al in; Some(n≥1) = toegevoegd.
    let joined = {
        let mut t = data.chest.lock().unwrap();
        match t.chests.get_mut(&msg_id) {
            None => None,
            Some(c) => {
                if c.joiners.iter().any(|(u, _)| u == &uid) {
                    Some(0)
                } else {
                    c.joiners.push((uid.clone(), name));
                    Some(c.joiners.len())
                }
            }
        }
    };
    let text = match joined {
        None => "📦 Too late — this chest already popped.".to_string(),
        Some(0) => "🎁 You're already in! Sit tight for the pop.".to_string(),
        Some(n) => format!(
            "🎁 You're in! **{n}** {} waiting for the pop.",
            if n == 1 { "opener" } else { "openers" }
        ),
    };
    respond_ephemeral(ctx, mc, &text).await?;
    Ok(())
}

/// Pop de chest (na CHEST_POP_DELAY): kies een random klikker, ken de prijs toe en
/// werk het bericht bij. Geeft het kanaal vrij en zet de anti-spam-cooldown.
async fn pop_chest(
    http: Arc<serenity::Http>,
    pool: DbPool,
    tracker: Arc<Mutex<ChestTracker>>,
    channel_id: serenity::ChannelId,
    msg_id: u64,
) {
    // Haal de chest eruit, geef het kanaal vrij, zet de cooldown.
    let joiners = {
        let mut t = tracker.lock().unwrap();
        let chest = t.chests.remove(&msg_id);
        if let Some(c) = &chest {
            t.active.remove(&c.channel_id);
            t.cooldown_until
                .insert(c.channel_id, now_secs() + CHEST_CHANNEL_COOLDOWN);
        }
        match chest {
            Some(c) => c.joiners,
            None => return, // al opgeruimd (zou niet mogen)
        }
    };

    let embed = if joiners.is_empty() {
        serenity::CreateEmbed::new()
            .title("📦 The chest crumbled to dust")
            .description("Nobody opened it in time.")
            .colour(0x95_A5_A6)
    } else {
        let idx = rand::thread_rng().gen_range(0..joiners.len());
        let (winner_uid, winner_name) = &joiners[idx];
        let total = db::award(&pool, winner_uid, winner_name, CHEST_PRIZE, now_secs());
        let coin_word = if CHEST_PRIZE == 1 { "coin" } else { "coins" };
        let opener_word = if joiners.len() == 1 { "opener" } else { "openers" };
        tracing::info!(
            "chest gepopt: {winner_name} wint {CHEST_PRIZE} coin(s) uit {} deelnemer(s)",
            joiners.len()
        );
        serenity::CreateEmbed::new()
            .title("💰 The chest popped!")
            .description(format!(
                "Out of {} lucky {opener_word}, <@{winner_uid}> wins \
                 **{CHEST_PRIZE} {coin_word}**!\nBalance: **{total}** 🪙",
                joiners.len()
            ))
            .colour(0x6B_9B_52)
    };

    let edit = serenity::EditMessage::new()
        .embeds(vec![embed])
        .components(vec![]); // knop verwijderen
    if let Err(e) = channel_id
        .edit_message(http.as_ref(), serenity::MessageId::new(msg_id), edit)
        .await
    {
        tracing::warn!("kan chest-bericht {msg_id} niet bijwerken: {e}");
    }
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

/// Achtergrondtaak: trek verlopen tijdelijke rollen (bv. 24u-tickets) weer in.
async fn role_grant_sweeper(pool: DbPool, cfg: Config) {
    let dc = crate::discord_rest::Discord::new(cfg.bot_token.clone(), cfg.guild_id.clone());
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        for (id, uid, role) in db::due_role_grants(&pool, now_secs()) {
            match dc.set_role(&uid, &role, false).await {
                Ok(()) => {
                    db::delete_role_grant(&pool, id);
                    tracing::info!("Ticket verlopen: rol {role} ingetrokken bij {uid}");
                }
                Err(e) => tracing::warn!("kan verlopen rol {role} niet intrekken bij {uid}: {e}"),
            }
        }
    }
}

pub async fn run(pool: DbPool, cfg: Config) -> Result<(), Error> {
    let token = cfg.bot_token.clone();
    let intents = serenity::GatewayIntents::GUILDS
        | serenity::GatewayIntents::GUILD_MESSAGES
        | serenity::GatewayIntents::MESSAGE_CONTENT
        | serenity::GatewayIntents::GUILD_MEMBERS;

    // Verlopen tijdelijke rollen periodiek intrekken.
    tokio::spawn(role_grant_sweeper(pool.clone(), cfg.clone()));

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
            Box::pin(async move {
                Ok(Data {
                    pool,
                    cfg,
                    chest: Arc::new(Mutex::new(ChestTracker::default())),
                })
            })
        })
        .build();

    let mut client = serenity::ClientBuilder::new(&token, intents)
        .framework(framework)
        .await?;
    client.start().await?;
    Ok(())
}
