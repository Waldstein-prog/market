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
// Gewogen kans per bericht: 80% → 1 coin, 19% → 2 coins, 1% → 3 coins. Som = 100.
const COIN_WEIGHTS: [(u32, i64); 3] = [(80, 1), (19, 2), (1, 3)];
const DEV_FEEDBACK: bool = false; // cooldown-terugkoppeling per bericht (dev-only; laat uit → geen ⏳-spam in #general)
const COIN_FEEDBACK: bool = false; // toon de speler in #general zijn coin-award ("+N coins! Total: X")
const COIN_CHANNEL_ID: u64 = 1229046340793663488; // #general: enkel hier coins per bericht (0 = overal). De chest-detectie volgt ditzelfde kanaal.
const FORTUNA_LOG_CHANNEL_ID: u64 = 1526181603624226938; // Magic Meadow #fortuna-log: elke coin-verdienste (0 = uit)
const MEADOWMARKET_LOG_CHANNEL_ID: u64 = 0; // saldo-log uit op prod (fortuna-log dekt de verdiensten)
const PROD_COINS_CHANNEL_ID: u64 = 1403044480218824794; // Magic Meadow 🪙meadowcoins (shout-out + level-up + weekly)
const PROD_GENERAL_CHANNEL_ID: u64 = 1296469405651435594; // Magic Meadow ☀️general (weekly zaterdag 15u)
const PROD_GUILD_ID: u64 = 1296469405651435592; // Magic Meadow — leave/rejoin-archief triggert enkel hier
const HOURLY_SHOUTOUT_MIN: i64 = 100; // drempel voor de uurlijkse shout-out (coins verdiend in het afgelopen uur)
// TEST-modus: vuur elke HOURLY_TEST_INTERVAL sec met venster = die interval en een
// lage drempel, i.p.v. op het uur. Zet op false voor prod (dan HH:01 + ≥100/uur).
const HOURLY_SHOUTOUT_TEST: bool = false;
const HOURLY_TEST_INTERVAL: f64 = 2.0 * 60.0; // test: interval én venster (s)
const HOURLY_TEST_MIN: i64 = 3; // test-drempel
// De custom Meadowcoins-emoji (guild-emoji). Bots moeten <:naam:id> sturen, niet :naam:.
const COIN_EMOJI: &str = "<:Meadowcoins:1526188363110023308>"; // Magic Meadow-emoji; bot zit in prod → rendert op beide guilds
const PREFIX: &str = "!"; // deze berichten leveren geen coins op (oude commando-syntax)
// --- daily-beloning (embed-knop) ----------------------------------------
const DAILY_COOLDOWN: f64 = 20.0 * 3600.0; // minstens 20u tussen twee claims
const DAILY_STREAK_WINDOW: f64 = 30.0 * 3600.0; // binnen 30u opnieuw klikken → streak behouden (anders reset)
// Streak-daily: dag 1 = random in [BASE_MIN, BASE_MAX]. Elke opeenvolgende dag
// verhoogt de ondergrens met MIN_STEP en de bovengrens met MAX_STEP. Een dag
// overslaan reset naar dag 1. Na dag STREAK_CAP stopt de verhoging.
const DAILY_BASE_MIN: i64 = 10;
const DAILY_BASE_MAX: i64 = 100;
const DAILY_MIN_STEP: i64 = 1;
const DAILY_MAX_STEP: i64 = 5;
const DAILY_STREAK_CAP: i64 = 200;
const DAILY_CUSTOM_ID: &str = "daily_claim"; // moet matchen met de embed-knop
const SITE_ACCESS_CUSTOM_ID: &str = "site_access"; // "site"-knop → under-construction (website nog niet open)
const MARKET_INFO_CUSTOM_ID: &str = "market_info"; // "Info"-knop → ephemeral uitleg
// --- treasure chest -----------------------------------------------------
// Chatten ≥ CHEST_DISTINCT_USERS verschillende mensen binnen CHEST_WINDOW in
// hetzelfde (test)kanaal → er verschijnt een chest met een knop. Klikken = meedoen;
// CHEST_POP_DELAY later popt hij en wint één random klikker CHEST_PRIZE coin(s).
const CHEST_ENABLED: bool = true;
const CHEST_DISTINCT_USERS: usize = 3; // aantal verschillende chatters binnen CHEST_WINDOW om te spawnen
const CHEST_WINDOW: f64 = 10.0 * 60.0; // venster voor de "verschillende chatters"-telling
const CHEST_POP_DELAY: u64 = 10 * 60; // seconden tussen verschijnen en poppen. Embedtekst leest dit dynamisch.
const CHEST_CHANNEL_COOLDOWN: f64 = 50.0 * 60.0; // rust per kanaal na een chest (anti-spam)
const CHEST_MIN_JOINERS: usize = 2; // minstens zoveel deelnemers, anders despawnt de chest (niks weggegeven)
const CHEST_SPAWN_ON_START: bool = false; // (was test) — nu vervangen door het !chest dev-commando
const CHEST_CUSTOM_ID: &str = "chest_open"; // knop custom_id
// Artwork ingebakken in de binary (geen losse bestanden bij deploy nodig). Gehangen
// als attachments aan het chest-bericht en via attachment:// in de embed getoond:
// chest = grote image (onderaan), coin = thumbnail (rechtsboven).
const CHEST_IMG: &[u8] = include_bytes!("../artwork/treasure chest.png");
const CHEST_COIN_IMG: &[u8] = include_bytes!("../artwork/coin.png");
const CRYING_IMG: &[u8] = include_bytes!("../artwork/crying.png"); // getoond als de chest despawnt
// Prijsverdeling: (gewicht in ‰ (per duizend), min, max coins). Som = CHEST_TIER_TOTAL.
// CHEST_TIERS = de ACTUELE (live) verdeling die de trekking gebruikt (coarse, zoals
// gevraagd). CHEST_TIERS_PROPOSAL = een fijnkorreliger VOORSTEL, enkel getoond in de
// !chest-embed (nog niet actief). Beide tonen in de embed, coarse boven, voorstel onder.
const CHEST_TIER_TOTAL: u32 = 1000; // som van de gewichten (‰)
const CHEST_TIERS: [(u32, i64, i64); 5] = [
    (700, 50, 100),   // 70%
    (200, 100, 300),  // 20%
    (50, 300, 500),   // 5%
    (40, 500, 800),   // 4%
    (10, 800, 1000),  // 1%
];
const CHEST_TIERS_PROPOSAL: [(u32, i64, i64); 10] = [
    (400, 50, 80),    // 40%
    (220, 80, 120),   // 22%
    (140, 120, 180),  // 14%
    (90, 180, 260),   // 9%
    (60, 260, 360),   // 6%
    (40, 360, 480),   // 4%
    (25, 480, 620),   // 2.5%
    (15, 620, 760),   // 1.5%
    (7, 760, 880),    // 0.7%
    (3, 880, 1000),   // 0.3%
];
// Dev-guild (WaldsteinDevZone): het !chest-overzichtscommando werkt ENKEL hier en
// nooit op een latere prod-guild (harde snowflake-check, niet config-afhankelijk).
const DEV_GUILD_ID: u64 = 652452615879262220;
// ------------------------------------------------------------------------

pub struct Data {
    pub pool: DbPool,
    #[allow(dead_code)] // behouden voor toekomstige config-afhankelijke features
    pub cfg: Config,
    // Gedeelde treasure-chest-staat (interne mutability; korte sync-secties, nooit
    // over een await vastgehouden).
    chest: Arc<Mutex<ChestTracker>>,
}
type Error = Box<dyn std::error::Error + Send + Sync>;

/// Eén lopende (nog niet gepopte) chest: het kanaal, de deelnemers die klikten,
/// en het pop-tijdstip (unix) zodat de embed bij elke klik met dezelfde live
/// aftel-timer herbouwd kan worden.
struct Chest {
    channel_id: u64,
    joiners: Vec<(String, String)>, // (uid, weergavenaam), ontdubbeld op uid
    pop_ts: i64,
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

/// Gewogen coin-award per bericht volgens COIN_WEIGHTS (80/19/1 %).
fn coin_amount() -> i64 {
    let mut roll = rand::thread_rng().gen_range(0..100);
    for (w, n) in COIN_WEIGHTS {
        if roll < w {
            return n;
        }
        roll -= w;
    }
    1 // vangnet (som != 100)
}

/// Log een coin-verdienste: `got N coins` → #fortuna-log, `balance: X` →
/// #meadowmarket-log (getallen in vet). Gebruikt voor berichten, daily én chest.
async fn log_earn(http: &serenity::Http, name: &str, amount: i64, total: i64) {
    if FORTUNA_LOG_CHANNEL_ID != 0 {
        let _ = serenity::ChannelId::new(FORTUNA_LOG_CHANNEL_ID)
            .say(http, format!("{name} + **{amount}** {COIN_EMOJI}"))
            .await;
    }
    if MEADOWMARKET_LOG_CHANNEL_ID != 0 {
        let _ = serenity::ChannelId::new(MEADOWMARKET_LOG_CHANNEL_ID)
            .say(http, format!("{name} balance: **{total}** {COIN_EMOJI}"))
            .await;
    }
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
    // Coins per bericht enkel in kanalen op de admin-beheerde coin-kanalenlijst.
    // Lege lijst = nergens coins (progressieve activering).
    if !db::is_coin_channel(&data.pool, msg.channel_id.get()) {
        return Ok(());
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
        let old_earned = db::get_stats(&data.pool, &uid).3; // total_earned vóór award
        let amount = coin_amount();
        let total = db::award(&data.pool, &uid, &name, amount, now);
        tracing::info!("{name}: +{amount} coins (totaal {total})");
        log_earn(&ctx.http, &name, amount, total).await;
        // Level-up? → 1% van het saldo cadeau + privé-melding (DM) aan het lid.
        let new_level = db::level_of(old_earned + amount);
        if new_level > db::level_of(old_earned) {
            let gift = total / 100; // 1% van het saldo
            if gift > 0 {
                db::admin_add_coins(&data.pool, &uid, &name, gift);
            }
            let new_bal = total + gift;
            let txt = if gift > 0 {
                format!("🎉 <@{uid}> reached **Level {new_level}**! A **1% bonus** landed in their purse: **+{gift}** {COIN_EMOJI} — balance now **{new_bal}**.")
            } else {
                format!("🎉 <@{uid}> reached **Level {new_level}**! 🚀")
            };
            // NOOIT een DM. Publiek bericht in het kanaal waar men levelde + in prod #coins.
            let _ = msg.channel_id.say(&ctx.http, txt.as_str()).await;
            // Ook in prod #coins, tenzij men net dáár levelde (geen dubbel bericht).
            if PROD_COINS_CHANNEL_ID != 0 && msg.channel_id.get() != PROD_COINS_CHANNEL_ID {
                let _ = serenity::ChannelId::new(PROD_COINS_CHANNEL_ID)
                    .say(&ctx.http, txt.as_str())
                    .await;
            }
        }
        if COIN_FEEDBACK {
            msg.reply(ctx, format!("{COIN_EMOJI} +{amount} coins! Total: **{total}**"))
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
            // TEST: meteen een chest in #general posten om de graphics te checken.
            if CHEST_SPAWN_ON_START {
                if let Err(e) = do_spawn_chest(
                    ctx.http.clone(),
                    serenity::ChannelId::new(COIN_CHANNEL_ID),
                    data.chest.clone(),
                    data.pool.clone(),
                )
                .await
                {
                    tracing::warn!("kan test-chest niet spawnen: {e}");
                }
            }
        }
        serenity::FullEvent::Message { new_message } => {
            handle_message(ctx, new_message, data).await?;
        }
        serenity::FullEvent::GuildMemberRemoval { guild_id, user, .. } => {
            // Lid verliet de prod-server → saldo archiveren + resetten (verse start bij terugkeer).
            if guild_id.get() == PROD_GUILD_ID {
                let uid = user.id.to_string();
                if let Some(archived) = db::archive_on_leave(&data.pool, &uid, now_secs()) {
                    tracing::info!(
                        "{} verliet de server — {archived} coins gearchiveerd, saldo gereset",
                        user.name
                    );
                }
            }
        }
        serenity::FullEvent::InteractionCreate { interaction } => {
            if let serenity::Interaction::Component(mc) = interaction {
                if mc.data.custom_id == DAILY_CUSTOM_ID {
                    handle_daily(ctx, mc, data).await?;
                } else if mc.data.custom_id == CHEST_CUSTOM_ID {
                    handle_chest_click(ctx, mc, data).await?;
                } else if mc.data.custom_id == SITE_ACCESS_CUSTOM_ID {
                    // Website nog niet open voor de community → under-construction-feedback.
                    respond_ephemeral(
                        ctx,
                        mc,
                        "🚧 The Meadow Market website is still under construction — coming soon! For now you earn & claim right here in Discord.",
                    )
                    .await?;
                } else if mc.data.custom_id == MARKET_INFO_CUSTOM_ID {
                    respond_ephemeral(
                        ctx,
                        mc,
                        &format!(
                            "ℹ️ **How Meadow Market works**\n\
                             • Chat in the community to earn {COIN_EMOJI} Meadowcoins.\n\
                             • Claim your **daily** below for a streak bonus — the longer your streak, the more you earn (every 20h; keep it alive by claiming within 30h).\n\
                             • Spend coins on **Gems**, **Boosts** and **Meadowland Access Passes**.\n\
                             • Climb the **leaderboard**!"
                        ),
                    )
                    .await?;
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

    // Streak: opnieuw geklikt binnen DAILY_STREAK_WINDOW (30u) → +1 dag, anders reset
    // naar dag 1. Eerste claim (last == 0) = dag 1. Gecapt op DAILY_STREAK_CAP.
    let streak = if last <= 0.0 || elapsed >= DAILY_STREAK_WINDOW {
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
    log_earn(&ctx.http, &name, amount, total).await;

    respond_ephemeral(
        ctx,
        mc,
        &format!(
            "🔥 **{name}** checked in for **{streak}** {day_word}! \
             You got **{amount}** Meadowcoins today! Balance: **{total}** {COIN_EMOJI}"
        ),
    )
    .await?;

    // Publiek regeltje in prod #coins.
    if PROD_COINS_CHANNEL_ID != 0 {
        let _ = serenity::ChannelId::new(PROD_COINS_CHANNEL_ID)
            .say(
                &ctx.http,
                format!("{name} checked in for {streak} {day_word} and earned {amount} {COIN_EMOJI}!"),
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

/// Trek een prijs volgens de ACTUELE gewogen verdeling (CHEST_TIERS).
fn chest_prize() -> i64 {
    let mut rng = rand::thread_rng();
    let mut roll = rng.gen_range(0..CHEST_TIER_TOTAL);
    for (w, lo, hi) in CHEST_TIERS {
        if roll < w {
            return rng.gen_range(lo..=hi);
        }
        roll -= w;
    }
    // Vangnet (mocht de som != CHEST_TIER_TOTAL zijn): laagste tier.
    let (_, lo, hi) = CHEST_TIERS[0];
    rng.gen_range(lo..=hi)
}

/// Formatteer een tier-tabel als embed-regels ("**X%** · lo–hi coins").
fn tier_lines(tiers: &[(u32, i64, i64)]) -> String {
    tiers
        .iter()
        .map(|&(w, lo, hi)| {
            let pct = if w % 10 == 0 {
                format!("{}%", w / 10)
            } else {
                format!("{:.1}%", w as f64 / 10.0)
            };
            format!("**{pct}** · {lo}–{hi} coins")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Embed met het overzicht: de huidige (live) verdeling + het fijnkorrelige voorstel.
fn chest_odds_embed() -> serenity::CreateEmbed {
    serenity::CreateEmbed::new()
        .title("🎁 Treasure chest — coin odds")
        .description("What an opened chest can pay out. Odds are per opening; the winner is a random opener.")
        .field("📊 Current (live)", tier_lines(&CHEST_TIERS), false)
        .field(
            "🔬 Proposal — finer-grained",
            tier_lines(&CHEST_TIERS_PROPOSAL),
            false,
        )
        .colour(0xF1_C4_0F)
}

/// Poise-check: laat een commando enkel toe op de dev-guild. Faalt de check (bv. op
/// een prod-guild), dan draait het commando NIET en wordt ook `pre_command` (de
/// bericht-opruiming) overgeslagen — dus volledig inert op prod.
async fn dev_guild_only(ctx: Context<'_>) -> Result<bool, Error> {
    Ok(ctx.guild_id().map(|g| g.get()) == Some(DEV_GUILD_ID))
}

/// `!chest` — spawn meteen een treasure chest in dit kanaal (om te testen). Enkel
/// dev-guild. Het commando-bericht wordt door de `pre_command`-hook opgeruimd.
#[poise::command(prefix_command, check = "dev_guild_only")]
pub async fn chest(ctx: Context<'_>) -> Result<(), Error> {
    let data = ctx.data();
    do_spawn_chest(
        ctx.serenity_context().http.clone(),
        ctx.channel_id(),
        data.chest.clone(),
        data.pool.clone(),
    )
    .await?;
    Ok(())
}

/// `!chestodds` — toon de prijsverdeling (huidig + fijnkorrelig voorstel). Enkel dev-guild.
#[poise::command(prefix_command, check = "dev_guild_only")]
pub async fn chestodds(ctx: Context<'_>) -> Result<(), Error> {
    ctx.send(poise::CreateReply::default().embed(chest_odds_embed()))
        .await?;
    Ok(())
}

/// Bouw de chest-embed voor het huidige aantal deelnemers. Onder de drempel
/// (CHEST_MIN_JOINERS) toont hij "It will despawn <t:R>." + "Needs N more
/// participant(s)."; zodra er genoeg deelnemers zijn verdwijnt die regel en
/// wordt het "It will open <t:R>.". Herbruikt bij spawn én bij elke klik.
fn chest_embed(pop_ts: i64, joiners: usize) -> serenity::CreateEmbed {
    let enough = joiners >= CHEST_MIN_JOINERS;
    let verb = if enough { "open" } else { "despawn" };
    // ### = iets groter (Markdown-header) op één regel (geen grote tussenruimte tussen
    // twee headers). Discord klapt meerdere spaties in tot één, dus een geforceerde
    // wrap na "grand prize!" kan niet — de regel wrapt vanzelf op de vensterbreedte.
    let mut desc = format!(
        "### See if you win the **grand prize**! It will **{verb}** <t:{pop_ts}:R>."
    );
    if !enough {
        let need = CHEST_MIN_JOINERS - joiners;
        let p = if need == 1 { "participant" } else { "participants" };
        desc.push_str(&format!("\nNeeds **{need}** more {p}."));
    }
    serenity::CreateEmbed::new()
        .title("🎁 A treasure chest appeared!")
        .description(desc)
        .image("attachment://chest.png") // grote chest onderaan
        .thumbnail("attachment://coin.png") // coin rechtsboven
        .colour(0xF1_C4_0F)
}

/// Post een chest-bericht (knop + afbeeldingen), registreer het in de tracker en
/// plan de pop. Herbruikt door de gewone spawn én de test-spawn bij startup.
async fn do_spawn_chest(
    http: Arc<serenity::Http>,
    channel_id: serenity::ChannelId,
    tracker: Arc<Mutex<ChestTracker>>,
    pool: DbPool,
) -> Result<(), Error> {
    let button = serenity::CreateButton::new(CHEST_CUSTOM_ID)
        .emoji('🎁')
        .label("Try your luck")
        .style(serenity::ButtonStyle::Success);
    // Pop-tijd als unix-timestamp: Discord's relatieve <t:…:R> telt er live naar af.
    let pop_ts = (now_secs() + CHEST_POP_DELAY as f64) as i64;
    let builder = serenity::CreateMessage::new()
        .embed(chest_embed(pop_ts, 0))
        .add_file(serenity::CreateAttachment::bytes(CHEST_IMG, "chest.png"))
        .add_file(serenity::CreateAttachment::bytes(CHEST_COIN_IMG, "coin.png"))
        .components(vec![serenity::CreateActionRow::Buttons(vec![button])]);
    let sent = channel_id.send_message(&http, builder).await?;
    let msg_id = sent.id.get();

    tracker.lock().unwrap().chests.insert(
        msg_id,
        Chest {
            channel_id: channel_id.get(),
            joiners: Vec::new(),
            pop_ts,
        },
    );
    tracing::info!(
        "treasure chest gespawned in kanaal {} (bericht {msg_id})",
        channel_id.get()
    );

    let http2 = http.clone();
    let pool2 = pool.clone();
    let tracker2 = tracker.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(CHEST_POP_DELAY)).await;
        pop_chest(http2, pool2, tracker2, channel_id, msg_id).await;
    });
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

    // `active` is al gereserveerd onder de lock → spawnen; bij fout weer vrijgeven.
    if let Err(e) = do_spawn_chest(
        ctx.http.clone(),
        msg.channel_id,
        data.chest.clone(),
        data.pool.clone(),
    )
    .await
    {
        data.chest.lock().unwrap().active.remove(&chan);
        return Err(e);
    }
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
    // edit = Some((pop_ts, n)) enkel bij een échte nieuwe deelnemer → embed bijwerken.
    let (joined, edit) = {
        let mut t = data.chest.lock().unwrap();
        match t.chests.get_mut(&msg_id) {
            None => (None, None),
            Some(c) => {
                if c.joiners.iter().any(|(u, _)| u == &uid) {
                    (Some(0), None)
                } else {
                    c.joiners.push((uid.clone(), name));
                    let n = c.joiners.len();
                    (Some(n), Some((c.pop_ts, n)))
                }
            }
        }
    };
    // Nieuwe deelnemer → werk de chest-embed bij (need-teller daalt; bij genoeg
    // deelnemers verdwijnt die regel en wordt "despawn" → "open").
    if let Some((pop_ts, n)) = edit {
        // Afbeeldingen opnieuw meesturen (keep_all bleek ze soms te droppen bij een
        // edit) zodat de chest-graphic altijd zichtbaar blijft na een klik.
        let atts = serenity::EditAttachments::new()
            .add(serenity::CreateAttachment::bytes(CHEST_IMG, "chest.png"))
            .add(serenity::CreateAttachment::bytes(CHEST_COIN_IMG, "coin.png"));
        let builder = serenity::EditMessage::new()
            .embeds(vec![chest_embed(pop_ts, n)])
            .attachments(atts);
        if let Err(e) = mc.channel_id.edit_message(&ctx.http, mc.message.id, builder).await {
            tracing::warn!("kan chest-embed niet bijwerken: {e}");
        }
    }
    let text = match joined {
        None => "📦 Too late — this chest already opened.".to_string(),
        Some(0) => "🎁 You're already in! Sit tight for the opening.".to_string(),
        Some(_) => "🎁 You're successfully trying to open the chest!".to_string(),
    };
    respond_ephemeral(ctx, mc, &text).await?;
    Ok(())
}

/// Pop de chest (na CHEST_POP_DELAY): kies een random klikker, ken de prijs toe,
/// verwijder het originele chest-bericht en post het resultaat als NIEUW embed
/// onderaan het kanaal. Geeft het kanaal vrij en zet de anti-spam-cooldown.
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

    // Origineel (met chest-afbeelding) altijd verwijderen — anders raken de
    // attachments los van de embed en toont Discord de coin op volledige grootte.
    if let Err(e) = channel_id
        .delete_message(http.as_ref(), serenity::MessageId::new(msg_id))
        .await
    {
        tracing::warn!("kan chest-bericht {msg_id} niet verwijderen: {e}");
    }

    // Te weinig deelnemers → despawn: niks weggeven, wel een "Fortuna cries"-embed.
    if joiners.len() < CHEST_MIN_JOINERS {
        tracing::info!(
            "chest despawned in kanaal {channel_id} (te weinig deelnemers: {})",
            joiners.len()
        );
        let who = if joiners.is_empty() { "**No one**" } else { "**Only one**" };
        let embed = serenity::CreateEmbed::new()
            .title("Fortuna cries...")
            .description(format!("{who} tried to open my chest"))
            .image("attachment://crying.png")
            .colour(0x95_A5_A6);
        let builder = serenity::CreateMessage::new()
            .embed(embed)
            .add_file(serenity::CreateAttachment::bytes(CRYING_IMG, "crying.png"));
        if let Err(e) = channel_id.send_message(http.as_ref(), builder).await {
            tracing::warn!("kan despawn-embed niet posten in {channel_id}: {e}");
        }
        return;
    }

    // Genoeg deelnemers → open: random winnaar, prijs, log, resultaat onderaan.
    let idx = rand::thread_rng().gen_range(0..joiners.len());
    let (winner_uid, winner_name) = &joiners[idx];
    let prize = chest_prize();
    let total = db::award(&pool, winner_uid, winner_name, prize, now_secs());
    log_earn(http.as_ref(), winner_name, prize, total).await;
    let opener_word = if joiners.len() == 1 { "opener" } else { "openers" };
    tracing::info!(
        "chest geopend: {winner_name} wint {prize} coin(s) uit {} deelnemer(s)",
        joiners.len()
    );
    let embed = serenity::CreateEmbed::new()
        .title("The Magic Chest opened!")
        .description(format!(
            "Out of **{}** {opener_word}, <@{winner_uid}> got lucky!\n\
             They won **{prize}** {COIN_EMOJI} !!!",
            joiners.len()
        ))
        .colour(0x6B_9B_52);
    if let Err(e) = channel_id
        .send_message(http.as_ref(), serenity::CreateMessage::new().embed(embed))
        .await
    {
        tracing::warn!("kan chest-resultaat niet posten in {channel_id}: {e}");
    }
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

/// Post om HH:01 een shout-out in #coins voor iedereen die in het net afgelopen
/// klok-uur ≥ HOURLY_SHOUTOUT_MIN coins verdiende. Geen bericht als niemand de
/// drempel haalde. State zit in de DB (earn_log) → overleeft een herstart.
async fn hourly_shoutouts(http: Arc<serenity::Http>, pool: DbPool) {
    loop {
        let (since, until, min) = if HOURLY_SHOUTOUT_TEST {
            // TEST: elke HOURLY_TEST_INTERVAL sec; venster = die laatste interval.
            tokio::time::sleep(std::time::Duration::from_secs_f64(HOURLY_TEST_INTERVAL)).await;
            let until = now_secs();
            (until - HOURLY_TEST_INTERVAL, until, HOURLY_TEST_MIN)
        } else {
            // PROD: slaap tot de eerstvolgende HH:01; venster = net afgelopen klok-uur.
            let now = now_secs();
            let boundary = (now / 3600.0).floor() * 3600.0; // huidig hele uur (:00)
            let mut fire = boundary + 60.0; // :01
            if fire <= now {
                fire += 3600.0;
            }
            tokio::time::sleep(std::time::Duration::from_secs_f64(fire - now)).await;
            let hour_end = (now_secs() / 3600.0).floor() * 3600.0;
            (hour_end - 3600.0, hour_end, HOURLY_SHOUTOUT_MIN)
        };

        let earners = db::hourly_earners(&pool, since, until, min);
        for (uid, _name, total) in &earners {
            let _ = serenity::ChannelId::new(PROD_COINS_CHANNEL_ID)
                .say(
                    &http,
                    format!("<@{uid}>, wow you've earned **{total}** {COIN_EMOJI} over the last hour! Well done!"),
                )
                .await;
        }
        if !earners.is_empty() {
            tracing::info!("shout-out: {} lid/leden ≥{min} coins", earners.len());
        }
        // Bewaar ~8 dagen earn_log (het weekly leaderboard leest ervan).
        db::prune_earn_log(&pool, now_secs() - 8.0 * 86400.0);
    }
}

/// Post elke zaterdag 15:00 (Brusselse tijd) het weekly leaderboard als embed in
/// het prod #general. Geen bericht als niemand deze week iets verdiende.
async fn weekly_leaderboard(http: Arc<serenity::Http>, pool: DbPool) {
    loop {
        let now = now_secs();
        let fire = db::next_saturday_1500_brussels(now);
        tokio::time::sleep(std::time::Duration::from_secs_f64((fire - now).max(1.0))).await;

        // Venster = de net afgelopen week: sinds de vorige zaterdag 15:00.
        let since = db::last_saturday_1500_brussels(now_secs()) - 7.0 * 86400.0;
        let top = db::leaderboard_week(&pool, since, 10);
        if top.is_empty() {
            continue;
        }
        let medal = |i: usize| match i {
            0 => "👑",
            1 => "🥈",
            2 => "🥉",
            _ => "🌼",
        };
        let lines: String = top
            .iter()
            .enumerate()
            .map(|(i, (uid, _n, total))| {
                format!("{} <@{uid}> — **{total}** {COIN_EMOJI}\n", medal(i))
            })
            .collect();
        let embed = serenity::CreateEmbed::new()
            .title("🏆 Weekly leaderboard")
            .description(format!("Top earners of the past week!\n\n{lines}"))
            .colour(0x6B_9B_52);
        if PROD_GENERAL_CHANNEL_ID != 0 {
            let _ = serenity::ChannelId::new(PROD_GENERAL_CHANNEL_ID)
                .send_message(http.as_ref(), serenity::CreateMessage::new().embed(embed))
                .await;
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

    // Enkel !chest (dev-only info-commando). Het !coins-leaderboard is verwijderd.
    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![chest(), chestodds()],
            prefix_options: poise::PrefixFrameworkOptions {
                prefix: Some(PREFIX.to_string()),
                ..Default::default()
            },
            // Steeds: wis het commando-bericht vóór uitvoering (properder kanaal).
            // Draait ná de checks (zie run_invocation), dus niet op een guild waar de
            // check faalt → een dev-only commando raakt prod ook hiermee niet aan.
            pre_command: |ctx| {
                Box::pin(async move {
                    if let poise::Context::Prefix(pctx) = ctx {
                        if let Err(e) = pctx.msg.delete(ctx.serenity_context()).await {
                            tracing::warn!("kan commando-bericht niet verwijderen: {e}");
                        }
                    }
                })
            },
            event_handler: |ctx, event, framework, data| {
                Box::pin(event_handler(ctx, event, framework, data))
            },
            ..Default::default()
        })
        .setup(move |ctx, _ready, _framework| {
            let hourly_pool = pool.clone();
            let hourly_http = ctx.http.clone();
            let weekly_pool = pool.clone();
            let weekly_http = ctx.http.clone();
            Box::pin(async move {
                // Uurlijkse shout-out voor wie ≥100 coins verdiende in het afgelopen uur.
                tokio::spawn(hourly_shoutouts(hourly_http, hourly_pool));
                // Weekly leaderboard elke zaterdag 15:00 (Brussel) in prod #general.
                tokio::spawn(weekly_leaderboard(weekly_http, weekly_pool));
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
