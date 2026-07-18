//! Admin-instelbare economieparameters: de type-kennis, de defaults en de
//! grenzen. De opslag zit in `db.rs` (tabel `settings`), de GUI in `web.rs`
//! (Manage → ⚙ Settings), en de waarden worden LIVE gelezen — draaien aan een
//! getal geldt bij het eerstvolgende bericht/chest, zonder deploy of herstart.
//!
//! **De unit zit in de sleutelnaam** (`_sec` / `_min` / `_hours` / `_coins` /
//! `_days`). Zo staat een eenheidsfout zichtbaar op de call-site
//! (`settings::f64_of(pool, "chest_window_min") * 60.0`) in plaats van verstopt
//! in een conversie hier.
//!
//! Twee dingen wonen bewust NIET hier, want het zijn lijsten en geen losse
//! waarden: de coin-uitkomsten per bericht (`db::coin_weights_all`) en de
//! chest-prijsverdeling (`db::chest_tiers_all`). Beide hebben een eigen tabel
//! met **relatieve** gewichten.
use crate::db::{self, DbPool};

#[derive(PartialEq, Clone, Copy)]
pub enum Kind {
    /// Vinkje in de GUI; opgeslagen als "0" / "1".
    Bool,
    /// Heel getal (spinner met stap 1).
    Int,
}

pub struct Spec {
    pub key: &'static str,
    pub label: &'static str,
    /// Kopje waaronder dit veld in de GUI staat.
    pub group: &'static str,
    pub kind: Kind,
    pub default: f64,
    pub min: f64,
    pub max: f64,
    /// Eén zin onder het veld: wat het getal dóét.
    pub help: &'static str,
}

pub const COINS: &str = "Coins per bericht";
pub const DAILY: &str = "Daily-beloning";
pub const CHEST: &str = "Treasure chest";
pub const SHOP: &str = "Shop";

/// Elke instelbare parameter, in weergavevolgorde. De defaults zijn de waarden
/// die vóór deze refactor als `const` in `bot.rs` stonden — een lege DB gedraagt
/// zich dus exact zoals de bot van vóór de Settings-tab.
pub const SPECS: &[Spec] = &[
    Spec {
        key: "msg_cooldown_sec",
        label: "Cooldown",
        group: COINS,
        kind: Kind::Int,
        default: 30.0,
        min: 0.0,
        max: 3600.0,
        help: "Minimale tijd tussen twee coin-toekenningen per lid. Berichten binnen de cooldown leveren niets op.",
    },
    Spec {
        key: "daily_cooldown_hours",
        label: "Claim-cooldown",
        group: DAILY,
        kind: Kind::Int,
        default: 20.0,
        min: 1.0,
        max: 168.0,
        help: "Minimum tussen twee claims. Staat lager dan 24u zodat je niet elke dag een uur later moet klikken.",
    },
    Spec {
        key: "daily_streak_window_hours",
        label: "Streak-venster",
        group: DAILY,
        kind: Kind::Int,
        default: 30.0,
        min: 1.0,
        max: 336.0,
        help: "Opnieuw klikken binnen dit venster telt als de volgende streakdag. Erbuiten valt de streak terug naar dag 1.",
    },
    Spec {
        key: "daily_base_min_coins",
        label: "Dag 1 — ondergrens",
        group: DAILY,
        kind: Kind::Int,
        default: 10.0,
        min: 0.0,
        max: 1_000_000.0,
        help: "De eerste claim is een willekeurig bedrag tussen deze ondergrens en de bovengrens.",
    },
    Spec {
        key: "daily_base_max_coins",
        label: "Dag 1 — bovengrens",
        group: DAILY,
        kind: Kind::Int,
        default: 100.0,
        min: 0.0,
        max: 1_000_000.0,
        help: "Ligt deze onder de ondergrens, dan wint de ondergrens (geen leeg bereik).",
    },
    Spec {
        key: "daily_min_step_coins",
        label: "Stijging ondergrens/dag",
        group: DAILY,
        kind: Kind::Int,
        default: 1.0,
        min: 0.0,
        max: 10_000.0,
        help: "Hoeveel de ondergrens omhooggaat per opeenvolgende streakdag.",
    },
    Spec {
        key: "daily_max_step_coins",
        label: "Stijging bovengrens/dag",
        group: DAILY,
        kind: Kind::Int,
        default: 5.0,
        min: 0.0,
        max: 10_000.0,
        help: "Hoeveel de bovengrens omhooggaat per opeenvolgende streakdag.",
    },
    Spec {
        key: "daily_streak_cap_days",
        label: "Streak-plafond",
        group: DAILY,
        kind: Kind::Int,
        default: 200.0,
        min: 1.0,
        max: 100_000.0,
        help: "Na zoveel dagen stopt de stijging. De streak zelf blijft wel doorlopen.",
    },
    Spec {
        key: "chest_enabled",
        label: "Chests aan",
        group: CHEST,
        kind: Kind::Bool,
        default: 1.0,
        min: 0.0,
        max: 1.0,
        help: "Uit = er spawnen geen nieuwe chests meer. Een chest die nu openstaat popt nog gewoon.",
    },
    Spec {
        key: "chest_distinct_users",
        label: "Chatters nodig",
        group: CHEST,
        kind: Kind::Int,
        default: 3.0,
        min: 1.0,
        max: 100.0,
        help: "Zoveel verschillende mensen moeten binnen het venster chatten voor een chest verschijnt.",
    },
    Spec {
        key: "chest_window_min",
        label: "Telvenster",
        group: CHEST,
        kind: Kind::Int,
        default: 10.0,
        min: 1.0,
        max: 1440.0,
        help: "Het venster waarbinnen die chatters geteld worden. Ruimer = chests spawnen makkelijker.",
    },
    Spec {
        key: "chest_pop_delay_min",
        label: "Open-tijd",
        group: CHEST,
        kind: Kind::Int,
        default: 10.0,
        min: 1.0,
        max: 1440.0,
        help: "Hoelang een chest openstaat voor hij popt. De aftel-tekst in de embed leest dit mee.",
    },
    Spec {
        key: "chest_channel_cooldown_min",
        label: "Kanaalrust",
        group: CHEST,
        kind: Kind::Int,
        default: 60.0,
        min: 0.0,
        max: 10_080.0,
        help: "Rust per kanaal na een chest — anti-spam. Op 0 kan er meteen een nieuwe komen.",
    },
    Spec {
        key: "chest_min_joiners",
        label: "Deelnemers nodig",
        group: CHEST,
        kind: Kind::Int,
        default: 2.0,
        min: 1.0,
        max: 100.0,
        help: "Minder klikkers dan dit → de chest despawnt en er wordt niets uitbetaald.",
    },
    Spec {
        key: "horseshoe_shop_odds_days",
        label: "Horseshoe shop-kans",
        group: SHOP,
        kind: Kind::Int,
        default: 14.0,
        min: 0.0,
        max: 365.0,
        help: "De Lucky Horseshoe verschijnt gemiddeld 1 keer per zoveel dagen in de dagshop (1-op-N-kans per dag). Hoger = zeldzamer. 0 = UIT: geen boosters in de dagshop (dan enkel gems).",
    },
];

pub fn spec(key: &str) -> Option<&'static Spec> {
    SPECS.iter().find(|s| s.key == key)
}

/// De waarde van een setting, geklemd binnen de grenzen van zijn spec.
/// Niet gezet, onleesbaar of buiten bereik → de default. Een onbekende sleutel
/// is een programmeerfout en paniekt (de SPECS-lijst is de waarheid).
pub fn f64_of(pool: &DbPool, key: &str) -> f64 {
    let sp = spec(key).unwrap_or_else(|| panic!("onbekende setting: {key}"));
    db::setting_get(pool, key)
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| v.is_finite())
        .map(|v| v.clamp(sp.min, sp.max))
        .unwrap_or(sp.default)
}

pub fn i64_of(pool: &DbPool, key: &str) -> i64 {
    f64_of(pool, key).round() as i64
}

pub fn usize_of(pool: &DbPool, key: &str) -> usize {
    i64_of(pool, key).max(0) as usize
}

pub fn bool_of(pool: &DbPool, key: &str) -> bool {
    f64_of(pool, key) != 0.0
}

/// Schrijf een waarde weg, geklemd binnen de spec. Geeft `false` bij een
/// onbekende sleutel of onleesbare invoer — de GUI toont dan een foutbanner
/// i.p.v. stil een default te bewaren.
pub fn set(pool: &DbPool, key: &str, raw: &str) -> bool {
    let Some(sp) = spec(key) else { return false };
    // Vinkjes komen als "on"/"1"/"true" (of ontbreken helemaal → "0").
    let parsed = match sp.kind {
        Kind::Bool => Some(match raw.trim() {
            "on" | "1" | "true" | "yes" => 1.0,
            _ => 0.0,
        }),
        // Komma als decimaalteken: op een Belgisch toetsenbord typ je 0,5.
        Kind::Int => raw.trim().replace(',', ".").parse::<f64>().ok().filter(|v| v.is_finite()),
    };
    let Some(v) = parsed else { return false };
    let v = v.clamp(sp.min, sp.max);
    let out = if sp.kind == Kind::Bool || v.fract() == 0.0 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v}")
    };
    db::setting_set(pool, key, &out);
    true
}
