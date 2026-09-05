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
const COIN_CHANNEL_ID: u64 = 1229046340793663488; // dev-#general (WaldsteinDevZone): enkel gebruikt door de CHEST_SPAWN_ON_START-testpad. Prod-coins/chests volgen de `coin_channels`-tabel.
const FORTUNA_LOG_CHANNEL_ID: u64 = 1526181603624226938; // Magic Meadow #fortuna-log: elke coin-verdienste (0 = uit)
const MEADOWMARKET_LOG_CHANNEL_ID: u64 = 0; // saldo-log uit op prod (fortuna-log dekt de verdiensten)
const PROD_COINS_CHANNEL_ID: u64 = 1403044480218824794; // Magic Meadow 🪙meadowcoins (shout-out + level-up + weekly)
const PROD_GENERAL_CHANNEL_ID: u64 = 1296469405651435594; // Magic Meadow ☀️general (weekly zaterdag 15u)
const PROD_GUILD_ID: u64 = 1296469405651435592; // Magic Meadow — leave/rejoin-archief triggert enkel hier
const DEV_COINS_CHANNEL_ID: u64 = 1525189157104648343; // dev "coins"-kanaal: previews/admin-rapporten (bv. thread-inhaalslag)
// Weekly leaderboard aan: vuurt elke zaterdag 15:00 (Brussel) in prod #general.
const WEEKLY_LEADERBOARD_ENABLED: bool = true;
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
const BIRTHDAY_ROLE_ID: u64 = 1422232059815919697; // Magic Meadow 🎂Birthday!🎂 (MEE6 zet hem op de dag zelf)
const CHEST_PING_ROLE_ID: u64 = 1544290527283912785; // Magic Meadow 🪙Chest!! — wordt gepingd bij een verjaardagsfeestje
const PSYCHE_BOT_ID: u64 = 1398743174104613026; // MEE6 ("Psyche") — zijn verjaardagsbericht in #general levert de leeftijd
const BUTTERBOTS_CHANNEL_ID: u64 = 1526293748906987591; // Magic Meadow 🦋butterbots — hier komt het verjaardagscadeau
const CHEST_SPAWN_CHANNEL_ID: u64 = 1296469405651435594; // Magic Meadow #general — nu enkel nog de terugval voor `chestrescue` als het logboek het originele kanaal niet kent. Natuurlijke chests volgen de coin-kanalen (zie maybe_spawn_chest).
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
    // Memo voor thread_parent: channel_id → Some(parent) als het een thread is, None
    // als het een gewoon kanaal is. Bespaart een get_channel-HTTP-call per bericht in
    // een niet-coin-kanaal (parent/thread-type zijn stabiel, dus veilig te cachen).
    parent_cache: Arc<Mutex<HashMap<u64, Option<u64>>>>,
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
/// #meadowmarket-log (namen en getallen in vet). Gebruikt voor berichten, daily én chest.
///
/// **Geen `<@id>`-vermeldingen in de logkanalen** (user-wens 2026-08-14): dat zijn
/// meelees-kanalen, en elke regel pingde het lid in kwestie. De naam staat er dus in het
/// vet. Elders (bv. de publieke regel in #coins of een chest-winnaar) blijft de
/// vermelding wél staan — daar is de ping net de bedoeling.
async fn log_earn(http: &serenity::Http, name: &str, amount: i64, total: i64) {
    if FORTUNA_LOG_CHANNEL_ID != 0 {
        let _ = serenity::ChannelId::new(FORTUNA_LOG_CHANNEL_ID)
            .say(http, format!("**{name}** + **{amount}** {COIN_EMOJI}"))
            .await;
    }
    if MEADOWMARKET_LOG_CHANNEL_ID != 0 {
        let _ = serenity::ChannelId::new(MEADOWMARKET_LOG_CHANNEL_ID)
            .say(http, format!("**{name}** balance: **{total}** {COIN_EMOJI}"))
            .await;
    }
}

/// Bij een bericht in een **thread** is `msg.channel_id` de thread zélf, niet het
/// bovenliggende kanaal. Voor de coin-check willen we dan het parent-kanaal gebruiken,
/// zodat threads in een coin-kanaal (bv. Arts & crafts) óók coins opleveren.
///
/// Geeft de parent-id terug enkel als het kanaal écht een thread is. Let op: een
/// gewóón kanaal heeft óók een `parent_id` (zijn categorie) — die mag hier NOOIT
/// als coin-kanaal gelden, vandaar de kind-gate. `to_channel` is cache-first (de
/// GUILDS-intent vult de thread-cache); enkel bij een cache-miss volgt één HTTP-call.
async fn thread_parent(ctx: &serenity::Context, data: &Data, id: serenity::ChannelId) -> Option<u64> {
    let key = id.get();
    // Memo eerst (guard valt vóór de await, nooit vastgehouden over een await).
    if let Some(cached) = data.parent_cache.lock().unwrap().get(&key).copied() {
        return cached;
    }
    // `to_channel` is in serenity 0.12 GÉÉN cache-lookup maar een echte get_channel-HTTP-call;
    // daarom memoïseren we het resultaat zodat elk kanaal maar één keer opgevraagd wordt.
    match id.to_channel(ctx).await {
        Ok(serenity::Channel::Guild(gc)) => {
            let is_thread = matches!(
                gc.kind,
                serenity::ChannelType::PublicThread
                    | serenity::ChannelType::PrivateThread
                    | serenity::ChannelType::NewsThread
            );
            let val = if is_thread { gc.parent_id.map(|p| p.get()) } else { None };
            data.parent_cache.lock().unwrap().insert(key, val);
            val
        }
        Ok(_) => {
            data.parent_cache.lock().unwrap().insert(key, None);
            None
        }
        // Transiënte fout (bv. rate-limit): NIET cachen → het volgende bericht probeert opnieuw.
        Err(e) => {
            tracing::warn!("thread_parent: kan kanaal {key} niet ophalen ({e})");
            None
        }
    }
}

/// Wáár mag een level-up-embed komen? Enkel in een kanaal waar ook een treasure chest mag
/// spawnen — dezelfde admin-beheerde `coin_channels`-lijst (een thread telt mee via zijn
/// parent). Levelt iemand door een knop te klikken in een kanaal dat er niet op staat
/// (bv. #meadowmarket), dan hoort het feestje daar niet thuis → terugval op prod #coins.
/// Bewust exact de check van `maybe_spawn_chest`: één lijst om te beheren.
async fn levelup_target(
    ctx: &serenity::Context,
    data: &Data,
    channel: serenity::ChannelId,
) -> serenity::ChannelId {
    let allowed = channel.get() != 0
        && (db::is_coin_channel(&data.pool, channel.get())
            || match thread_parent(ctx, data, channel).await {
                Some(parent) => db::is_coin_channel(&data.pool, parent),
                None => false,
            });
    if allowed {
        channel
    } else {
        serenity::ChannelId::new(PROD_COINS_CHANNEL_ID)
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
    // Activiteits-tracking voor Manage → Inactives: élk niet-bot-bericht in de prod-guild
    // ververst last_seen — ongeacht kanaal, commando of coin-cooldown (activiteit ≠ coins).
    if msg.guild_id.map(|g| g.get()) == Some(PROD_GUILD_ID) {
        let n = msg
            .author
            .global_name
            .clone()
            .unwrap_or_else(|| msg.author.name.clone());
        db::touch_activity(&data.pool, &msg.author.id.to_string(), &n, now_secs());
    }
    // Commando's (!coins e.d.) zijn immuun: geen coins, cooldown onaangeroerd.
    if msg.content.starts_with(PREFIX) {
        return Ok(());
    }
    // Coins per bericht enkel in kanalen op de admin-beheerde coin-kanalenlijst.
    // Lege lijst = nergens coins (progressieve activering). Een bericht in een
    // thread telt mee als zijn PARENT-kanaal op de lijst staat (thread_parent).
    let coin_here = db::is_coin_channel(&data.pool, msg.channel_id.get())
        || match thread_parent(ctx, data, msg.channel_id).await {
            Some(parent) => db::is_coin_channel(&data.pool, parent),
            None => false,
        };
    if !coin_here {
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
    // Fast-path Rust-check voor de normale flow; de échte guard zit atomisch in `award_if_ready`
    // (WHERE last_award <= guard_ts) en vangt twee snelle berichten die beide deze check passeren
    // vóór er iets geschreven is → geen dubbele award. `None` = race verloren (of net op cooldown).
    let awarded = if elapsed >= cooldown {
        let amount = coin_amount(&data.pool);
        db::award_if_ready(&data.pool, &uid, &name, amount, now, now - cooldown)
            .map(|total| (amount, total))
    } else {
        None
    };
    if let Some((amount, total)) = awarded {
        tracing::info!("{name}: +{amount} coins (totaal {total})");
        // Een award van 0 is een geldige uitkomst (rij `0` in coin_weights) en wordt
        // gelogd als elke andere: stilte las als een bug, niet als pech.
        log_earn(&ctx.http, &name, amount, total).await;
        // Level-up? → embed met claim-knop (gecentraliseerd, zie maybe_levelup). Dit pad is
        // pas bereikt ná de `coin_here`-gate hierboven, dus dit kanaal staat gegarandeerd op
        // de coin-kanalenlijst → geen `levelup_target` nodig.
        maybe_levelup(&ctx.http, &data.pool, &uid, &name, msg.channel_id).await;
        if COIN_FEEDBACK {
            msg.reply(ctx, format!("{COIN_EMOJI} +{amount} coins! Total: **{total}**"))
                .await?;
        }
    } else {
        let remaining = ((cooldown - elapsed) as i64 + 1).max(1);
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
                        // Activiteits-klok starten (Manage → Inactives): elk huidig prod-lid
                        // krijgt last_seen = nu, enkel als het nog niet bestaat (bestaande
                        // metingen blijven). Zo start niemand vals als "al lang inactief".
                        if gid.get() == PROD_GUILD_ID {
                            let ts = now_secs();
                            for m in &humans {
                                db::seed_activity(
                                    &data.pool,
                                    &m.user.id.to_string(),
                                    &m.display_name().to_string(),
                                    ts,
                                );
                            }
                            tracing::info!("Inactives: {} leden geseed op nu", humans.len());
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
        serenity::FullEvent::ReactionAdd { add_reaction } => {
            // Een reactie in de prod-guild telt als activiteit (Manage → Inactives).
            // Discord stuurt bij een guild-reactie het member-object mee → weergavenaam;
            // valt dat weg, dan updaten we last_seen met lege naam (blijft behouden).
            if add_reaction.guild_id.map(|g| g.get()) == Some(PROD_GUILD_ID) {
                if let Some(uid) = add_reaction.user_id {
                    let bot_react = add_reaction
                        .member
                        .as_ref()
                        .is_some_and(|m| m.user.bot);
                    if !bot_react {
                        let n = add_reaction
                            .member
                            .as_ref()
                            .map(|m| m.display_name().to_string())
                            .unwrap_or_default();
                        db::touch_activity(&data.pool, &uid.to_string(), &n, now_secs());
                    }
                }
            }
        }
        serenity::FullEvent::GuildMemberUpdate {
            old_if_available,
            event,
            ..
        } => {
            // Kreeg dit lid net de Birthday-rol? Dan post Fortuna het cadeau.
            // Stond de oude rollenlijst niet in de cache, dan vangt de
            // jaar-grendel in post_birthday_gift een dubbel cadeau op.
            if event.guild_id.get() == PROD_GUILD_ID {
                let has_now = event.roles.iter().any(|r| r.get() == BIRTHDAY_ROLE_ID);
                let had_before = old_if_available
                    .as_ref()
                    .map(|m| m.roles.iter().any(|r| r.get() == BIRTHDAY_ROLE_ID))
                    .unwrap_or(false);
                if has_now && !had_before {
                    let uid = event.user.id.to_string();
                    let name = event
                        .user
                        .global_name
                        .clone()
                        .unwrap_or_else(|| event.user.name.clone());
                    post_birthday_gift(&ctx.http, &data.pool, &uid, &name).await;
                }
            }
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
                } else if mc.data.custom_id.starts_with("bg:") {
                    handle_birthday_claim(ctx, mc, data).await?;
                } else if mc.data.custom_id.starts_with("pb:") {
                    handle_party_claim(ctx, mc, data).await?;
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
    // Atomische guard tegen de dubbelklik-race: enkel boeken als de cooldown écht verstreken
    // is (last_daily <= guard_ts). De Rust-check hierboven is de normale flow; deze guard vangt
    // twee interactie-tasks die beide die check passeren vóór er iets geschreven is.
    let guard_ts = now - daily_cooldown;
    let Some(total) = db::award_daily(&data.pool, &uid, &name, amount, streak, now, guard_ts)
    else {
        // Race verloren: een gelijktijdige claim was net eerder → toon dezelfde "too soon".
        let last = db::get_last_daily(&data.pool, &uid);
        let left = (daily_cooldown - (now - last)).max(0.0);
        let hrs = (left / 3600.0).floor() as i64;
        let mins = ((left % 3600.0) / 60.0).floor() as i64;
        respond_ephemeral(ctx, mc, &format!("⏳ Too soon! Come back in **{hrs}h {mins}m**.")).await?;
        return Ok(());
    };
    let day_word = if streak == 1 { "day" } else { "days" };
    tracing::info!("daily: {name} +{amount} (streak {streak}, totaal {total})");
    // Daily kan je over een levelgrens tillen → level-up-check. De knop kan in élk kanaal
    // geklikt zijn, dus het doelkanaal eerst toetsen aan de coin-kanalenlijst.
    let lvl_ch = levelup_target(ctx, data, mc.channel_id).await;
    maybe_levelup(&ctx.http, &data.pool, &uid, &name, lvl_ch).await;
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
                    "🔧 daily — **{name}** got **{amount}** {COIN_EMOJI} · streak **{streak}** · rolled in [**{lo}**–**{hi}**] · balance **{total}**"
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

// --- verjaardagscadeau ---------------------------------------------------

/// Het verjaardags-embed. De tekst staat er letterlijk zoals Faybelle ze
/// schreef; de `#`-regel is Discords grootste tekstformaat.
fn birthday_embed() -> serenity::CreateEmbed {
    serenity::CreateEmbed::new()
        .title("🎁 Fortuna's Gift")
        .description(
            "# <:MM_party:1522596802874835014> HAPPY BIRTHDAY!! <:MM_party:1522596802874835014>\n\
             Fortuna wishes you an amazing birthday.\n**You** are Celebrated!",
        )
        .image("https://magicmeadow.org/img/birthday.png")
        .colour(0xF1_C4_0F)
}

/// De knop onder het verjaardags-embed. `enabled = false` geeft de dode versie
/// voor `!birthdaytest`: wel te zien, niets te claimen.
fn birthday_button_row(custom_id: String, enabled: bool) -> serenity::CreateActionRow {
    serenity::CreateActionRow::Buttons(vec![serenity::CreateButton::new(custom_id)
        .emoji('🎁')
        .label("Open your Gift!")
        .style(serenity::ButtonStyle::Success)
        .disabled(!enabled)])
}

/// (jaar, "MM-DD") van een tijdstip in Brusselse tijd — zodat een verjaardag die
/// 's ochtends om 9u binnenkomt niet op de vorige dag valt.
fn birthday_date(ts: f64) -> (i64, String) {
    let (y, m, d) = db::brussels_ymd(ts);
    (y, format!("{m:02}-{d:02}"))
}

/// Het eerste getal in een tekst dat een leeftijd kán zijn (1 t/m 120). Lange
/// getallenreeksen (user-id's in mentions, jaartallen) vallen er vanzelf uit.
fn first_age(text: &str) -> Option<i64> {
    let mut run = String::new();
    for c in text.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_digit() {
            run.push(c);
            continue;
        }
        if !run.is_empty() {
            if let Ok(n) = run.parse::<i64>() {
                if (1..=120).contains(&n) {
                    return Some(n);
                }
            }
            run.clear();
        }
    }
    None
}

/// Zoek in de recente berichten van #general het verjaardagsbericht van Psyche
/// (MEE6) over dít lid en haal er de leeftijd uit → geboortejaar.
///
/// Het bericht wordt herkend aan: van Psyche, het woord "birthday" erin, en de
/// persoon erin vernoemd (als mention óf met zijn naam). Staat er geen leeftijd
/// in, dan houdt die persoon ze bewust voor zich en blijft het jaar leeg.
/// Het gevonden bericht gaat integraal naar de logs — het formaat is nog nooit
/// in het echt gezien, dus zo kunnen we bij de eerste jarige nakijken of de
/// leeftijd er correct uit komt.
async fn harvest_birth_year(
    http: &Arc<serenity::Http>,
    uid: &str,
    name: &str,
    year: i64,
) -> Option<i64> {
    let msgs = serenity::ChannelId::new(PROD_GENERAL_CHANNEL_ID)
        .messages(http, serenity::GetMessages::new().limit(50))
        .await
        .map_err(|e| tracing::warn!("kan #general niet lezen voor de leeftijd: {e}"))
        .ok()?;
    let needle = name.to_lowercase();
    for m in msgs.iter().filter(|m| m.author.id.get() == PSYCHE_BOT_ID) {
        let mut blob = m.content.clone();
        for e in &m.embeds {
            if let Some(t) = &e.title {
                blob.push(' ');
                blob.push_str(t);
            }
            if let Some(d) = &e.description {
                blob.push(' ');
                blob.push_str(d);
            }
            for f in &e.fields {
                blob.push_str(&format!(" {} {}", f.name, f.value));
            }
        }
        let low = blob.to_lowercase();
        if !low.contains("birthday") {
            continue;
        }
        if !(blob.contains(&format!("<@{uid}>")) || low.contains(&needle)) {
            continue;
        }
        let age = first_age(&blob);
        tracing::info!("Psyche-verjaardagsbericht voor {name}: {blob:?} → leeftijd {age:?}");
        return age.map(|a| year - a);
    }
    tracing::info!("geen verjaardagsbericht van Psyche gevonden voor {name} — geen leeftijd");
    None
}

/// Iemand kreeg de Birthday-rol: leg de verjaardag vast en post het cadeau in
/// #butterbots (mention op de eerste regel, embed + knop eronder).
///
/// Grendel: één cadeau per lid per kalenderjaar (`db::had_birthday_gift`). MEE6
/// zet de rol op en af en het vangnet leest de rollen opnieuw — zonder grendel
/// kon dezelfde jarige meermaals een cadeau pakken.
async fn post_birthday_gift(http: &Arc<serenity::Http>, pool: &DbPool, uid: &str, name: &str) {
    let now = now_secs();
    let (year, day) = birthday_date(now);
    db::record_birthday(pool, uid, &day, now);
    if db::last_birthday_gift_ts(pool, uid).is_some_and(|t| db::brussels_ymd(t).0 == year) {
        return;
    }
    // Eerst de leeftijd uit Psyche's bericht halen, dan pas posten: zo staat het
    // geboortejaar al vast tegen dat de jarige zijn cadeau opent en het feestje
    // begint. Kennen we het jaar al, dan hoeft er niets meer gezocht te worden.
    if db::birth_year(pool, uid).is_none() {
        if let Some(by) = harvest_birth_year(http, uid, name, year).await {
            db::set_birth_year(pool, uid, by);
        }
    }
    let amount = settings::i64_of(pool, "birthday_gift");
    let gid = db::create_level_gift(pool, uid, amount, 0, "birthday", now);
    db::log_event(
        pool,
        now,
        &db::LogEntry::new("birthday", "gift")
            .actor(uid, name)
            .amount(amount)
            .detail(format!("birthday {day}")),
    );
    let msg = serenity::CreateMessage::new()
        .content(format!("<@{uid}>"))
        .embed(birthday_embed())
        .components(vec![birthday_button_row(format!("bg:{gid}"), true)]);
    if let Err(e) = serenity::ChannelId::new(BUTTERBOTS_CHANNEL_ID)
        .send_message(http, msg)
        .await
    {
        tracing::warn!("verjaardagscadeau posten mislukt voor {name}: {e}");
    }
}

/// Klik op "Open your Gift!" — keert eenmalig uit aan de jarige zelf, antwoordt
/// onzichtbaar en meldt het publiek in #coins.
async fn handle_birthday_claim(
    ctx: &serenity::Context,
    mc: &serenity::ComponentInteraction,
    data: &Data,
) -> Result<(), Error> {
    // custom_id = "bg:<gift-id>" of "bg:<gift-id>:here" (die laatste komt van
    // !birthdaytestlive: dan landt het publieke regeltje hier i.p.v. in #coins).
    let rest = mc.data.custom_id.strip_prefix("bg:").unwrap_or_default();
    let here = rest.ends_with(":here");
    let gid: i64 = rest
        .trim_end_matches(":here")
        .parse()
        .unwrap_or(-1);
    let uid = mc.user.id.to_string();
    let name = mc
        .user
        .global_name
        .clone()
        .unwrap_or_else(|| mc.user.name.clone());
    match db::claim_level_gift(&data.pool, gid, &uid, &name, now_secs()) {
        db::GiftClaim::Granted(amount) => {
            // Het feestje mag enkel starten als het cadeau binnen 24 u na het
            // verschijnen geclaimd wordt. Te laat = wél de coins, geen feest.
            let created = db::level_gift_ts(&data.pool, gid).unwrap_or_else(now_secs);
            let in_time = here || now_secs() - created <= 24.0 * 3600.0;
            let extra = if in_time {
                "<:MM_party:1522596802874835014> *You have started a Party for all of the Magic \
                 Meadow, with a chest full of **Goodie Bags**! Yaay, partyyy!!!* \
                 <:MM_party:1522596802874835014>"
            } else {
                "You can only start a birthday party within 24 hours of your birthday!"
            };
            respond_ephemeral(
                ctx,
                mc,
                &format!(
                    "**Birthday Gift Claimed!** See <#{PROD_COINS_CHANNEL_ID}> channel to see \
                     what you got!\n{extra}"
                ),
            )
            .await?;
            let done = serenity::CreateButton::new(mc.data.custom_id.clone())
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
            let public = if here {
                mc.channel_id
            } else {
                serenity::ChannelId::new(PROD_COINS_CHANNEL_ID)
            };
            let _ = public
                .say(
                    &ctx.http,
                    format!(
                        "**{}** opened their Birthday Gift and got **{amount}** coins!",
                        escape_md(&name)
                    ),
                )
                .await;
            db::log_event(
                &data.pool,
                now_secs(),
                &db::LogEntry::new("birthday", "claim")
                    .actor(&uid, &name)
                    .amount(amount)
                    .detail("claimed birthday gift".to_string()),
            );
            // Zoals bij elk cadeau: de coins tellen mee voor total_earned, dus meteen
            // nakijken of er een level bij komt.
            let lvl_ch = levelup_target(ctx, data, mc.channel_id).await;
            maybe_levelup(&ctx.http, &data.pool, &uid, &name, lvl_ch).await;
            // Deel 2: het feestje openen. Bij een testcadeau blijft alles in dit
            // kanaal en zonder rol-ping.
            if in_time {
                let party_ch = if here {
                    mc.channel_id
                } else {
                    serenity::ChannelId::new(PROD_GENERAL_CHANNEL_ID)
                };
                post_party(&ctx.http, &data.pool, &uid, &name, party_ch, here).await;
            }
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

/// Vangnet: elk kwartier de leden mét de Birthday-rol nalezen en alsnog posten
/// voor wie dit jaar nog geen cadeau kreeg. Zonder dit mist een jarige zijn
/// cadeau als Fortuna net herstartte toen MEE6 de rol gaf.
async fn birthday_sweeper(http: Arc<serenity::Http>, pool: DbPool) {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(900)).await;
        let guild = serenity::GuildId::new(PROD_GUILD_ID);
        let mut after = serenity::UserId::new(1);
        loop {
            match guild.members(&http, Some(1000), Some(after)).await {
                Ok(batch) if !batch.is_empty() => {
                    after = batch[batch.len() - 1].user.id;
                    for m in batch {
                        if m.roles.iter().any(|r| r.get() == BIRTHDAY_ROLE_ID) {
                            let uid = m.user.id.to_string();
                            let name = m
                                .user
                                .global_name
                                .clone()
                                .unwrap_or_else(|| m.user.name.clone());
                            post_birthday_gift(&http, &pool, &uid, &name).await;
                        }
                    }
                }
                _ => break,
            }
        }
    }
}

// --- verjaardagsfeestje: goodie bags -------------------------------------

/// 1st, 2nd, 3rd, 4th … met de uitzonderingen 11th/12th/13th.
fn ordinal(n: i64) -> String {
    let suffix = match (n % 100, n % 10) {
        (11..=13, _) => "th",
        (_, 1) => "st",
        (_, 2) => "nd",
        (_, 3) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

#[cfg(test)]
mod leeftijd_uit_bericht {
    use super::first_age;

    /// Mentions en jaartallen mogen nooit voor een leeftijd doorgaan.
    #[test]
    fn pikt_de_leeftijd_en_niet_de_ruis() {
        assert_eq!(first_age("<@233179495094419456> turns 34 today!"), Some(34));
        assert_eq!(first_age("Happy 1st birthday"), Some(1));
        assert_eq!(first_age("Happy birthday! (2026)"), None);
        assert_eq!(first_age("Happy birthday <@233179495094419456>!"), None);
        assert_eq!(first_age("It is 9am, happy 40th birthday"), Some(9)); // bekend risico: eerste getal wint
    }
}

#[cfg(test)]
mod verjaardag_ordinaal {
    use super::ordinal;

    /// De valkuil zit in de tienertallen: 11/12/13 krijgen "th", niet st/nd/rd.
    #[test]
    fn suffix_klopt_ook_voor_de_tieners() {
        for (n, want) in [
            (1, "1st"),
            (2, "2nd"),
            (3, "3rd"),
            (4, "4th"),
            (11, "11th"),
            (12, "12th"),
            (13, "13th"),
            (21, "21st"),
            (22, "22nd"),
            (33, "33rd"),
            (100, "100th"),
            (111, "111th"),
        ] {
            assert_eq!(ordinal(n), want, "leeftijd {n}");
        }
    }
}

/// Het feest-embed. Kennen we het geboortejaar niet, dan valt het getal weg en
/// blijft dezelfde zin staan. Custom server-emoji renderen niet in een titel,
/// vandaar de gewone 🎉 daar.
fn party_embed(host_name: &str, age: Option<i64>) -> serenity::CreateEmbed {
    let host = escape_md(host_name);
    let line = match age {
        Some(a) => format!(
            "{host} is celebrating their {} Birthday today!",
            ordinal(a)
        ),
        None => format!("{host} is celebrating their Birthday today!"),
    };
    serenity::CreateEmbed::new()
        .title(format!("🎉 {host_name}'s Birthday Party!!"))
        .description(format!(
            "# {line}\nHere are some Party Goodie Bags they're treating you all with!!"
        ))
        .image("https://magicmeadow.org/img/goodiebags.png")
        .colour(0xF1_C4_0F)
}

/// De goodie-bag-knop. `enabled = false` = de grijze "Claimed"-versie.
fn party_button_row(custom_id: String, enabled: bool) -> serenity::CreateActionRow {
    let label = if enabled { "Grab a Goodie Bag!" } else { "Claimed" };
    let style = if enabled {
        serenity::ButtonStyle::Success
    } else {
        serenity::ButtonStyle::Secondary
    };
    serenity::CreateActionRow::Buttons(vec![serenity::CreateButton::new(custom_id)
        .emoji('🎁')
        .label(label)
        .style(style)
        .disabled(!enabled)])
}

/// Post het feestje: rol-ping + embed + knop, en pin het bericht. `test` = een
/// proefdraai (`!partytestlive`): dan géén rol-ping, een korte looptijd, en het
/// publieke regeltje blijft in hetzelfde kanaal.
async fn post_party(
    http: &Arc<serenity::Http>,
    pool: &DbPool,
    host_uid: &str,
    host_name: &str,
    channel: serenity::ChannelId,
    test: bool,
) {
    let now = now_secs();
    let secs = if test {
        600.0
    } else {
        settings::i64_of(pool, "party_hours") as f64 * 3600.0
    };
    let pid = db::create_party(pool, host_uid, host_name, channel.get(), now, now + secs, test);
    // Leeftijd = dit jaar min het geboortejaar, als we dat kennen.
    let age = db::birth_year(pool, host_uid).map(|y| db::brussels_ymd(now).0 - y);
    let cid = if test {
        format!("pb:{pid}:here")
    } else {
        format!("pb:{pid}")
    };
    let mut msg = serenity::CreateMessage::new()
        .embed(party_embed(host_name, age))
        .components(vec![party_button_row(cid, true)]);
    if !test {
        msg = msg.content(format!("<@&{CHEST_PING_ROLE_ID}>"));
    }
    match channel.send_message(http, msg).await {
        Ok(m) => {
            db::set_party_message(pool, pid, m.id.get());
            if let Err(e) = m.pin(http).await {
                tracing::warn!("feestbericht kon niet gepind worden: {e}");
            }
        }
        Err(e) => tracing::warn!("feestbericht posten mislukt voor {host_name}: {e}"),
    }
}

/// Klik op de goodie bag: loot een bedrag, boekt het eenmalig per persoon, en
/// meldt het publiek in #coins.
async fn handle_party_claim(
    ctx: &serenity::Context,
    mc: &serenity::ComponentInteraction,
    data: &Data,
) -> Result<(), Error> {
    let rest = mc.data.custom_id.strip_prefix("pb:").unwrap_or_default();
    let here = rest.ends_with(":here");
    let pid: i64 = rest.trim_end_matches(":here").parse().unwrap_or(-1);
    let uid = mc.user.id.to_string();
    let name = mc
        .user
        .global_name
        .clone()
        .unwrap_or_else(|| mc.user.name.clone());
    let lo = settings::i64_of(&data.pool, "party_bag_min");
    let hi = settings::i64_of(&data.pool, "party_bag_max").max(lo);
    let amount = rand::thread_rng().gen_range(lo..=hi);
    match db::claim_party_bag(&data.pool, pid, &uid, &name, amount, now_secs()) {
        db::BagClaim::Granted(amount) => {
            respond_ephemeral(
                ctx,
                mc,
                &format!(
                    "Yaay, you opened your Party Goodie Bag!! Go see what was in it in \
                     <#{PROD_COINS_CHANNEL_ID}>!"
                ),
            )
            .await?;
            let (host_uid, host) = db::party_host(&data.pool, pid).unwrap_or_default();
            let public = if here {
                mc.channel_id
            } else {
                serenity::ChannelId::new(PROD_COINS_CHANNEL_ID)
            };
            // De jarige die uit zijn eigen kist pakt, krijgt een eigen regel —
            // anders stond zijn naam er twee keer in dezelfde zin.
            let line = if host_uid == uid {
                format!(
                    "**{}** treated themselves to one of their own Party Goodie Bags and got \
                     **{amount}** coins!",
                    escape_md(&name)
                )
            } else {
                format!(
                    "**{}** got **{amount}** coins out of **{}**'s Party Goodie Bag! \
                     Be sure to wish them a Happy Birthday!!",
                    escape_md(&name),
                    escape_md(&host)
                )
            };
            let _ = public.say(&ctx.http, line).await;
            db::log_event(
                &data.pool,
                now_secs(),
                &db::LogEntry::new("birthday", "goodiebag")
                    .actor(&uid, &name)
                    .amount(amount)
                    .detail(format!("goodie bag from {host}")),
            );
            // Bij een testfeestje bewogen er geen coins → ook geen level-check.
            if !here {
                let lvl_ch = levelup_target(ctx, data, mc.channel_id).await;
                maybe_levelup(&ctx.http, &data.pool, &uid, &name, lvl_ch).await;
            }
        }
        db::BagClaim::AlreadyTaken => {
            respond_ephemeral(ctx, mc, "You already grabbed a Goodie Bag from this chest!").await?
        }
        db::BagClaim::Closed => {
            respond_ephemeral(ctx, mc, "This reward is no longer available.").await?
        }
    }
    Ok(())
}

/// Sluiter: elke minuut de afgelopen feestjes grijs maken en ontpinnen. Leest de
/// DB, dus een herstart tijdens een feestje verandert niets.
async fn party_closer(http: Arc<serenity::Http>, pool: DbPool) {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        let now = now_secs();
        for (pid, ch, msg, expires, _test) in db::open_parties(&pool) {
            if now < expires {
                continue;
            }
            if msg != 0 {
                let channel = serenity::ChannelId::new(ch);
                let mid = serenity::MessageId::new(msg);
                let _ = channel
                    .edit_message(
                        &http,
                        mid,
                        serenity::EditMessage::new()
                            .components(vec![party_button_row(format!("pb:{pid}"), false)]),
                    )
                    .await;
                let _ = channel.unpin(&http, mid).await;
            }
            db::close_party(&pool, pid);
        }
    }
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
/// De claim boekt de coins als échte verdienste (`total_earned` + `earn_log`):
/// álle coins tellen mee voor de level-up, ongeacht bron. Geen op-hol-slaan: 1,5% < een levelgat.
///
/// `channel` = wáár het embed komt: het kanaal dat de level-up uitlokte (het bericht dat de coins
/// opleverde, de chest, de daily-knop). Vroeger ging alles naar #coins; dat haalde het feestje weg
/// bij het gesprek waar het lid mee bezig was. Terugval op #coins als er geen kanaal is.
///
/// ⚠️ De caller levert een **toegelaten** kanaal aan: enkel kanalen waar ook chests mogen spawnen
/// (de `coin_channels`-lijst). Komt de level-up van een knopklik die overal kan gebeuren, dan hoort
/// daar `levelup_target()` vóór — anders belandt het embed in bv. #meadowmarket.
async fn maybe_levelup(
    http: &Arc<serenity::Http>,
    pool: &DbPool,
    uid: &str,
    name: &str,
    channel: serenity::ChannelId,
) {
    let (coins, _max, _pub, earned) = db::get_stats(pool, uid);
    let cur = db::level_of(earned);
    // Claim de range [prev+1, cur] atomisch (compare-and-swap op de marker). Een gelijktijdige
    // 2e aanroep (bv. bericht + daily tegelijk) krijgt None → post niets → geen dubbele
    // cadeaus/embeds. `prev` is de marker-waarde op het claim-moment, niet een losse read.
    let Some(gifted) = db::advance_gifted_level(pool, uid, cur) else {
        return;
    };
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
        // Kanaal van de uitlokker; enkel als dat ontbreekt vallen we terug op #coins.
        let target = if channel.get() != 0 {
            channel
        } else {
            serenity::ChannelId::new(PROD_COINS_CHANNEL_ID)
        };
        if target.get() != 0 {
            // De `<@uid>` ín het embed rendert wel als naam maar pingt niet — Discord stuurt geen
            // melding voor een mention in een embed. Daarom de mention óók als gewone berichttekst
            // vlak boven het embed: dát is wat het lid effectief een seintje geeft.
            let builder = serenity::CreateMessage::new()
                .content(format!("<@{uid}>"))
                .embed(levelup_embed(uid, level))
                .components(vec![claim_button_row(format!("lg:{gid}"))]);
            let _ = target.send_message(http, builder).await;
        }
    }
    // Marker is al vooraf gezet door advance_gifted_level (atomische claim) — niets meer te doen.
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
            let lvl_ch = levelup_target(ctx, data, mc.channel_id).await;
            maybe_levelup(&ctx.http, &data.pool, &uid, &name, lvl_ch).await;
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

/// Kies een index uit `weights` bij een gegeven `roll` ∈ [0, Σweights): loop de
/// gewichten af en pak de eerste waar de roll binnenvalt. Apart en puur gehouden
/// omdat dit de winnaar van een treasure chest bepaalt (Lucky Horseshoe = 2 loten):
/// zo is de verdeling exact te bewijzen i.p.v. te bemonsteren — zie `mod horseshoe_odds`.
/// Lege `weights` of een te grote roll kan niet voorkomen (caller telt zelf op), maar
/// valt terug op 0 i.p.v. te panieken tijdens een uitbetaling.
fn pick_weighted(weights: &[u32], mut roll: u32) -> usize {
    for (i, w) in weights.iter().enumerate() {
        if roll < *w {
            return i;
        }
        roll -= *w;
    }
    0
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

/// `!test` — Fortuna post zelf "Testing started" in dit kanaal. Bedoeld als
/// bewijs dat de bot in een (privé-)testkanaal kan schrijven. Admin-only, dus
/// bruikbaar op de prod-guild. Het commando-bericht wordt opgeruimd door de
/// `pre_command`-hook.
#[poise::command(prefix_command, check = "admin_only")]
pub async fn test(ctx: Context<'_>) -> Result<(), Error> {
    ctx.say("Testing started").await?;
    Ok(())
}

/// `!birthdaytest` — toon het verjaardags-embed in dit kanaal, met een dode
/// knop: enkel om te zien hoe het eruitziet. Er wordt geen cadeau aangemaakt en
/// er bewegen geen coins. Admin-only, dus bruikbaar op de prod-guild.
#[poise::command(prefix_command, check = "admin_only")]
pub async fn birthdaytest(ctx: Context<'_>) -> Result<(), Error> {
    let uid = ctx.author().id.to_string();
    ctx.channel_id()
        .send_message(
            ctx.serenity_context().http.clone(),
            serenity::CreateMessage::new()
                .content(format!("<@{uid}>"))
                .embed(birthday_embed())
                .components(vec![birthday_button_row("bg:preview".to_string(), false)]),
        )
        .await?;
    Ok(())
}

/// `!birthdaytestlive` — hetzelfde bericht, maar met een **werkende** knop van
/// 0 coins in dit kanaal. Zo zie je het volledige pad (klik → onzichtbaar
/// antwoord → knop wordt grijs → publiek regeltje) zonder dat er coins bewegen
/// en zonder iets in #coins te zetten. Soort `birthdaytest`, dus het telt niet
/// mee voor de jaargrendel van een echt verjaardagscadeau. Admin-only.
#[poise::command(prefix_command, check = "admin_only")]
pub async fn birthdaytestlive(ctx: Context<'_>) -> Result<(), Error> {
    let uid = ctx.author().id.to_string();
    let name = ctx
        .author()
        .global_name
        .clone()
        .unwrap_or_else(|| ctx.author().name.clone());
    let gid = db::create_level_gift(&ctx.data().pool, &uid, 0, 0, "birthdaytest", now_secs());
    tracing::info!("birthdaytestlive: cadeau {gid} van 0 coins voor {name}");
    ctx.channel_id()
        .send_message(
            ctx.serenity_context().http.clone(),
            serenity::CreateMessage::new()
                .content(format!("<@{uid}>"))
                .embed(birthday_embed())
                .components(vec![birthday_button_row(format!("bg:{gid}:here"), true)]),
        )
        .await?;
    Ok(())
}

/// `!partytest [leeftijd]` — toon het feest-embed in dit kanaal met een dode
/// knop en zonder rol-ping. Zonder getal krijg je de zin zonder leeftijd (wat
/// vandaag iedereen krijgt); met `!partytest 34` zie je hoe "34th" oogt.
#[poise::command(prefix_command, check = "admin_only")]
pub async fn partytest(ctx: Context<'_>, age: Option<i64>) -> Result<(), Error> {
    let name = ctx
        .author()
        .global_name
        .clone()
        .unwrap_or_else(|| ctx.author().name.clone());
    ctx.channel_id()
        .send_message(
            ctx.serenity_context().http.clone(),
            serenity::CreateMessage::new()
                .embed(party_embed(&name, age))
                .components(vec![party_button_row("pb:preview".to_string(), false)]),
        )
        .await?;
    Ok(())
}

/// `!partytestlive` — een echt feestje in dit kanaal: werkende knop, echte
/// goodie bags, maar zonder rol-ping en met het publieke regeltje hier i.p.v.
/// in #coins. Loopt 10 minuten i.p.v. 24 uur, zodat je het grijs worden en het
/// ontpinnen meteen ziet. Er wordt wel een bedrag geloot en getoond, maar er
/// worden géén coins uitbetaald (zie `db::claim_party_bag`, vlag `test`).
#[poise::command(prefix_command, check = "admin_only")]
pub async fn partytestlive(ctx: Context<'_>) -> Result<(), Error> {
    let uid = ctx.author().id.to_string();
    let name = ctx
        .author()
        .global_name
        .clone()
        .unwrap_or_else(|| ctx.author().name.clone());
    post_party(
        &ctx.serenity_context().http,
        &ctx.data().pool,
        &uid,
        &name,
        ctx.channel_id(),
        true,
    )
    .await;
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

    // Zonder message-id: de laatste verweesde chest automatisch opzoeken.
    let msg_id = match msg_id.or_else(|| db::last_unresolved_chest(pool)) {
        Some(id) => id,
        None => {
            ctx.say("⚠️ No orphaned (unresolved) chest found in the log.").await?;
            return Ok(());
        }
    };
    // Het kanaal waar de chest écht stond (chests spawnen nu in álle coin-kanalen).
    // Terugval op #general enkel als het logboek geen kanaal kent (oude chest).
    let channel = serenity::ChannelId::new(
        db::chest_channel_from_log(pool, msg_id).unwrap_or(CHEST_SPAWN_CHANNEL_ID),
    );

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
    let idx = pick_weighted(&weights, rand::thread_rng().gen_range(0..total_weight));
    let winner_had_luck = weights[idx] > 1;
    let (winner_uid, winner_name) = &joiners[idx];
    let prize = chest_prize(pool);
    let total = db::award(pool, winner_uid, winner_name, prize, now_secs());
    log_earn(http.as_ref(), winner_name, prize, total).await;
    maybe_levelup(&http, pool, winner_uid, winner_name, channel).await;
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

// ---------------------------------------------------------------------------
// Retroactieve thread-inhaalslag (!threadfix_preview / _commit / _reset)
//
// Threads leverden vroeger geen coins op (msg.channel_id = de thread, niet het
// coin-kanaal — zie thread_parent). Deze inhaalslag scant alle threads onder de
// coin-kanalen, rolt PER BERICHT een bedrag (coin_weights, géén cooldown), bevriest
// dat in `thread_backfill` en toont in dev-coins wie hoeveel krijgt. Pas na
// `!threadfix_commit` worden de saldi op prod echt bijgewerkt. Idempotent + resumable.
// ---------------------------------------------------------------------------

/// Post tekst naar een kanaal in blokken < 2000 tekens (Discord-limiet).
async fn post_chunks(dc: &crate::discord_rest::Discord, channel: &str, header: &str, lines: &[String]) {
    let mut buf = header.to_string();
    for line in lines {
        if buf.len() + line.len() + 1 > 1900 {
            let _ = dc.send_channel_message(channel, &buf).await;
            buf.clear();
        }
        if !buf.is_empty() {
            buf.push('\n');
        }
        buf.push_str(line);
    }
    if !buf.is_empty() {
        let _ = dc.send_channel_message(channel, &buf).await;
    }
}

async fn run_thread_backfill_preview(pool: DbPool, cfg: Config) {
    let dc = crate::discord_rest::Discord::new(cfg.bot_token.clone(), PROD_GUILD_ID.to_string());
    let guild = PROD_GUILD_ID.to_string();
    let ch = DEV_COINS_CHANNEL_ID.to_string();

    let coin_chs = db::coin_channels(&pool); // Vec<(channel_id, naam)>
    if coin_chs.is_empty() {
        let _ = dc
            .send_channel_message(&ch, "⚠️ Thread-inhaalslag: er zijn geen coin-kanalen ingesteld.")
            .await;
        return;
    }
    let _ = dc
        .send_channel_message(
            &ch,
            &format!("⏳ Thread-inhaalslag gestart — {} coin-kanalen worden gescand…", coin_chs.len()),
        )
        .await;

    let coin_set: HashSet<String> = coin_chs.iter().map(|(id, _)| id.clone()).collect();
    let members: HashMap<String, String> = match dc.list_members(&guild).await {
        Ok(v) => v.into_iter().collect(),
        Err(e) => {
            let _ = dc.send_channel_message(&ch, &format!("❌ Kan de ledenlijst niet ophalen: {e}")).await;
            return;
        }
    };

    // Alle threads verzamelen: actief (guild-breed, gefilterd op coin-parent) + gearchiveerd
    // (per kanaal, publiek + private). Dedup op thread-id.
    let mut threads: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut warns: Vec<String> = Vec::new();
    match dc.active_threads(&guild).await {
        Ok(list) => {
            for (tid, parent) in list {
                if coin_set.contains(&parent) && seen.insert(tid.clone()) {
                    threads.push(tid);
                }
            }
        }
        Err(e) => warns.push(format!("actieve threads: {e}")),
    }
    for (cid, cname) in &coin_chs {
        for private in [false, true] {
            match dc.archived_threads(cid, private).await {
                Ok(ids) => {
                    for tid in ids {
                        if seen.insert(tid.clone()) {
                            threads.push(tid);
                        }
                    }
                }
                // 403 op private threads is normaal zonder Manage Threads — enkel loggen.
                Err(e) => warns.push(format!(
                    "#{cname} gearchiveerd ({}): {e}",
                    if private { "private" } else { "publiek" }
                )),
            }
        }
    }

    // Berichten per thread aflopen en per bericht rollen (enkel nieuwe berichten).
    let now = now_secs();
    let mut new_msgs = 0i64;
    let mut left_skipped = 0i64;
    for tid in &threads {
        let mut before: Option<String> = None;
        loop {
            let batch = match dc.get_messages_detailed(tid, before.as_deref(), 100).await {
                Ok(b) => b,
                Err(e) => {
                    warns.push(format!("thread {tid}: {e}"));
                    break;
                }
            };
            if batch.is_empty() {
                break;
            }
            let mut oldest = u64::MAX;
            for (aid, mid, is_bot, content) in &batch {
                if *mid < oldest {
                    oldest = *mid;
                }
                if *is_bot || content.starts_with(PREFIX) {
                    continue;
                }
                let name = match members.get(aid) {
                    Some(n) => n.clone(),
                    None => {
                        left_skipped += 1; // auteur verliet de server → niks uit te keren
                        continue;
                    }
                };
                let amount = coin_amount(&pool);
                if db::backfill_record(&pool, &mid.to_string(), aid, &name, amount, now) {
                    new_msgs += 1;
                }
            }
            before = Some(oldest.to_string());
            tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        }
    }

    // Rapport samenstellen.
    let (coins, msgs, users) = db::backfill_totals(&pool);
    let pending = db::backfill_pending(&pool);
    let header = format!(
        "🧵 **Thread-inhaalslag — PREVIEW** (nog niets uitbetaald)\n\
         Threads gescand: **{}** · nieuwe berichten deze scan: **{}**\n\
         Regels: per bericht gerold (géén cooldown); bots, `!`-commando's en oud-leden ({} berichten) uitgesloten.\n\
         De coins tellen mee voor leveling (total_earned) maar komen NIET in het weekly/uur-overzicht.\n\
         ────────────",
        threads.len(),
        new_msgs,
        left_skipped
    );
    if pending.is_empty() {
        let _ = dc
            .send_channel_message(&ch, &format!("{header}\n_Niets te vergoeden — geen thread-berichten gevonden._"))
            .await;
    } else {
        let mut lines: Vec<String> = pending
            .iter()
            .map(|(_, name, sum, cnt)| format!("**{name}** — {sum} {COIN_EMOJI}  ({cnt} berichten)"))
            .collect();
        lines.push("────────────".to_string());
        lines.push(format!("**Totaal: {coins} {COIN_EMOJI}** · {users} leden · {msgs} berichten"));
        lines.push("▶️ Akkoord? `!threadfix_commit` betaalt uit op prod. Opnieuw rollen? `!threadfix_reset` → `!threadfix_preview`.".to_string());
        post_chunks(&dc, &ch, &header, &lines).await;
    }
    if !warns.is_empty() {
        let shown: Vec<String> = warns.iter().take(10).cloned().collect();
        let extra = if warns.len() > 10 { format!(" (+{} meer)", warns.len() - 10) } else { String::new() };
        let _ = dc
            .send_channel_message(
                &ch,
                &format!("⚠️ Overgeslagen (leesrecht/403){extra}:\n{}", shown.join("\n")),
            )
            .await;
    }
}

/// `!threadfix_preview` — scan alle threads onder de coin-kanalen, rol per bericht en toon
/// in dev-coins wie hoeveel alsnog krijgt. Betaalt NIETS uit. Admin-only, draait in de
/// achtergrond (kan even duren bij veel threads).
#[poise::command(prefix_command, check = "admin_only")]
pub async fn threadfix_preview(ctx: Context<'_>) -> Result<(), Error> {
    let pool = ctx.data().pool.clone();
    let cfg = ctx.data().cfg.clone();
    tokio::spawn(async move { run_thread_backfill_preview(pool, cfg).await });
    Ok(())
}

/// `!threadfix_commit` — betaal de getoonde inhaalslag echt uit op prod (idempotent).
/// Admin-only.
#[poise::command(prefix_command, check = "admin_only")]
pub async fn threadfix_commit(ctx: Context<'_>) -> Result<(), Error> {
    let pool = ctx.data().pool.clone();
    let cfg = ctx.data().cfg.clone();
    tokio::spawn(async move {
        let dc = crate::discord_rest::Discord::new(cfg.bot_token.clone(), PROD_GUILD_ID.to_string());
        let ch = DEV_COINS_CHANNEL_ID.to_string();
        let (_, msgs, users) = db::backfill_totals(&pool);
        if users == 0 {
            let _ = dc
                .send_channel_message(&ch, "ℹ️ Niets openstaand — draai eerst `!threadfix_preview`.")
                .await;
            return;
        }
        let done = db::backfill_apply(&pool);
        let paid: i64 = done.iter().map(|(_, _, a)| *a).sum();
        let _ = dc
            .send_channel_message(
                &ch,
                &format!(
                    "✅ **Thread-inhaalslag uitbetaald op prod** — {paid} {COIN_EMOJI} over {} leden ({msgs} berichten). \
                     Saldi bijgewerkt; een eventuele level-up volgt vanzelf bij de volgende activiteit.",
                    done.len()
                ),
            )
            .await;
    });
    Ok(())
}

/// `!threadfix_reset` — gooi de openstaande (nog niet uitbetaalde) inhaalslag weg om
/// opnieuw te rollen. Raakt al uitbetaalde rijen niet aan. Admin-only.
#[poise::command(prefix_command, check = "admin_only")]
pub async fn threadfix_reset(ctx: Context<'_>) -> Result<(), Error> {
    let n = db::backfill_reset_pending(&ctx.data().pool);
    let dc = crate::discord_rest::Discord::new(
        ctx.data().cfg.bot_token.clone(),
        PROD_GUILD_ID.to_string(),
    );
    let _ = dc
        .send_channel_message(
            &DEV_COINS_CHANNEL_ID.to_string(),
            &format!("🧹 {n} openstaande rijen gewist — `!threadfix_preview` rolt opnieuw."),
        )
        .await;
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
    // Natuurlijke chests spawnen in de coin-kanalen (en hun threads) — exact dezelfde
    // admin-beheerde lijst die coins uitkeert (`coin_channels`). Meadowland (de in-game
    // chat-bridge) staat niet op die lijst en krijgt dus géén chests. Een bericht in een
    // thread telt mee als zijn PARENT-kanaal een coin-kanaal is; de chest verschijnt dan
    // ín die thread (het venster keyt op de thread-id). Deze check is vandaag redundant
    // met de coin-gate in `handle_message`, maar houdt `maybe_spawn_chest` zelfstandig.
    let chest_here = db::is_coin_channel(&data.pool, msg.channel_id.get())
        || match thread_parent(ctx, data, msg.channel_id).await {
            Some(parent) => db::is_coin_channel(&data.pool, parent),
            None => false,
        };
    if !chest_here {
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
    let idx = pick_weighted(&weights, rand::thread_rng().gen_range(0..total_weight));
    let winner_had_luck = weights[idx] > 1;
    let (winner_uid, winner_name) = &joiners[idx];
    let prize = chest_prize(&pool);
    let total = db::award(&pool, winner_uid, winner_name, prize, now_secs());
    log_earn(http.as_ref(), winner_name, prize, total).await;
    // Chest-winst kan je over een levelgrens tillen → level-up-check. Een chest spawnt enkel
    // in een coin-kanaal (of een thread daarvan), dus dit kanaal is per definitie toegelaten.
    maybe_levelup(&http, &pool, winner_uid, winner_name, channel_id).await;
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
    // Marker met het tijdstip van de laatst-gevuurde zaterdag-15:00, zodat een gemiste fire
    // (bot lag plat rond za 15:00) na herstart alsnog wordt ingehaald i.p.v. overgeslagen.
    const MARKER: &str = "weekly_last_fired";
    loop {
        let now = now_secs();
        let last_sat = db::last_saturday_1500_brussels(now);
        let last_fired = db::kv_get(&pool, MARKER)
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        if last_fired <= 0.0 {
            // Eerste run met deze marker: niet met terugwerkende kracht posten — markeer de
            // huidige occurrence als afgehandeld zodat we enkel vooruit vuren.
            db::kv_set(&pool, MARKER, &last_sat.to_string());
        } else if last_fired < last_sat {
            // Gemiste (of net-nu aangebroken) zaterdag-15:00 → inhalen: post het venster van
            // `last_sat`. Ook bij een lege week zetten we de marker, zodat we niet blijven retryen.
            post_weekly_leaderboard(&http, &pool, last_sat).await;
            db::kv_set(&pool, MARKER, &last_sat.to_string());
        }
        // Slaap tot de volgende zaterdag 15:00 (Brussel).
        let now = now_secs();
        let next = db::next_saturday_1500_brussels(now);
        tokio::time::sleep(std::time::Duration::from_secs_f64((next - now).max(1.0))).await;
    }
}

/// Bouw + post het weekly-embed voor het venster van zaterdag-15:00 `sat` (venster = de week
/// ervóór). Geen bericht als niemand deze week iets verdiende.
async fn post_weekly_leaderboard(http: &Arc<serenity::Http>, pool: &DbPool, sat: f64) {
    {
        // Venster = de net afgelopen week: sinds de vorige zaterdag 15:00.
        let since = sat - 7.0 * 86400.0;
        let top = db::leaderboard_week(pool, since, 20);
        if top.is_empty() {
            return;
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
                "Top earners of the past week!\n\n{lines}\n🎉 **Top Three claim your prize below!** <:MM_party:1522596802874835014>"
            ))
            .colour(0x6B_9B_52);
        // ÉÉN "🎁 Claim your reward"-knop voor de top 3 (300/200/100 naar plaats 1/2/3).
        // De 3 cadeau-rijen (level_gifts kind='weekly') zitten in de custom_id "wg:g1,g2,g3";
        // de handler kiest bij een klik het cadeau van de klikker. Discord kan de knop niet
        // per-gebruiker grijzen — iedereen ziet 'm groen; nr 4+ krijgt de ephemeral.
        let now2 = now_secs();
        let mut gids: Vec<String> = Vec::new();
        for (rank, amount) in [(0usize, 300i64), (1, 200), (2, 100)] {
            if let Some((uid, _n, _t)) = top.get(rank) {
                let gid = db::create_level_gift(pool, uid, amount, 0, "weekly", now2);
                gids.push(gid.to_string());
            }
        }
        // Tag de top 3 in de CONTENT (mentions in een embed pingen niet — in de content wel).
        let pings: String = top
            .iter()
            .take(3)
            .map(|(uid, _n, _t)| format!("<@{uid}>"))
            .collect::<Vec<_>>()
            .join(" ");
        let mut msg = serenity::CreateMessage::new().content(pings).embed(embed);
        if !gids.is_empty() {
            let btn = serenity::CreateButton::new(format!("wg:{}", gids.join(",")))
                .emoji('🎁')
                .label("Claim your reward")
                .style(serenity::ButtonStyle::Success);
            msg = msg.components(vec![serenity::CreateActionRow::Buttons(vec![btn])]);
        }
        // In prod #coins (user-keuze 2026-07-18) — zelfde kanaal als de claim-melding.
        if PROD_COINS_CHANNEL_ID != 0 {
            let _ = serenity::ChannelId::new(PROD_COINS_CHANNEL_ID)
                .send_message(http.as_ref(), msg)
                .await;
        }
    }
}

/// Klik op de ene "🎁 Claim your reward"-knop van de weekly. De custom_id "wg:g1,g2,g3" bevat
/// de 3 cadeau-rijen (top 3). We proberen elk cadeau te claimen voor de klikker: enkel de
/// eigenaar (plaats 1/2/3) krijgt zijn bedrag (300/200/100) → publiek regeltje in #coins.
/// Al geclaimd → stil. Niet in de top 3 (bv. plaats 4+) → ephemeral "This is not your prize.".
async fn handle_weekly_claim(
    ctx: &serenity::Context,
    mc: &serenity::ComponentInteraction,
    data: &Data,
) -> Result<(), Error> {
    let gids: Vec<i64> = mc
        .data
        .custom_id
        .strip_prefix("wg:")
        .unwrap_or("")
        .split(',')
        .filter_map(|s| s.parse::<i64>().ok())
        .collect();
    let uid = mc.user.id.to_string();
    let name = mc
        .user
        .global_name
        .clone()
        .unwrap_or_else(|| mc.user.name.clone());
    // Zoek onder de 3 cadeaus dat van de klikker. claim_level_gift geeft NotYours voor een
    // cadeau dat niet van hem is (zonder iets te wijzigen), Granted voor zíjn openstaande
    // cadeau (claimt het), AlreadyClaimed als hij het al ophaalde.
    let mut granted: Option<i64> = None;
    let mut owns_but_claimed = false;
    for &gid in &gids {
        match db::claim_level_gift(&data.pool, gid, &uid, &name, now_secs()) {
            db::GiftClaim::Granted(a) => {
                granted = Some(a);
                break;
            }
            db::GiftClaim::AlreadyClaimed => owns_but_claimed = true,
            db::GiftClaim::NotYours | db::GiftClaim::NotFound => {}
        }
    }
    if let Some(amount) = granted {
        let _ = mc
            .create_response(&ctx.http, serenity::CreateInteractionResponse::Acknowledge)
            .await;
        // Alle top-3 geclaimd? → knop uitschakelen/grijs, duidelijk dat er niks meer te halen valt.
        if !gids.is_empty() && gids.iter().all(|&g| db::gift_claimed(&data.pool, g)) {
            let ids = gids.iter().map(|g| g.to_string()).collect::<Vec<_>>().join(",");
            let done = serenity::CreateButton::new(format!("wg:{ids}"))
                .emoji('🎁')
                .label("Claim your reward")
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
        }
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
        let lvl_ch = levelup_target(ctx, data, mc.channel_id).await;
        maybe_levelup(&ctx.http, &data.pool, &uid, &name, lvl_ch).await;
    } else if owns_but_claimed {
        // Winnaar die al claimde → stil acken, geen bericht.
        let _ = mc
            .create_response(&ctx.http, serenity::CreateInteractionResponse::Acknowledge)
            .await;
    } else {
        // Geen cadeau voor deze klikker (niet in de top 3) → ephemeral.
        respond_ephemeral(ctx, mc, "This is not your prize.").await?;
    }
    Ok(())
}

pub async fn run(pool: DbPool, cfg: Config) -> Result<(), Error> {
    let token = cfg.bot_token.clone();
    let intents = serenity::GatewayIntents::GUILDS
        | serenity::GatewayIntents::GUILD_MESSAGES
        | serenity::GatewayIntents::MESSAGE_CONTENT
        | serenity::GatewayIntents::GUILD_MEMBERS
        // Reacties tellen mee als activiteit voor Manage → Inactives (niet-privileged intent).
        | serenity::GatewayIntents::GUILD_MESSAGE_REACTIONS;

    // Verlopen tijdelijke rollen periodiek intrekken.
    tokio::spawn(role_grant_sweeper(pool.clone(), cfg.clone()));

    // Enkel !chest (dev-only info-commando). Het !coins-leaderboard is verwijderd.
    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![
                chest(),
                chestodds(),
                chestrescue(),
                test(),
                birthdaytest(),
                birthdaytestlive(),
                partytest(),
                partytestlive(),
                threadfix_preview(),
                threadfix_commit(),
                threadfix_reset(),
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
            let bday_pool = pool.clone();
            let bday_http = ctx.http.clone();
            let party_pool = pool.clone();
            let party_http = ctx.http.clone();
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
                // Vangnet voor verjaardagen: elk kwartier de Birthday-rol nalezen.
                tokio::spawn(birthday_sweeper(bday_http, bday_pool));
                // Feestjes sluiten na hun looptijd (knop grijs + uit de pins).
                tokio::spawn(party_closer(party_http, party_pool));

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

                Ok(Data { pool, cfg, chest, parent_cache: Arc::new(Mutex::new(HashMap::new())) })
            })
        })
        .build();

    let mut client = serenity::ClientBuilder::new(&token, intents)
        .framework(framework)
        .await?;
    client.start().await?;
    Ok(())
}

/// Het odds-bewijs van de Lucky Horseshoe. Bezit = gewicht 2 i.p.v. 1 bij de
/// chest-trekking; de vraag is of dat écht "dubbele kans" oplevert. Dit werd eerder
/// met 500k willekeurige trekkingen bemonsterd — hier **uitputtend**: elke mogelijke
/// roll ∈ [0, Σweights) precies één keer, dus de verdeling is exact, niet statistisch.
#[cfg(test)]
mod horseshoe_odds {
    use super::pick_weighted;

    /// Tel per index hoeveel van alle mogelijke rolls daar terechtkomen.
    fn tally(weights: &[u32]) -> Vec<u32> {
        let total: u32 = weights.iter().sum();
        let mut hits = vec![0u32; weights.len()];
        for roll in 0..total {
            hits[pick_weighted(weights, roll)] += 1;
        }
        hits
    }

    #[test]
    fn houder_wint_exact_dubbel_zo_vaak() {
        // Eén houder tussen N-1 gewone spelers, voor 2 t/m 6 deelnemers.
        for spelers in 2..=6 {
            let mut w = vec![1u32; spelers];
            w[0] = 2; // de horseshoe-houder
            let hits = tally(&w);
            assert_eq!(hits.iter().sum::<u32>(), spelers as u32 + 1, "elke roll telt exact 1×");
            for (i, h) in hits.iter().enumerate().skip(1) {
                assert_eq!(hits[0], h * 2, "houder vs speler {i} bij {spelers} deelnemers");
            }
            // Concreet: bij 2 spelers is dat 2/3 vs 1/3, bij 3 spelers 2/4 vs 1/4, …
            assert_eq!(hits[0], 2);
        }
    }

    #[test]
    fn zonder_houder_is_iedereen_gelijk() {
        let hits = tally(&[1, 1, 1, 1]);
        assert_eq!(hits, vec![1, 1, 1, 1]);
    }

    #[test]
    fn meerdere_houders_blijven_onderling_gelijk() {
        // 2 houders + 2 gewone spelers: 2/6 elk vs 1/6 elk.
        let hits = tally(&[2, 2, 1, 1]);
        assert_eq!(hits, vec![2, 2, 1, 1]);
    }

    #[test]
    fn randgevallen_paniekeren_niet() {
        assert_eq!(pick_weighted(&[], 0), 0, "lege lijst valt terug op 0");
        assert_eq!(pick_weighted(&[1, 1], 99), 0, "roll buiten bereik valt terug op 0");
        assert_eq!(pick_weighted(&[2], 1), 0, "enige deelnemer wint altijd");
    }
}
