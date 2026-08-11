//! Hoeveel speeltijd heeft een pas nog? — het antwoord komt van de **tale-kant**.
//!
//! Sinds 2026-08-04 is een pas een tegoed aan speeltijd: hij loopt enkel leeg terwijl de
//! speler in-game is. Alleen de server weet wie er op dat moment speelt, dus daar wordt die
//! klok bijgehouden. De tale-bot schrijft het resultaat weg in `passes.json`:
//!
//! ```json
//! {"version": 2, "passes": {"Waldstein": {"granted": 3638.0, "used": 0.0, "remaining": 3638.0}}}
//! ```
//!
//! Market is de winkel, niet de klok. Het verkoopt tijd (en stapelt die op `expires`, waar de
//! bot de toekenning uit afleidt) en toont hier enkel wat de tale-kant zegt dat er over is.
//! Eén schrijver per teller, dus geen twee boekhoudingen die uit elkaar lopen.
//!
//! **Online of niet?** Dat staat niet in het bestand, maar valt eraan af te lezen: `used`
//! loopt enkel op terwijl iemand speelt. Zien we die teller stijgen, dan is hij binnen; blijft
//! ze staan, dan is de pas op pauze. Vandaar dat we periodiek bemonsteren in plaats van enkel
//! bij een paginabezoek — anders zou een speler die nooit z'n inventaris opent, nooit als
//! online gelden.
//!
//! **Faalt veilig.** Is het bestand er niet of niet leesbaar, dan geeft `lookup` niets terug
//! en valt de site terug op de oude weergave (aftellen op `expires`). Market schrijft nooit
//! in dit bestand.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Standaardpad op de VPS; te overschrijven met `MARKET_PASSES_JSON` (voor tests).
const DEFAULT_PATH: &str = "/opt/hytale/passes.json";
/// Hoe vaak we het bestand opnieuw inlezen. Klein bestand, maar een paginabezoek mag er
/// nooit op wachten — dus lezen we op de achtergrond en bedienen we uit het geheugen.
const SAMPLE_EVERY: Duration = Duration::from_secs(20);
/// Zolang na de laatste stijging van `used` gelden we iemand nog als online. Ruim boven de
/// bemonsteringsstap, zodat een speler niet knippert tussen online en gepauzeerd.
const ONLINE_GRACE: Duration = Duration::from_secs(90);

/// Wat market van één pas moet weten om hem te tonen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ledger {
    /// Resterende speeltijd in seconden.
    pub remaining: f64,
    /// Speelt hij nu? (afgeleid uit een stijgende `used`-teller)
    pub online: bool,
    /// Is dit **testtijd**? Tijdens de testfase houdt de tale-kant een apart tegoed bij
    /// (`"kind": "test"`), en staat het gewone tegoed stil. Market gebruikt dit om een
    /// tweede testpas te weigeren zolang de eerste nog loopt. Ontbreekt het veld (oudere
    /// tale-bot), dan is het `false` en verandert er hier niets.
    pub test: bool,
}

#[derive(Default)]
struct State {
    /// Naam (kleine letters) → resterende tijd + laatst geziene `used`.
    passes: HashMap<String, Entry>,
    /// Is het bestand ooit met succes gelezen? Zo niet, dan valt de site terug op `expires`.
    have_data: bool,
}

#[derive(Clone, Copy)]
struct Entry {
    remaining: f64,
    used: f64,
    /// `kind == "test"`: de tale-kant telt nu testtijd af (zie `Ledger::test`).
    test: bool,
    /// Wanneer `used` voor het laatst stéég — het bewijs dat iemand aan het spelen is.
    last_rise: Option<Instant>,
}

fn state() -> &'static Mutex<State> {
    static S: OnceLock<Mutex<State>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(State::default()))
}

/// De pas van deze speler, zoals de tale-kant hem kent. None = geen gegevens (bestand
/// onleesbaar, of deze naam staat er niet in).
pub fn lookup(hytale_name: &str) -> Option<Ledger> {
    let st = state().lock().ok()?;
    if !st.have_data {
        return None;
    }
    let e = st.passes.get(&hytale_name.to_lowercase())?;
    Some(Ledger {
        remaining: e.remaining.max(0.0),
        online: e.last_rise.is_some_and(|t| t.elapsed() < ONLINE_GRACE),
        test: e.test,
    })
}

/// Lees het bestand één keer in en werk de toestand bij.
fn sample(path: &str) -> Result<usize, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let passes = v.get("passes").and_then(|p| p.as_object()).ok_or("geen passes-object")?;

    let mut st = state().lock().map_err(|_| "state vergrendeld")?;
    let mut fresh: HashMap<String, Entry> = HashMap::new();
    for (name, p) in passes {
        let used = p.get("used").and_then(|u| u.as_f64()).unwrap_or(0.0);
        let remaining = p
            .get("remaining")
            .and_then(|r| r.as_f64())
            // Ouder formaat of half ingevuld: dan zelf uitrekenen.
            .unwrap_or_else(|| p.get("granted").and_then(|g| g.as_f64()).unwrap_or(0.0) - used);
        let test = p.get("kind").and_then(|k| k.as_str()) == Some("test");
        let key = name.to_lowercase();
        let prev = st.passes.get(&key);
        let last_rise = match prev {
            // `used` steeg ⇒ die speler is nu aan het spelen.
            Some(old) if used > old.used + 0.001 => Some(Instant::now()),
            Some(old) => old.last_rise,
            None => None,
        };
        fresh.insert(key, Entry { remaining, used, last_rise, test });
    }
    st.passes = fresh;
    st.have_data = true;
    Ok(st.passes.len())
}

/// Achtergrondtaak: houdt de gegevens vers.
pub async fn run() {
    let path = std::env::var("MARKET_PASSES_JSON").unwrap_or_else(|_| DEFAULT_PATH.to_string());
    // Eén keer luid zeggen of dit werkt: zonder leesrecht is de pas-teller op de site de
    // oude wandklok, en dat wil je weten vóór iemand zich afvraagt waarom de tijd niet klopt.
    match sample(&path) {
        Ok(n) => tracing::info!("Pas-grootboek: {path} gelezen — {n} pas(sen) van de tale-kant"),
        Err(e) => tracing::warn!(
            "Pas-grootboek NIET leesbaar ({path}: {e}) — de site valt terug op aftellen \
             op de aankoopdatum. Market heeft leesrecht op dat bestand nodig."
        ),
    }
    loop {
        tokio::time::sleep(SAMPLE_EVERY).await;
        if let Err(e) = sample(&path) {
            tracing::debug!("pas-grootboek lezen faalde: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &std::path::Path, body: &str) {
        std::fs::write(path, body).unwrap();
    }

    /// Het formaat dat de tale-bot schrijft, en de afleiding van "speelt nu".
    #[test]
    fn leest_het_grootboek_en_ziet_wie_speelt() {
        let p = std::env::temp_dir().join(format!("passes-{}.json", std::process::id()));
        write(
            &p,
            r#"{"version":2,"passes":{"Waldstein":{"granted":7200.0,"used":0.0,"remaining":7200.0}}}"#,
        );
        assert_eq!(sample(p.to_str().unwrap()).unwrap(), 1);

        // Eerste meting: we weten nog niet of hij speelt — pas op pauze tot het tegendeel blijkt.
        let l = lookup("Waldstein").expect("pas gevonden");
        assert_eq!(l.remaining, 7200.0);
        assert!(!l.online, "zonder stijging gaan we niet uit van online");

        // Hoofdletters mogen niet uitmaken: de naam komt uit twee bronnen.
        assert!(lookup("waldstein").is_some());
        assert!(lookup("Onbekend").is_none());

        // `used` stijgt ⇒ hij is binnen, en de resterende tijd zakt mee.
        write(
            &p,
            r#"{"version":2,"passes":{"Waldstein":{"granted":7200.0,"used":600.0,"remaining":6600.0}}}"#,
        );
        sample(p.to_str().unwrap()).unwrap();
        let l = lookup("Waldstein").expect("pas");
        assert_eq!(l.remaining, 6600.0);
        assert!(l.online, "stijgende used = aan het spelen");

        // Blijft staan ⇒ hij is weg; de teller bevriest op wat er over is.
        write(
            &p,
            r#"{"version":2,"passes":{"Waldstein":{"granted":7200.0,"used":600.0,"remaining":6600.0}}}"#,
        );
        sample(p.to_str().unwrap()).unwrap();
        let l = lookup("Waldstein").expect("pas");
        assert_eq!(l.remaining, 6600.0, "gepauzeerd verandert er niets aan het tegoed");

        let _ = std::fs::remove_file(p);
    }

    /// Testfase: de tale-kant merkt de entry als testtijd. Zonder dat merkje blijft het
    /// gewone tijd — een oudere bot schrijft het veld niet, en dan mag er niets veranderen.
    #[test]
    fn herkent_testtijd_aan_het_merkje() {
        let p = std::env::temp_dir().join(format!("passes-kind-{}.json", std::process::id()));
        write(
            &p,
            r#"{"version":2,"passes":{
                 "Tester":{"granted":900.0,"used":0.0,"remaining":900.0,"kind":"test"},
                 "Gewoon":{"granted":7200.0,"used":0.0,"remaining":7200.0,"kind":"normal"},
                 "Oud":{"granted":7200.0,"used":0.0,"remaining":7200.0}}}"#,
        );
        assert_eq!(sample(p.to_str().unwrap()).unwrap(), 3);
        assert!(lookup("Tester").unwrap().test, "kind=test ⇒ testtijd");
        assert!(!lookup("Gewoon").unwrap().test);
        assert!(!lookup("Oud").unwrap().test, "veld ontbreekt ⇒ gewone tijd, geen slot");

        // Testtijd opgebruikt: het merkje blijft, maar er loopt niets meer — dán mag er
        // weer een testpas gekocht worden.
        write(
            &p,
            r#"{"version":2,"passes":{"Tester":{"granted":900.0,"used":900.0,"remaining":0.0,"kind":"test"}}}"#,
        );
        sample(p.to_str().unwrap()).unwrap();
        let l = lookup("Tester").unwrap();
        assert!(l.test && l.remaining == 0.0);

        let _ = std::fs::remove_file(p);
    }

    /// Een onleesbaar of kapot bestand mag nooit een verkeerde tijd tonen — dan liever niets.
    #[test]
    fn kapot_of_afwezig_bestand_geeft_niets() {
        assert!(sample("/bestaat/echt/niet.json").is_err());
        let p = std::env::temp_dir().join(format!("passes-bad-{}.json", std::process::id()));
        write(&p, "{geen json");
        assert!(sample(p.to_str().unwrap()).is_err());
        write(&p, r#"{"version":2}"#);
        assert!(sample(p.to_str().unwrap()).is_err(), "zonder passes-object: geen gegevens");
        let _ = std::fs::remove_file(p);
    }
}
