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
    /// Vrije tekst (getrimd opgeslagen, geen grenzen). Lezen met `str_of` —
    /// `f64_of` paniekt bewust op een tekst-sleutel. Een sleutel die op `_text`
    /// eindigt krijgt in de GUI een meerregelig veld i.p.v. één regel.
    Text,
    /// Keuze uit een lijst die pas op de pagina zelf bekend is (nu: de
    /// channel-points-rewards van het kanaal). Opgeslagen als vrije tekst — de
    /// **id** van het gekozene — en dus ook met `str_of` te lezen. In de GUI een
    /// keuzelijst, want zo'n id staat nergens in het Twitch-dashboard: die valt
    /// niet over te typen, enkel te kiezen. Leeg = niets gekozen.
    Choice,
}

pub struct Spec {
    pub key: &'static str,
    pub label: &'static str,
    /// Kopje waaronder dit veld in de GUI staat.
    pub group: &'static str,
    pub kind: Kind,
    /// Getal-default (Bool/Int). Bij `Kind::Text` niet gebruikt — zie `text_default`.
    pub default: f64,
    pub min: f64,
    pub max: f64,
    /// Startwaarde van een tekstveld, en enkel te vullen met tekst die de **user** zelf
    /// gaf — wij verzinnen geen speler-zichtbare zinnen. Leeg = het veld begint leeg.
    /// NB: dit geldt alleen zolang de sleutel nooit bewaard is. Wist Faybelle het veld,
    /// dan blijft het leeg (= bericht uit); de startwaarde springt niet terug.
    pub text_default: &'static str,
    /// Eén zin onder het veld: wat het getal dóét.
    pub help: &'static str,
}

pub const COINS: &str = "Coins per bericht";
pub const DAILY: &str = "Daily-beloning";
pub const CHEST: &str = "Treasure chest";
pub const TWITCH: &str = "Twitch-redeem → Hytale-pas";
// (Er was ook een groep "Shop"; die staat leeg sinds de shoprotatie per item geregeld
// wordt in Manage → Shop i.p.v. met een instelling hier.)

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
        text_default: "",
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
        text_default: "",
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
        text_default: "",
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
        text_default: "",
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
        text_default: "",
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
        text_default: "",
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
        text_default: "",
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
        text_default: "",
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
        text_default: "",
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
        text_default: "",
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
        text_default: "",
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
        text_default: "",
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
        text_default: "",
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
        text_default: "",
        min: 1.0,
        max: 100.0,
        help: "Minder klikkers dan dit → de chest despawnt en er wordt niets uitbetaald.",
    },
    // NB: de zeldzaamheid van de Lucky Horseshoe stond hier ooit als één instelling
    // (`horseshoe_shop_odds_days`, 1-op-N per dag). Vervangen door een gewicht **per item**
    // in Manage → Shop, want dat geldt voor élk shopitem en niet enkel voor de booster.
    //
    // --- Twitch ------------------------------------------------------------------
    // De streamer maakt de channel-points-reward ZELF aan in haar Twitch-dashboard
    // (met "kijker moet tekst invullen" aan). Market maakt of beheert géén rewards
    // meer; het herkent de juiste redeem aan haar **id**.
    //
    // ⚠️ Dit was ooit de titel, en dat brak op 2026-08-04 stil: de reward kreeg een
    // emoji vooraan ('Meadowland Pass' → '🎫Meadowland Pass') en elke redeem viel
    // vanaf dan in de "niet van ons"-tak. Vier redeems, geen pas, geen bericht. De id
    // van een reward verandert nooit, ook niet bij hernoemen — vandaar de omslag.
    Spec {
        key: "twitch_reward_id",
        label: "Reward (tijdelijke pas)",
        group: TWITCH,
        kind: Kind::Choice,
        default: 0.0,
        text_default: "",
        min: 0.0,
        max: 0.0,
        help: "De channel-points-reward die een pas geeft. De lijst komt rechtstreeks van het kanaal; hernoemen in Twitch verandert hier niets. Niets gekozen = market doet niets met redeems.",
    },
    Spec {
        key: "twitch_pass_hours",
        label: "Duur van de pas",
        group: TWITCH,
        kind: Kind::Int,
        default: 2.0,
        text_default: "",
        min: 1.0,
        max: 8760.0,
        help: "Hoelang een kijker op de server mag na één redeem. Twee keer redeemen stapelt de tijd op.",
    },
    Spec {
        key: "twitch_whisper_text",
        label: "Whisper naar de kijker",
        group: TWITCH,
        kind: Kind::Text,
        default: 0.0,
        text_default: "",
        min: 0.0,
        max: 0.0,
        help: "Privébericht na een geslaagde redeem. Gebruik {uren} en {naam}; zet hier ook het server-adres in. Leeg = geen bericht.",
    },
    // Tweede redeem met een àndere Hytale-naam: er wordt niets toegekend (de naam ligt na de
    // eerste keer vast) en de kijker krijgt dit bericht. De tekst is letterlijk van de user;
    // Faybelle past ze hier aan zonder deploy. Leeg = geen bericht — de weigering blijft.
    Spec {
        key: "twitch_mismatch_whisper_text",
        label: "Whisper bij een afwijkende naam",
        group: TWITCH,
        kind: Kind::Text,
        default: 0.0,
        text_default: "Oh oh, you filled in a different Hytale name. The time is not granted. \
                       Contact Faybelle to get your points back.",
        min: 0.0,
        max: 0.0,
        help: "Privébericht als de kijker bij een volgende redeem een andere Hytale-naam invult dan de naam die al aan zijn Twitch-account vastzit. Er wordt dan géén tijd toegekend. {naam} = de vastgezette naam. Leeg = geen bericht.",
    },
    Spec {
        key: "twitch_perma_reward_id",
        label: "Reward (permanente pas)",
        group: TWITCH,
        kind: Kind::Choice,
        default: 0.0,
        text_default: "",
        min: 0.0,
        max: 0.0,
        help: "Optionele tweede reward die permanente toegang geeft. Niets gekozen = die redeem bestaat niet.",
    },
    Spec {
        key: "twitch_perma_whisper_text",
        label: "Whisper (permanente pas)",
        group: TWITCH,
        kind: Kind::Text,
        default: 0.0,
        text_default: "",
        min: 0.0,
        max: 0.0,
        help: "Privébericht na een geslaagde permanente redeem. Gebruik {naam}. Leeg = geen bericht.",
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
    assert!(
        !matches!(sp.kind, Kind::Text | Kind::Choice),
        "setting {key} is tekst — lees ze met str_of"
    );
    db::setting_get(pool, key)
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| v.is_finite())
        .map(|v| clamp_bounds(v, sp.min, sp.max))
        .unwrap_or(sp.default)
}

/// Klem `v` binnen [min,max]. Verdraagt een omgekeerde spec (min>max) zonder te paniceren
/// (std `f64::clamp` paniekt als min>max) door de grenzen te ordenen.
fn clamp_bounds(v: f64, min: f64, max: f64) -> f64 {
    let (lo, hi) = if min <= max { (min, max) } else { (max, min) };
    v.clamp(lo, hi)
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

/// De waarde van een `Kind::Text`- of `Kind::Choice`-setting, getrimd. Nooit gezet ⇒
/// leeg — en leeg betekent bij elk van die velden "uit", nooit een verzonnen default.
pub fn str_of(pool: &DbPool, key: &str) -> String {
    let sp = spec(key).unwrap_or_else(|| panic!("onbekende setting: {key}"));
    assert!(matches!(sp.kind, Kind::Text | Kind::Choice), "setting {key} is geen tekst");
    match db::setting_get(pool, key) {
        Some(v) => v.trim().to_string(),
        None => sp.text_default.to_string(),
    }
}

/// Schrijf een waarde weg, geklemd binnen de spec. Geeft `false` bij een
/// onbekende sleutel of onleesbare invoer — de GUI toont dan een foutbanner
/// i.p.v. stil een default te bewaren.
pub fn set(pool: &DbPool, key: &str, raw: &str) -> bool {
    let Some(sp) = spec(key) else { return false };
    // Tekst gaat er getrimd in zoals ze getypt is — geen grenzen, en leeg mag
    // (dat is bij deze velden juist de manier om ze uit te zetten). Een keuze is
    // opgeslagen tekst (de id van het gekozene) en volgt exact dezelfde weg.
    if matches!(sp.kind, Kind::Text | Kind::Choice) {
        db::setting_set(pool, key, raw.trim());
        return true;
    }
    // Vinkjes komen als "on"/"1"/"true" (of ontbreken helemaal → "0").
    let parsed = match sp.kind {
        Kind::Bool => Some(match raw.trim() {
            "on" | "1" | "true" | "yes" => 1.0,
            _ => 0.0,
        }),
        // Komma als decimaalteken: op een Belgisch toetsenbord typ je 0,5.
        Kind::Int => raw.trim().replace(',', ".").parse::<f64>().ok().filter(|v| v.is_finite()),
        Kind::Text | Kind::Choice => unreachable!("tekst is hierboven al afgehandeld"),
    };
    let Some(v) = parsed else { return false };
    let v = clamp_bounds(v, sp.min, sp.max);
    let out = if sp.kind == Kind::Bool || v.fract() == 0.0 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v}")
    };
    db::setting_set(pool, key, &out);
    true
}

#[cfg(test)]
mod clamp_tests {
    use super::clamp_bounds;

    /// clamp_bounds klemt normaal én verdraagt een omgekeerde spec (min>max) zonder panic.
    #[test]
    fn clamp_bounds_handles_reversed_spec() {
        assert_eq!(clamp_bounds(5.0, 0.0, 10.0), 5.0);
        assert_eq!(clamp_bounds(-1.0, 0.0, 10.0), 0.0);
        assert_eq!(clamp_bounds(99.0, 0.0, 10.0), 10.0);
        // Omgekeerd (min>max): std clamp zou paniceren — clamp_bounds ordent en klemt gewoon.
        assert_eq!(clamp_bounds(5.0, 10.0, 0.0), 5.0);
        assert_eq!(clamp_bounds(99.0, 10.0, 0.0), 10.0);
        assert_eq!(clamp_bounds(-5.0, 10.0, 0.0), 0.0);
    }
}

/// Startwaarde van een tekstveld: alleen zolang de sleutel nooit bewaard is.
#[cfg(test)]
mod text_default_tests {
    use super::*;

    #[test]
    fn startwaarde_geldt_tot_faybelle_het_veld_zelf_bewaart() {
        let p = std::env::temp_dir().join(format!("market-td-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&p);
        let pool = db::init_pool(p.to_str().unwrap());

        // Nooit gezet ⇒ de tekst die de user aanleverde.
        let start = str_of(&pool, "twitch_mismatch_whisper_text");
        assert!(start.starts_with("Oh oh, you filled in a different Hytale name."));
        assert!(start.ends_with("Contact Faybelle to get your points back."));

        // Aangepast ⇒ die tekst wint.
        assert!(set(&pool, "twitch_mismatch_whisper_text", "  Iets anders  "));
        assert_eq!(str_of(&pool, "twitch_mismatch_whisper_text"), "Iets anders");

        // Leeggemaakt ⇒ blijft leeg (= bericht uit). De startwaarde mag NIET terugspringen,
        // anders valt zo'n bericht nooit uit te zetten.
        assert!(set(&pool, "twitch_mismatch_whisper_text", ""));
        assert_eq!(str_of(&pool, "twitch_mismatch_whisper_text"), "");

        // De velden waar de user nog geen tekst voor gaf, beginnen wél leeg.
        assert_eq!(str_of(&pool, "twitch_whisper_text"), "");

        let _ = std::fs::remove_file(p);
    }
}
