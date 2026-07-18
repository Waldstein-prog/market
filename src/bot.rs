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
use crate::settings;

// --- dev-instellingen (later aanpassen) ---------------------------------
// NB: de economie-parameters (cooldown, coin-gewichten, daily, chest) staan hier
// NIET meer als const — die zijn admin-instelbaar geworden via Manage → ⚙ Settings
// en worden LIVE uit de DB gelezen. Zie `settings.rs` voor de sleutels + defaults.
const DEV_FEEDBACK: bool = false; // cooldown-terugkoppeling per bericht (dev-only; laat uit → geen ⏳-spam in #general)
const COIN_FEEDBACK: bool = false; // toon de speler in #general zijn coin-award ("+N coins! Total: X")
const COIN_CHANNEL_ID: u64 = 1229046340793663488; // #general: enkel hier coins per bericht (0 = overal). De chest-detectie volgt ditzelfde kanaal.
const FORTUNA_LOG_CHANNEL_ID: u64 = 1526181603624226938; // Magic Meadow #fortuna-log: elke coin-verdienste (0 = uit)
const MEADOWMARKET_LOG_CHANNEL_ID: u64 = 0; // saldo-log uit op prod (fortuna-log dekt de verdiensten)
const PROD_COINS_CHANNEL_ID: u64 = 1403044480218824794; // Magic Meadow 🪙meadowcoins (shout-out + level-up + weekly)
const PROD_GENERAL_CHANNEL_ID: u64 = 1296469405651435594; // Magic Meadow ☀️general (weekly zaterdag 15u)
const PROD_GUILD_ID: u64 = 1296469405651435592; // Magic Meadow — leave/rejoin-archief triggert enkel hier
// Weekly leaderboard tijdelijk uitgeschakeld tot de cadeauknoppen-feature af is (2026-07-18).
const WEEKLY_LEADERBOARD_ENABLED: bool = false;
const HOURLY_SHOUTOUT_MIN: i64 = 1; // drempel: minstens 1 coin verdiend in het afgelopen uur
const HOURLY_SHOUTOUT_TOP: i64 = 10; // hoeveel leden in het uurlijkse top-embed
// TEST-modus: vuur elke HOURLY_TEST_INTERVAL sec met venster = die interval,
// i.p.v. op het uur. Zet op false voor prod (dan HH:01 + venster = het klok-uur).
const HOURLY_SHOUTOUT_TEST: bool = false;
const HOURLY_TEST_INTERVAL: f64 = 2.0 * 60.0; // test: interval én venster (s)
const HOURLY_TEST_MIN: i64 = 1; // test-drempel
// De custom Meadowcoins-emoji (guild-emoji). Bots moeten <:naam:id> sturen, niet :naam:.
const COIN_EMOJI: &str = "<:Meadowcoins:1526188363110023308>"; // Magic Meadow-emoji; bot zit in prod → rendert op beide guilds

// Level-up-embed: het stukje ná de komma varieert willekeurig. Exact de teksten van de
// user — NIET zelf uitbreiden (elke speler-zichtbare tekst is een beslissing van de user).
const LEVELUP_VARIANTS: &[&str] = &[
    "super inspiring!",
    "terrifically done!",
    "be proud of you!",
    "you did amazing!",
    "lots of praise to you!",
];
const PREFIX: &str = "!"; // deze berichten leveren geen coins op (oude commando-syntax)
// --- daily-beloning (embed-knop) ----------------------------------------
// Streak-daily: dag 1 = random in [daily_base_min_coins, daily_base_max_coins].
// Elke opeenvolgende dag verhoogt de ondergrens met daily_min_step_coins en de
// bovengrens met daily_max_step_coins. Een dag overslaan reset naar dag 1. Na
// dag daily_streak_cap_days stopt de verhoging. Alle vijf instelbaar in Settings.
const DAILY_CUSTOM_ID: &str = "daily_claim"; // moet matchen met de embed-knop
const SITE_ACCESS_CUSTOM_ID: &str = "site_access"; // "site"-knop → under-construction (website nog niet open)
// --- treasure chest -----------------------------------------------------
// Chatten ≥ chest_distinct_users verschillende mensen binnen chest_window_min in
// hetzelfde (test)kanaal → er verschijnt een chest met een knop. Klikken = meedoen;
// chest_pop_delay_min later popt hij en wint één random klikker de getrokken prijs.
// Die vijf staan in Settings; wat hier overblijft is niet-economisch.
const CHEST_SPAWN_CHANNEL_ID: u64 = 1296469405651435594; // natuurlijke chests spawnen ENKEL hier (Magic Meadow #general)
const CHEST_TICK_SECS: u64 = 2; // interval waarmee de M:SS-timer in de embed wordt bijgewerkt (vloeiender)
const CHEST_SPAWN_ON_START: bool = false; // (was test) — nu vervangen door het !chest dev-commando
const CHEST_CUSTOM_ID: &str = "chest_open"; // knop custom_id
// Artwork ingebakken in de binary (geen losse bestanden bij deploy nodig). Gehangen
// als attachments aan het chest-bericht en via attachment:// in de embed getoond:
// chest = grote image (onderaan), coin = thumbnail (rechtsboven).
const CRYING_IMG: &[u8] = include_bytes!("../artwork/crying.png"); // getoond als de chest despawnt
// De chest-prijsverdeling staat in de tabel `chest_tiers` (Manage → ⚙ Settings):
// per tier een RELATIEF gewicht + een coin-bereik. De som hoeft nergens op uit te
// komen — `chest_prize` deelt door het totaal. De 10-tier-verdeling die hier als
// const stond (live sinds 2026-07-14) is nu de seed van die tabel.
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
    // per kanaal: recente (uid, naam, ts) om "N verschillende chatters binnen het venster"
    // te tellen én om bij een spawn te kunnen loggen wie de chest uitlokte.
    recent: HashMap<u64, Vec<(String, String, f64)>>,
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

/// Trek een coin-award voor één bericht uit de tabel `coin_weights`: per rij een
/// bedrag en een RELATIEF gewicht. We rollen in [0, som) en lopen de rijen af —
/// zo hoeven de gewichten nergens op uit te komen en is "half zoveel kans"
/// letterlijk 0.5. Een bedrag van 0 is een geldige uitkomst (= niets gewonnen).
/// Hoelang een chest openstaat, in seconden. De instelling staat in minuten
/// (`chest_pop_delay_min`); dit is de enige plek die dat omrekent.
fn pop_delay_secs(pool: &DbPool) -> u64 {
    (settings::i64_of(pool, "chest_pop_delay_min") * 60).max(1) as u64
}

fn coin_amount(pool: &DbPool) -> i64 {
    let weights = db::coin_weights_all(pool);
    let total: f64 = weights.iter().map(|(_, w)| w.max(0.0)).sum();
    // Vangnet: lege tabel of enkel nul-gewichten → 1 coin, zoals vóór de refactor.
    if total <= 0.0 {
        return 1;
    }
    let mut roll = rand::thread_rng().gen_range(0.0..total);
    for (amount, w) in &weights {
        roll -= w.max(0.0);
        if roll < 0.0 {
            return *amount;
        }
    }
    // Onbereikbaar op afrondingsfouten na; dan de laatste rij.
    weights.last().map(|(a, _)| *a).unwrap_or(1)
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

    let cooldown = settings::f64_of(&data.pool, "msg_cooldown_sec");
    if elapsed >= cooldown {
        let amount = coin_amount(&data.pool);
        let total = db::award(&data.pool, &uid, &name, amount, now);
        tracing::info!("{name}: +{amount} coins (totaal {total})");
        // Een award van 0 is een geldige uitkomst (rij `0` in coin_weights) en wordt
        // gelogd als elke andere: stilte las als een bug, niet als pech.
        log_earn(&ctx.http, &name, amount, total).await;
        // Level-up? → embed met claim-knop in #coins (gecentraliseerd, zie maybe_levelup).
        maybe_levelup(&ctx.http, &data.pool, &uid, &name).await;
        if COIN_FEEDBACK {
            msg.reply(ctx, format!("{COIN_EMOJI} +{amount} coins! Total: **{total}**"))
                .await?;
        }
    } else {
        let remaining = (cooldown - elapsed) as i64 + 1;
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
                    pop_delay_secs(&data.pool),
                    &[],
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
                } else if mc.data.custom_id.starts_with("lg:") {
                    handle_level_claim(ctx, mc, data).await?;
                } else if mc.data.custom_id.starts_with("wg:") {
                    handle_weekly_claim(ctx, mc, data).await?;
                } else if mc.data.custom_id == SITE_ACCESS_CUSTOM_ID {
                    // De site-gate stuurt niet-admins naar /info; admins mogen de market in.
                    // Admins krijgen een login-link die na inloggen meteen op /market landt
                    // (de gate laat een niet-ingelogde admin anders óók naar /info lopen).
                    let base = data.cfg.base_url.trim_end_matches('/');
                    let uid = mc.user.id.to_string();
                    let msg = if crate::web::is_admin(&uid) {
                        format!("🛒 Admin access — open the Meadow Market here:\n{base}/login?next=/market")
                    } else {
                        format!("🌼 Peek at the Meadow Market here:\n{base}/info")
                    };
                    respond_ephemeral(ctx, mc, &msg).await?;
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
    let daily_cooldown = settings::f64_of(&data.pool, "daily_cooldown_hours") * 3600.0;
    if elapsed < daily_cooldown {
        let left = daily_cooldown - elapsed;
        let hrs = (left / 3600.0).floor() as i64;
        let mins = ((left % 3600.0) / 60.0).floor() as i64;
        respond_ephemeral(
            ctx,
            mc,
            &format!("⏳ Too soon! Come back in **{hrs}h {mins}m**."),
        )
        .await?;
        return Ok(());
    }

    // Streak: opnieuw geklikt binnen het streak-venster → +1 dag, anders reset naar
    // dag 1. Eerste claim (last == 0) = dag 1. Gecapt op het streak-plafond.
    let streak_window = settings::f64_of(&data.pool, "daily_streak_window_hours") * 3600.0;
    let streak = if last <= 0.0 || elapsed >= streak_window {
        1
    } else {
        (db::get_daily_streak(&data.pool, &uid) + 1)
            .min(settings::i64_of(&data.pool, "daily_streak_cap_days"))
    };
    // Dag N: ondergrens/bovengrens schuiven mee met de streak.
    let step = streak - 1;
    let lo = settings::i64_of(&data.pool, "daily_base_min_coins")
        + step * settings::i64_of(&data.pool, "daily_min_step_coins");
    let hi = settings::i64_of(&data.pool, "daily_base_max_coins")
        + step * settings::i64_of(&data.pool, "daily_max_step_coins");
    // De twee grenzen zijn los instelbaar, dus een admin kán ze omgekeerd zetten;
    // `gen_range` zou daarop paniekeren. Ondergrens wint — nooit een leeg bereik.
    let hi = hi.max(lo);
    let amount = rand::thread_rng().gen_range(lo..=hi);
    let total = db::award_daily(&data.pool, &uid, &name, amount, streak, now);
    let day_word = if streak == 1 { "day" } else { "days" };
    tracing::info!("daily: {name} +{amount} (streak {streak}, totaal {total})");
    // Daily kan je over een levelgrens tillen → level-up-check.
    maybe_levelup(&ctx.http, &data.pool, &uid, &name).await;
    // Logboek: dagelijkse check-in (bedrag + streak) — zodat coin-instroom te volgen is.
    db::log_event(
        &data.pool,
        now,
        &db::LogEntry::new("daily", "checkin")
            .actor(&uid, &name)
            .amount(amount)
            .detail(format!("streak {streak} · balance {total}")),
    );
    // Interactie stil bevestigen — GEEN ephemeral bij een geslaagde claim (de feedback
    // komt publiek in #coins). Vroeg acken zodat we ruim binnen de 3s-limiet blijven.
    let _ = mc
        .create_response(&ctx.http, serenity::CreateInteractionResponse::Acknowledge)
        .await;
    // DEBUG-regel voor admins in #fortuna-log: bedrag + streak + de rnd-grenzen,
    // zodat het rekenwerk (dag N → [lo,hi] → gekozen bedrag) te volgen is.
    if FORTUNA_LOG_CHANNEL_ID != 0 {
        let _ = serenity::ChannelId::new(FORTUNA_LOG_CHANNEL_ID)
            .say(
                &ctx.http,
                format!(
                    "🔧 daily — <@{uid}> got **{amount}** {COIN_EMOJI} · streak **{streak}** · rolled in [**{lo}**–**{hi}**] · balance **{total}**"
                ),
            )
            .await;
    }

    // Publiek regeltje in prod #coins.
    if PROD_COINS_CHANNEL_ID != 0 {
        let _ = serenity::ChannelId::new(PROD_COINS_CHANNEL_ID)
            .say(
                &ctx.http,
                format!("<@{uid}> checked in for **{streak}** {day_word} and earned **{amount}** {COIN_EMOJI}!"),
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

// --- level-up-cadeaus ----------------------------------------------------

/// Bouw de "LEVEL UP!"-embed. Het stukje na de komma varieert willekeurig uit
/// `LEVELUP_VARIANTS`. `[tag]` = een mention (pingt), het levelgetal staat vet.
fn levelup_embed(uid: &str, level: i64) -> serenity::CreateEmbed {
    let variant = LEVELUP_VARIANTS[rand::thread_rng().gen_range(0..LEVELUP_VARIANTS.len())];
    serenity::CreateEmbed::new()
        .title("🎉 LEVEL UP! 🎉")
        .description(format!("<@{uid}>, you are now level **{level}**, {variant}"))
        .colour(0xF1_C4_0F)
}

/// De 🎁-claim-knop (één knop-rij) met een gegeven custom_id.
fn claim_button_row(custom_id: String) -> serenity::CreateActionRow {
    serenity::CreateActionRow::Buttons(vec![serenity::CreateButton::new(custom_id)
        .emoji('🎁')
        .label("Claim reward")
        .style(serenity::ButtonStyle::Success)])
}

/// Level-up-check: post een embed + claim-knop voor élk nieuw level boven de marker.
/// Zelfhelend — een level-up die via daily/chest/admin/gift liep, wordt hier alsnog opgepikt
/// zodra het lid weer coins verdient. Cadeau = 1,5% van het huidige saldo, half naar boven.
/// De claim boekt de coins als échte verdienste (`credit_earned` → `total_earned` + `earn_log`):
/// álle coins tellen mee voor de level-up, ongeacht bron. Geen op-hol-slaan: 1,5% < een levelgat.
async fn maybe_levelup(http: &Arc<serenity::Http>, pool: &DbPool, uid: &str, name: &str) {
    let (coins, _max, _pub, earned) = db::get_stats(pool, uid);
    let cur = db::level_of(earned);
    let gifted = db::get_gifted_level(pool, uid);
    if cur <= gifted {
        return;
    }
    let now = now_secs();
    for level in (gifted + 1)..=cur {
        let amount = ((coins as f64) * 0.015).round() as i64;
        let gid = db::create_level_gift(pool, uid, amount, level, "levelup", now);
        db::log_event(
            pool,
            now,
            &db::LogEntry::new("level", "levelup")
                .actor(uid, name)
                .amount(amount)
                .detail(format!("reached level {level}")),
        );
        if PROD_COINS_CHANNEL_ID != 0 {
            let builder = serenity::CreateMessage::new()
                .embed(levelup_embed(uid, level))
                .components(vec![claim_button_row(format!("lg:{gid}"))]);
            let _ = serenity::ChannelId::new(PROD_COINS_CHANNEL_ID)
                .send_message(http, builder)
                .await;
        }
    }
    db::set_gifted_level(pool, uid, cur);
}

/// Klik op een 🎁-claim-knop: keert het cadeau eenmalig uit aan de eigenaar en post een
/// publiek regeltje. Dubbelklik of een andere klikker → ephemeral melding, geen uitkering.
async fn handle_level_claim(
    ctx: &serenity::Context,
    mc: &serenity::ComponentInteraction,
    data: &Data,
) -> Result<(), Error> {
    let gid: i64 = mc
        .data
        .custom_id
        .strip_prefix("lg:")
        .and_then(|s| s.parse().ok())
        .unwrap_or(-1);
    let uid = mc.user.id.to_string();
    let name = mc
        .user
        .global_name
        .clone()
        .unwrap_or_else(|| mc.user.name.clone());
    match db::claim_level_gift(&data.pool, gid, &uid, &name, now_secs()) {
        db::GiftClaim::Granted(amount) => {
            // Stil acken + de knop uitschakelen (voorkomt herklikken, toont "Claimed").
            let _ = mc
                .create_response(&ctx.http, serenity::CreateInteractionResponse::Acknowledge)
                .await;
            let done = serenity::CreateButton::new(format!("lg:{gid}"))
                .emoji('🎁')
                .label("Claimed")
                .style(serenity::ButtonStyle::Secondary)
                .disabled(true);
            let _ = mc
                .channel_id
                .edit_message(
                    &ctx.http,
                    mc.message.id,
                    serenity::EditMessage::new()
                        .components(vec![serenity::CreateActionRow::Buttons(vec![done])]),
                )
                .await;
            // Publiek regeltje — de NAAM (geen ping), bewust anders dan de tag in de embed.
            let _ = mc
                .channel_id
                .say(
                    &ctx.http,
                    format!(
                        "**{}** got **{amount}** {COIN_EMOJI} for the level up.",
                        escape_md(&name)
                    ),
                )
                .await;
            db::log_event(
                &data.pool,
                now_secs(),
                &db::LogEntry::new("level", "claim")
                    .actor(&uid, &name)
                    .amount(amount)
                    .detail("claimed level-up reward".to_string()),
            );
            // De gift telt nu mee voor total_earned → in het zeldzame randgeval dat ze een
            // volgend level ontgrendelt, pikt dit dat meteen op (zelfhelend, bounded).
            maybe_levelup(&ctx.http, &data.pool, &uid, &name).await;
        }
        db::GiftClaim::AlreadyClaimed => {
            respond_ephemeral(ctx, mc, "You already claimed this reward. 🎁").await?
        }
        db::GiftClaim::NotYours => {
            respond_ephemeral(ctx, mc, "Uh-oh! This is not your reward!").await?
        }
        db::GiftClaim::NotFound => {
            respond_ephemeral(ctx, mc, "This reward is no longer available.").await?
        }
    }
    Ok(())
}

/// Trek een prijs volgens de ACTUELE verdeling in `chest_tiers`: eerst een tier
/// op relatief gewicht, dan een bedrag binnen dat tier-bereik.
fn chest_prize(pool: &DbPool) -> i64 {
    let tiers = db::chest_tiers_all(pool);
    let mut rng = rand::thread_rng();
    let total: f64 = tiers.iter().map(|(_, w, _, _)| w.max(0.0)).sum();
    // Vangnet: geen tiers (of enkel nul-gewichten) → de laagste historische tier.
    if total <= 0.0 {
        return rng.gen_range(50..=80);
    }
    let mut roll = rng.gen_range(0.0..total);
    for (_, w, lo, hi) in &tiers {
        roll -= w.max(0.0);
        if roll < 0.0 {
            return rng.gen_range(*lo.min(hi)..=*lo.max(hi));
        }
    }
    let (_, _, lo, hi) = tiers[0];
    rng.gen_range(lo.min(hi)..=lo.max(hi))
}

/// Formatteer de tier-tabel als embed-regels ("**X%** · lo–hi coins"). De
/// percentages worden uit de relatieve gewichten gerekend, dus de embed klopt
/// ook als een admin de gewichten niet op 100 (of 1000) laat uitkomen.
fn tier_lines(tiers: &[(i64, f64, i64, i64)]) -> String {
    let total: f64 = tiers.iter().map(|(_, w, _, _)| w.max(0.0)).sum();
    if total <= 0.0 {
        return "_geen verdeling ingesteld_".into();
    }
    tiers
        .iter()
        .map(|&(_, w, lo, hi)| {
            let pct = w.max(0.0) / total * 100.0;
            let pct = if (pct - pct.round()).abs() < 0.05 {
                format!("{}%", pct.round() as i64)
            } else {
                format!("{pct:.1}%")
            };
            format!("**{pct}** · {lo}–{hi} coins")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Embed met het overzicht van de live prijsverdeling.
fn chest_odds_embed(pool: &DbPool) -> serenity::CreateEmbed {
    serenity::CreateEmbed::new()
        .title("🎁 Treasure chest — coin odds")
        .description("What an opened chest can pay out. Odds are per opening; the winner is a random opener.")
        .field("📊 Odds", tier_lines(&db::chest_tiers_all(pool)), false)
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
        pop_delay_secs(&data.pool), // prod-timing
        &[],
    )
    .await?;
    Ok(())
}

/// `!chestodds` — toon de live prijsverdeling. Enkel dev-guild.
#[poise::command(prefix_command, check = "dev_guild_only")]
pub async fn chestodds(ctx: Context<'_>) -> Result<(), Error> {
    ctx.send(poise::CreateReply::default().embed(chest_odds_embed(&ctx.data().pool)))
        .await?;
    Ok(())
}

/// Poise-check: enkel de site-admins (web::is_admin). Werkt overal, ook op de
/// prod-guild — nodig voor beheer-ingrepen zoals een verweesde chest heropenen.
async fn admin_only(ctx: Context<'_>) -> Result<bool, Error> {
    Ok(crate::web::is_admin(&ctx.author().id.to_string()))
}

/// `!chestrescue <message_id>` — heropen alsnog een verweesde treasure chest
/// (bv. verloren toen de bot herstartte terwijl de chest nog open stond). Haalt
/// de deelnemers uit het logboek, trekt een winnaar (gewogen op Lucky Horseshoe),
/// betaalt uit en post het resultaat in #general. Admin-only.
#[poise::command(prefix_command, check = "admin_only")]
pub async fn chestrescue(ctx: Context<'_>, msg_id: Option<u64>) -> Result<(), Error> {
    let pool = &ctx.data().pool;
    let http = ctx.serenity_context().http.clone();
    let channel = serenity::ChannelId::new(CHEST_SPAWN_CHANNEL_ID);

    // Zonder message-id: de laatste verweesde chest automatisch opzoeken.
    let msg_id = match msg_id.or_else(|| db::last_unresolved_chest(pool)) {
        Some(id) => id,
        None => {
            ctx.say("⚠️ No orphaned (unresolved) chest found in the log.").await?;
            return Ok(());
        }
    };

    let joiners = db::chest_joiners_from_log(pool, msg_id);
    let joiner_names = joiners.iter().map(|(_, n)| n.as_str()).collect::<Vec<_>>().join(", ");
    if joiners.len() < settings::usize_of(pool, "chest_min_joiners") {
        ctx.say(format!(
            "⚠️ Chest `{msg_id}` only has {} participant(s) in the log — nothing to pay out.",
            joiners.len()
        ))
        .await?;
        return Ok(());
    }

    // Gewogen winnaar (zelfde logica als pop_chest): wie de Lucky Horseshoe bezit = 2 loten.
    // De horseshoe is permanent — niets te verbruiken na afloop.
    let weights: Vec<u32> = joiners.iter().map(|(uid, _)| db::chest_weight(pool, uid)).collect();
    let total_weight: u32 = weights.iter().sum();
    let mut roll = rand::thread_rng().gen_range(0..total_weight);
    let mut idx = 0;
    for (i, w) in weights.iter().enumerate() {
        if roll < *w {
            idx = i;
            break;
        }
        roll -= *w;
    }
    let winner_had_luck = weights[idx] > 1;
    let (winner_uid, winner_name) = &joiners[idx];
    let prize = chest_prize(pool);
    let total = db::award(pool, winner_uid, winner_name, prize, now_secs());
    log_earn(http.as_ref(), winner_name, prize, total).await;
    maybe_levelup(&http, pool, winner_uid, winner_name).await;
    tracing::info!(
        "chest RESCUE geopend ({msg_id}): {winner_name} wint {prize} coin(s) uit {} deelnemer(s)",
        joiners.len()
    );
    db::log_event(
        pool,
        now_secs(),
        &db::LogEntry::new("chest", "win")
            .actor(winner_uid, winner_name)
            .channel(channel.get())
            .reference(msg_id)
            .amount(prize)
            .detail(format!("RESCUE — won from {} participant(s): {joiner_names}", joiners.len())),
    );
    // Cooldown alsnog zetten (geheugen + schijf), net als een echte opening.
    let until = now_secs() + settings::f64_of(pool, "chest_channel_cooldown_min") * 60.0;
    ctx.data().chest.lock().unwrap().cooldown_until.insert(channel.get(), until);
    db::set_chest_cooldown(pool, channel.get(), until);
    // Uit de tracker + persistente lijst (mocht hij er nog in zitten).
    ctx.data().chest.lock().unwrap().chests.remove(&msg_id);
    db::delete_live_chest(pool, msg_id);

    let opener_word = if joiners.len() == 1 { "opener" } else { "openers" };
    let luck_line = if winner_had_luck {
        "\n🍀 Their Lucky Horseshoe doubled the odds!"
    } else {
        ""
    };
    // Ruim het verweesde originele chest-bericht op (dode knop + bevroren timer),
    // net zoals een normale opening dat doet — anders is te zien dat er iets misliep.
    if let Err(e) = channel
        .delete_message(http.as_ref(), serenity::MessageId::new(msg_id))
        .await
    {
        tracing::warn!("kan verweesd chest-bericht {msg_id} niet verwijderen: {e}");
    }

    let embed = serenity::CreateEmbed::new()
        .title("The Magic Chest opened!")
        .description(format!(
            "Out of **{}** {opener_word}, <@{winner_uid}> got lucky!\n\
             They won **{prize}** {COIN_EMOJI} !!!{luck_line}",
            joiners.len()
        ))
        .colour(0x6B_9B_52);
    channel
        .send_message(http.as_ref(), serenity::CreateMessage::new().embed(embed))
        .await?;
    ctx.say(format!("✅ Chest `{msg_id}` heropend — {winner_name} won {prize} coins.")).await?;
    Ok(())
}

/// Bouw de chest-embed voor het huidige aantal deelnemers. Onder de drempel
/// (`chest_min_joiners`) toont hij "It will despawn <t:R>." + "Needs N more
/// participant(s)."; zodra er genoeg deelnemers zijn verdwijnt die regel en
/// wordt het "It will open <t:R>.". Herbruikt bij spawn én bij elke klik.
/// De drempel wordt hier live gelezen, dus de ticker toont een wijziging in
/// Settings ook aan een chest die al openstaat.
fn chest_embed(pool: &DbPool, pop_ts: i64, joiners: usize) -> serenity::CreateEmbed {
    let min_joiners = settings::usize_of(pool, "chest_min_joiners");
    let enough = joiners >= min_joiners;
    let verb = if enough { "open" } else { "despawn" };
    // Resterende tijd als M:SS — een ticker-taak werkt de embed periodiek bij zodat
    // dit zichtbaar aftelt (Discord's <t:R> telt boven 1 min niet per seconde af).
    let remaining = (pop_ts as f64 - now_secs()).max(0.0) as i64;
    let (mm, ss) = (remaining / 60, remaining % 60);
    // ### = iets groter (Markdown-header), één regel (Discord klapt spaties in).
    let mut desc =
        format!("### See if you win the **grand prize**! It will **{verb}** in **{mm}:{ss:02}**.");
    if !enough {
        let need = min_joiners - joiners;
        let p = if need == 1 { "participant" } else { "participants" };
        desc.push_str(&format!("\nNeeds **{need}** more {p}."));
    }
    serenity::CreateEmbed::new()
        .title("🎁 Fortuna's Favor")
        .description(desc)
        // Beide afbeeldingen via vaste URL (geen attachments) → goedkope, betrouwbare edits.
        .image("https://magicmeadow.org/img/chest.png")
        .thumbnail("https://cdn.discordapp.com/emojis/1526188363110023308.png?size=96")
        .colour(0xF1_C4_0F)
}

/// Post een chest-bericht (knop + afbeeldingen), registreer het in de tracker en
/// plan de pop. Herbruikt door de gewone spawn én de test-spawn bij startup.
async fn do_spawn_chest(
    http: Arc<serenity::Http>,
    channel_id: serenity::ChannelId,
    tracker: Arc<Mutex<ChestTracker>>,
    pool: DbPool,
    pop_delay: u64,
    triggers: &[(String, String)], // chatters die de chest uitlokten (leeg bij handmatig/test)
) -> Result<u64, Error> {
    let button = serenity::CreateButton::new(CHEST_CUSTOM_ID)
        .emoji('🎁')
        .label("Try your luck")
        .style(serenity::ButtonStyle::Success);
    let pop_ts = (now_secs() + pop_delay as f64) as i64;
    let builder = serenity::CreateMessage::new()
        .embed(chest_embed(&pool, pop_ts, 0)) // afbeeldingen via URL → geen attachments
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
    // Logboek: de chest verscheen, met (indien natuurlijk) wie hem uitlokte.
    let who = triggers
        .iter()
        .map(|(_, n)| n.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    db::log_event(
        &pool,
        now_secs(),
        &db::LogEntry::new("chest", "spawn")
            .channel(channel_id.get())
            .reference(msg_id)
            .detail(if who.is_empty() {
                "handmatig gespawned".to_string()
            } else {
                format!("uitgelokt door: {who}")
            }),
    );

    // Persistente opslag: overleeft een herstart (wordt bij opstart hervat).
    db::save_live_chest(&pool, msg_id, channel_id.get(), pop_ts);
    // Plan de pop-taak + de live M:SS-ticker.
    schedule_chest_tasks(http, pool, tracker, channel_id, msg_id, pop_ts);
    Ok(msg_id)
}

/// Plan de pop-taak + de M:SS-ticker voor een lopende chest. Herbruikt door een
/// verse spawn én door het hervatten van een chest na een herstart: de pop wacht
/// tot `pop_ts` (meteen als dat tijdstip al verstreken is).
fn schedule_chest_tasks(
    http: Arc<serenity::Http>,
    pool: DbPool,
    tracker: Arc<Mutex<ChestTracker>>,
    channel_id: serenity::ChannelId,
    msg_id: u64,
    pop_ts: i64,
) {
    let delay = (pop_ts as f64 - now_secs()).max(0.0) as u64;
    // Pop-taak: na de resterende tijd de chest openen/despawnen.
    let http2 = http.clone();
    let pool2 = pool.clone();
    let tracker2 = tracker.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
        pop_chest(http2, pool2, tracker2, channel_id, msg_id).await;
    });

    // Ticker: werk de M:SS-timer elke CHEST_TICK_SECS bij tot de chest weg is.
    let http3 = http.clone();
    let tracker3 = tracker.clone();
    let pool3 = pool.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(CHEST_TICK_SECS)).await;
            let info = {
                let t = tracker3.lock().unwrap();
                t.chests.get(&msg_id).map(|c| (c.pop_ts, c.joiners.len()))
            };
            match info {
                Some((pop_ts, n)) if (pop_ts as f64) > now_secs() + 1.0 => {
                    let builder =
                        serenity::EditMessage::new().embeds(vec![chest_embed(&pool3, pop_ts, n)]);
                    let _ = channel_id
                        .edit_message(http3.as_ref(), serenity::MessageId::new(msg_id), builder)
                        .await;
                }
                _ => break, // chest gepopt/despawned of pop-moment bereikt
            }
        }
    });
}

/// Registreer de chatter en spawn — bij ≥ CHEST_DISTINCT_USERS verschillende
/// chatters binnen CHEST_WINDOW — een treasure chest (met knop) in het kanaal.
/// Wordt enkel voor geldige (test)kanaal-berichten aangeroepen.
async fn maybe_spawn_chest(
    ctx: &serenity::Context,
    msg: &serenity::Message,
    data: &Data,
) -> Result<(), Error> {
    if !settings::bool_of(&data.pool, "chest_enabled") {
        return Ok(());
    }
    // Natuurlijke chests spawnen ENKEL in het aangewezen kanaal (Magic Meadow #general).
    if msg.channel_id.get() != CHEST_SPAWN_CHANNEL_ID {
        return Ok(());
    }
    let now = now_secs();
    let chan = msg.channel_id.get();
    let uid = msg.author.id.to_string();
    let name = msg
        .author
        .global_name
        .clone()
        .unwrap_or_else(|| msg.author.name.clone());

    // Lees de spawn-drempels vóór de lock: `settings::*` doet een DB-query en die
    // hoort niet binnen een gehouden Mutex.
    let window = settings::f64_of(&data.pool, "chest_window_min") * 60.0;
    let distinct_needed = settings::usize_of(&data.pool, "chest_distinct_users");
    // Beslis onder de lock: registreer de chatter, prune het venster, tel distinct.
    // Bij een spawn houden we de triggerende chatters (uid, naam) bij om te loggen.
    let (spawn, triggers) = {
        let mut t = data.chest.lock().unwrap();
        let on_cd = t.cooldown_until.get(&chan).is_some_and(|&u| u > now);
        let active = t.active.contains(&chan);
        let distinct = {
            let v = t.recent.entry(chan).or_default();
            v.retain(|(_, _, ts)| now - *ts < window); // verlopen entries weg
            v.retain(|(u, _, _)| u != &uid); // oude entry van deze uid weg (verse ts erbij)
            v.push((uid.clone(), name.clone(), now));
            v.iter().map(|(u, _, _)| u.as_str()).collect::<HashSet<_>>().len()
        };
        let go = !on_cd && !active && distinct >= distinct_needed;
        let mut triggers: Vec<(String, String)> = Vec::new();
        if go {
            if let Some(v) = t.recent.get_mut(&chan) {
                // Ontdubbel op uid (nieuwste naam wint) → de chatters die de chest uitlokten.
                let mut seen = HashSet::new();
                for (u, n, _) in v.iter().rev() {
                    if seen.insert(u.clone()) {
                        triggers.push((u.clone(), n.clone()));
                    }
                }
                v.clear(); // venster resetten zodat het niet blijft hertriggeren
            }
            t.active.insert(chan); // meteen reserveren → geen dubbele spawn
        }
        (go, triggers)
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
        pop_delay_secs(&data.pool),
        &triggers,
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
                    c.joiners.push((uid.clone(), name.clone()));
                    let n = c.joiners.len();
                    (Some(n), Some((c.pop_ts, n)))
                }
            }
        }
    };
    // Logboek: elke klik vastleggen — óók een te late klik (chest al gepopt),
    // want net dát verklaart een "ontbrekende" deelnemer bij de opening.
    let (log_event, log_detail) = match joined {
        None => ("too_late", "clicked after the chest was already gone".to_string()),
        Some(0) => ("already_in", "clicked again (already in)".to_string()),
        Some(n) => ("join", format!("participant #{n}")),
    };
    db::log_event(
        &data.pool,
        now_secs(),
        &db::LogEntry::new("chest", log_event)
            .actor(&uid, &name)
            .channel(mc.channel_id.get())
            .reference(msg_id)
            .detail(log_detail),
    );
    // Nieuwe deelnemer → werk de chest-embed bij (need-teller daalt; bij genoeg
    // deelnemers verdwijnt die regel en wordt "despawn" → "open").
    if let Some((pop_ts, n)) = edit {
        // Afbeeldingen zitten via URL in de embed → gewoon de embed bijwerken.
        let builder = serenity::EditMessage::new().embeds(vec![chest_embed(&data.pool, pop_ts, n)]);
        if let Err(e) = mc.channel_id.edit_message(&ctx.http, mc.message.id, builder).await {
            tracing::warn!("kan chest-embed niet bijwerken: {e}");
        }
    }
    let text = match joined {
        None => format!(
            "📦 Too late — make sure you click within **{} minutes** next time!",
            pop_delay_secs(&data.pool) / 60
        ),
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
    // Harde fix voor "verdwenen deelnemer": sluit éérst het klik-venster door de
    // knop te verwijderen — en dat WHILE de chest nog in de map zit, zodat klikken
    // die net nog binnenkomen gewoon als deelnemer geteld worden (niet stil als
    // "te laat" sneuvelen in het gaatje tussen map-verwijdering en bericht-wissen).
    // Pas nadat de knop weg is, nemen we de deelnemerslijst vast en trekken we.
    if let Err(e) = channel_id
        .edit_message(
            http.as_ref(),
            serenity::MessageId::new(msg_id),
            serenity::EditMessage::new().components(vec![]),
        )
        .await
    {
        tracing::warn!("kan chest-knop niet sluiten vóór pop ({msg_id}): {e}");
    }

    // Haal de chest eruit, geef het kanaal vrij, zet de cooldown.
    let cooldown_until = now_secs() + settings::f64_of(&pool, "chest_channel_cooldown_min") * 60.0;
    let (joiners, cd_channel) = {
        let mut t = tracker.lock().unwrap();
        let chest = t.chests.remove(&msg_id);
        if let Some(c) = &chest {
            t.active.remove(&c.channel_id);
            t.cooldown_until.insert(c.channel_id, cooldown_until);
        }
        match chest {
            Some(c) => {
                let chan = c.channel_id;
                (c.joiners, chan)
            }
            None => return, // al opgeruimd (zou niet mogen)
        }
    };
    // Cooldown ook op schijf zetten → overleeft een herstart (anders spawnt er
    // meteen na een redeploy weer een chest terwijl de rust nog zou moeten lopen).
    db::set_chest_cooldown(&pool, cd_channel, cooldown_until);
    // De chest is afgehandeld → uit de persistente lijst (geen hervatting bij opstart).
    db::delete_live_chest(&pool, msg_id);

    // Origineel (met chest-afbeelding) altijd verwijderen — anders raken de
    // attachments los van de embed en toont Discord de coin op volledige grootte.
    if let Err(e) = channel_id
        .delete_message(http.as_ref(), serenity::MessageId::new(msg_id))
        .await
    {
        tracing::warn!("kan chest-bericht {msg_id} niet verwijderen: {e}");
    }

    // Namen van alle deelnemers (voor het logboek-detail).
    let joiner_names = joiners
        .iter()
        .map(|(_, n)| n.as_str())
        .collect::<Vec<_>>()
        .join(", ");

    // Te weinig deelnemers → despawn: niks weggeven, wel een "Fortuna cries"-embed.
    if joiners.len() < settings::usize_of(&pool, "chest_min_joiners") {
        tracing::info!(
            "chest despawned in kanaal {channel_id} (te weinig deelnemers: {})",
            joiners.len()
        );
        db::log_event(
            &pool,
            now_secs(),
            &db::LogEntry::new("chest", "despawn")
                .channel(channel_id.get())
                .reference(msg_id)
                .amount(joiners.len() as i64)
                .detail(if joiner_names.is_empty() {
                    "0 participants".to_string()
                } else {
                    format!("{} participant(s): {joiner_names}", joiners.len())
                }),
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

    // Genoeg deelnemers → open. De winnaar wordt GEWOGEN getrokken: wie de Lucky
    // Horseshoe bezit, heeft 2 loten i.p.v. 1 (dubbele kans). De horseshoe is permanent —
    // hij blijft na de chest gewoon meetellen, er valt niets te verbruiken.
    let weights: Vec<u32> = joiners.iter().map(|(uid, _)| db::chest_weight(&pool, uid)).collect();
    let total_weight: u32 = weights.iter().sum();
    let mut roll = rand::thread_rng().gen_range(0..total_weight);
    let mut idx = 0;
    for (i, w) in weights.iter().enumerate() {
        if roll < *w {
            idx = i;
            break;
        }
        roll -= *w;
    }
    let winner_had_luck = weights[idx] > 1;
    let (winner_uid, winner_name) = &joiners[idx];
    let prize = chest_prize(&pool);
    let total = db::award(&pool, winner_uid, winner_name, prize, now_secs());
    log_earn(http.as_ref(), winner_name, prize, total).await;
    // Chest-winst kan je over een levelgrens tillen → level-up-check.
    maybe_levelup(&http, &pool, winner_uid, winner_name).await;
    let opener_word = if joiners.len() == 1 { "opener" } else { "openers" };
    tracing::info!(
        "chest geopend: {winner_name} wint {prize} coin(s) uit {} deelnemer(s)",
        joiners.len()
    );
    db::log_event(
        &pool,
        now_secs(),
        &db::LogEntry::new("chest", "win")
            .actor(winner_uid, winner_name)
            .channel(channel_id.get())
            .reference(msg_id)
            .amount(prize)
            .detail(format!(
                "won from {} participant(s): {joiner_names}",
                joiners.len()
            )),
    );
    let luck_line = if winner_had_luck {
        "\n🍀 Their Lucky Horseshoe doubled the odds!"
    } else {
        ""
    };
    let embed = serenity::CreateEmbed::new()
        .title("The Magic Chest opened!")
        .description(format!(
            "Out of **{}** {opener_word}, <@{winner_uid}> got lucky!\n\
             They won **{prize}** {COIN_EMOJI} !!!{luck_line}",
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

/// Namen komen als platte tekst in een embed; `_` en `*` in een Discord-naam zouden
/// anders de opmaak van de regel breken.
fn escape_md(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if matches!(ch, '*' | '_' | '~' | '`' | '|' | '\\' | '>' | '#' | '[' | ']') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// Post om HH:01 in #coins een embed met de grootste HOURLY_SHOUTOUT_TOP verdieners
/// van het net afgelopen klok-uur (≥ HOURLY_SHOUTOUT_MIN coins). De DB selecteert op
/// coins, maar het embed toont ze **alfabetisch** — het is een eregalerij, geen
/// rangschikking, dus geen medailles/plaatsnummers. Namen staan als platte
/// tekst in het embed — bewust géén mentions, dat pingt het hele lijstje elk uur.
/// Geen bericht als niemand iets verdiende. State zit in de DB (earn_log) →
/// overleeft een herstart.
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

        let mut earners = db::hourly_earners(&pool, since, until, min, HOURLY_SHOUTOUT_TOP);
        if !earners.is_empty() {
            // De query koos de grootste verdieners; het embed toont ze alfabetisch.
            earners.sort_by_key(|(_uid, name, _total)| name.to_lowercase());
            let lines: String = earners
                .iter()
                .map(|(_uid, name, total)| {
                    format!("🌼 {} — **{total}** {COIN_EMOJI}\n", escape_md(name))
                })
                .collect();
            let embed = serenity::CreateEmbed::new()
                .title("⏳ Earners of the last hour")
                .description(lines)
                .colour(0x6B_9B_52);
            let _ = serenity::ChannelId::new(PROD_COINS_CHANNEL_ID)
                .send_message(http.as_ref(), serenity::CreateMessage::new().embed(embed))
                .await;
            tracing::info!("uurlijkse top: {} lid/leden ≥{min} coins", earners.len());
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
        let top = db::leaderboard_week(&pool, since, 20);
        if top.is_empty() {
            continue;
        }
        // Plaats 1-3 = medailles; plaats 4-9 = cijfer-emoji (4️⃣…9️⃣); vanaf 10 = gewoon het getal.
        let medal = |i: usize| match i {
            0 => "👑".to_string(),
            1 => "🥈".to_string(),
            2 => "🥉".to_string(),
            3 => "4️⃣".to_string(),
            4 => "5️⃣".to_string(),
            5 => "6️⃣".to_string(),
            6 => "7️⃣".to_string(),
            7 => "8️⃣".to_string(),
            8 => "9️⃣".to_string(),
            n => format!("**{}.**", n + 1),
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
            .description(format!(
                "Top earners of the past week!\n\n{lines}\n🎉 **Top Three claim your prize below!**"
            ))
            .colour(0x6B_9B_52);
        // Cadeauknoppen voor de top 3 (Gold 300 / Silver 200 / Bronze 100). Elk enkel
        // claimbaar door de bijhorende winnaar (server-side check in claim_level_gift).
        // Hergebruikt de level_gifts-tabel (kind='weekly') voor de atomische claim.
        let now2 = now_secs();
        let mut buttons: Vec<serenity::CreateButton> = Vec::new();
        for (rank, amount, label) in [(0usize, 300i64, "Gold"), (1, 200, "Silver"), (2, 100, "Bronze")] {
            if let Some((uid, _n, _t)) = top.get(rank) {
                let gid = db::create_level_gift(&pool, uid, amount, 0, "weekly", now2);
                buttons.push(
                    serenity::CreateButton::new(format!("wg:{gid}"))
                        .label(label)
                        .style(serenity::ButtonStyle::Secondary),
                );
            }
        }
        let mut msg = serenity::CreateMessage::new().embed(embed);
        if !buttons.is_empty() {
            msg = msg.components(vec![serenity::CreateActionRow::Buttons(buttons)]);
        }
        if PROD_GENERAL_CHANNEL_ID != 0 {
            let _ = serenity::ChannelId::new(PROD_GENERAL_CHANNEL_ID)
                .send_message(http.as_ref(), msg)
                .await;
        }
    }
}

/// Klik op een weekly-cadeauknop (Gold/Silver/Bronze): keert het bedrag eenmalig uit aan de
/// bijhorende winnaar en post een publiek regeltje in #coins. Enkel de eigenaar; een andere
/// klikker of dubbelklik doet niets — **stil acken, GEEN ephemeral** (huisregel).
async fn handle_weekly_claim(
    ctx: &serenity::Context,
    mc: &serenity::ComponentInteraction,
    data: &Data,
) -> Result<(), Error> {
    let gid: i64 = mc
        .data
        .custom_id
        .strip_prefix("wg:")
        .and_then(|s| s.parse().ok())
        .unwrap_or(-1);
    let uid = mc.user.id.to_string();
    let name = mc
        .user
        .global_name
        .clone()
        .unwrap_or_else(|| mc.user.name.clone());
    // Altijd stil acken (geen ephemeral, geen zichtbaar antwoord op de interactie).
    let _ = mc
        .create_response(&ctx.http, serenity::CreateInteractionResponse::Acknowledge)
        .await;
    if let db::GiftClaim::Granted(amount) =
        db::claim_level_gift(&data.pool, gid, &uid, &name, now_secs())
    {
        if PROD_COINS_CHANNEL_ID != 0 {
            let _ = serenity::ChannelId::new(PROD_COINS_CHANNEL_ID)
                .say(
                    &ctx.http,
                    format!(
                        "**{}** won **{amount}** coins with the weekly leaderboard.",
                        escape_md(&name)
                    ),
                )
                .await;
        }
        db::log_event(
            &data.pool,
            now_secs(),
            &db::LogEntry::new("level", "weekly_claim")
                .actor(&uid, &name)
                .amount(amount)
                .detail("weekly leaderboard reward".to_string()),
        );
        maybe_levelup(&ctx.http, &data.pool, &uid, &name).await;
    }
    Ok(())
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
            commands: vec![
                chest(),
                chestodds(),
                chestrescue(),
            ],
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
            let resume_http = ctx.http.clone();
            Box::pin(async move {
                // Uurlijkse shout-out voor wie ≥100 coins verdiende in het afgelopen uur.
                tokio::spawn(hourly_shoutouts(hourly_http, hourly_pool));
                // Weekly leaderboard elke zaterdag 15:00 (Brussel) in prod #general.
                // TIJDELIJK UIT tot de cadeauknoppen-feature af/goedgekeurd is: de loop wordt
                // niet gespawnd → geen fire. Zet WEEKLY_LEADERBOARD_ENABLED weer op true wanneer klaar.
                if WEEKLY_LEADERBOARD_ENABLED {
                    tokio::spawn(weekly_leaderboard(weekly_http, weekly_pool));
                }

                // Chest-staat uit de DB herstellen → een herstart verliest niets meer:
                // (1) de per-kanaal cooldowns, (2) de lopende chests met hun pop-timer.
                let mut tracker = ChestTracker {
                    cooldown_until: db::load_chest_cooldowns(&pool, now_secs()),
                    ..Default::default()
                };
                let resume = db::load_live_chests(&pool);
                for &(msg_id, channel_id, pop_ts) in &resume {
                    // Deelnemers uit het logboek terughalen zodat de trekking klopt.
                    let joiners = db::chest_joiners_from_log(&pool, msg_id);
                    tracker.active.insert(channel_id);
                    tracker.chests.insert(msg_id, Chest { channel_id, joiners, pop_ts });
                }
                let chest = Arc::new(Mutex::new(tracker));

                // Voor elke hervatte chest de pop-taak + ticker opnieuw plannen (wacht
                // de resterende tijd; popt meteen als het pop-moment al voorbij is).
                if !resume.is_empty() {
                    tracing::info!("chest-herstel: {} lopende chest(s) hervat", resume.len());
                }
                for (msg_id, channel_id, pop_ts) in resume {
                    schedule_chest_tasks(
                        resume_http.clone(),
                        pool.clone(),
                        chest.clone(),
                        serenity::ChannelId::new(channel_id),
                        msg_id,
                        pop_ts,
                    );
                }

                Ok(Data { pool, cfg, chest })
            })
        })
        .build();

    let mut client = serenity::ClientBuilder::new(&token, intents)
        .framework(framework)
        .await?;
    client.start().await?;
    Ok(())
}
