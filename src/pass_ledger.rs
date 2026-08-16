//! Hoeveel speeltijd heeft een pas nog? — het antwoord komt van de **tale-kant**.
//!
//! Sinds 2026-08-04 is een pas een tegoed aan speeltijd: hij loopt enkel leeg terwijl de
//! speler in-game is. Alleen de server weet wie er op dat moment speelt, dus daar wordt die
//! klok bijgehouden. De tale-bot schrijft het resultaat weg in `passes.json`:
//!
//! ```json
//! {"version": 3, "passes": {"Waldstein": {"test_remaining": 0.0, "pass_remaining": 3638.0,
//!                                          "kind": "normal", "used": 0.0}}}
//! ```
//!
//! Market is de winkel, niet de klok. Het verkoopt tijd (en stapelt die op `expires`, waar de
//! bot de toekenning uit afleidt) en toont hier enkel wat de tale-kant zegt dat er over is.
//! Eén schrijver per teller, dus geen twee boekhoudingen die uit elkaar lopen.
//!
//! **Online of niet?** Dat staat niet in `passes.json`, maar de speeltijd-teller van de
//! tale-kant (`hytale-playtime.service`) houdt het wél bij: in `playtime.json` staat onder
//! `open` wie er op dit moment in-game is (de Forgotten Temple meegerekend). Dát is onze bron.
//!
//! Tot 2026-08-14 stond hier enkel een gok: `used` loopt op terwijl iemand speelt, dus een
//! stijgende teller = binnen. Die gok blijft staan als terugval, maar hij plakte — na het
//! uitloggen gold je nog anderhalve minuut als online (`ONLINE_GRACE`) en bleef de klok op de
//! site zichtbaar doortellen tot ze bij de volgende sync terugsprong. Faybelle zag dat op haar
//! Test Pass. De echte spelerslijst kent dat probleem niet.
//!
//! **Faalt veilig.** Is het bestand er niet of niet leesbaar, dan geeft `lookup` niets terug
//! en valt de site terug op de oude weergave (aftellen op `expires`). Market schrijft nooit
//! in dit bestand.

use crate::db::{self, DbPool};
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Standaardpad op de VPS; te overschrijven met `MARKET_PASSES_JSON` (voor tests).
const DEFAULT_PATH: &str = "/opt/hytale/passes.json";
/// De spelerslijst van de speeltijd-teller; te overschrijven met `MARKET_PLAYTIME_JSON`.
const DEFAULT_PLAYTIME: &str = "/opt/hytale/playtime.json";
/// Hoe oud `playtime.json` mag zijn voor we hem niet meer vertrouwen. De teller schrijft
/// minstens elke 30s zolang de server draait (ook met een lege server), dus 3 minuten stilte
/// betekent dat er iets stuk is — dan liever terugvallen op de oude gok dan een spelerslijst
/// van een uur geleden geloven.
const PLAYTIME_STALE: Duration = Duration::from_secs(180);
/// Hoe vaak we het bestand opnieuw inlezen. Klein bestand, maar een paginabezoek mag er
/// nooit op wachten — dus lezen we op de achtergrond en bedienen we uit het geheugen.
const SAMPLE_EVERY: Duration = Duration::from_secs(20);
/// Hoe vaak we de spelerslijst opnieuw inlezen. Vaker dan het grootboek: dit bepaalt of de
/// klok op de site loopt of stilstaat, en het bestand is een paar honderd bytes.
const PLAYTIME_EVERY: Duration = Duration::from_secs(5);
/// Terugval-gok: zolang na de laatste stijging van `used` gelden we iemand nog als online.
/// Ruim boven de bemonsteringsstap, zodat een speler niet knippert tussen online en
/// gepauzeerd — maar dus ook ruim ná het uitloggen. Enkel in gebruik als `playtime.json`
/// niets bruikbaars zegt.
const ONLINE_GRACE: Duration = Duration::from_secs(90);
/// Hoeveel het tegoed minstens moet stijgen voor we het een toekenning noemen. Een tegoed
/// zakt terwijl iemand speelt; het stijgt enkel als er tijd bij komt. De drempel vangt
/// afrondingsruis van de tale-kant op, en ligt ruim onder de kortste testwaarde (60s).
const GRANT_MIN: f64 = 5.0;

/// Er is speeltijd bijgekomen op de klok van de tale-kant. Dít is het bewijs dat een
/// aankoop ook echt tijd opleverde: market stapelt enkel `expires`, de server zet dat om.
#[derive(Debug, Clone, PartialEq)]
pub struct Grant {
    /// De Hytale-naam zoals ze in `passes.json` staat.
    pub name: String,
    /// Hoeveel seconden erbij kwamen.
    pub added: f64,
    /// Wat er ná de toekenning op dat potje stond.
    pub after: f64,
    /// Wat er ná de toekenning in totaal op de klok stond (test + pas samen).
    pub total_after: f64,
    /// Testtijd (aparte pot) i.p.v. gewone pastijd.
    pub test: bool,
}

/// Wat market van één pas moet weten om hem te tonen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ledger {
    /// Resterende speeltijd in seconden, beide potjes samen (test + pas).
    pub remaining: f64,
    /// Enkel de resterende TESTtijd (uit een Test Pass). Loopt altijd eerst leeg.
    pub test_remaining: f64,
    /// Enkel de resterende gewone PAStijd. Staat stil zolang er testtijd is.
    pub pass_remaining: f64,
    /// Speelt hij nu? (afgeleid uit een stijgende `used`-teller)
    pub online: bool,
    /// Hoeveel speeltijd er in totaal al van zijn passen af is. Enkel stijgend, en enkel
    /// terwijl hij in-game is — daarmee valt te zien of een gekochte pas al opgebrand is.
    pub used: f64,
    /// Is dit **testtijd**? Tijdens de testfase houdt de tale-kant een apart tegoed bij
    /// (`"kind": "test"`), en staat het gewone tegoed stil. Sinds de testpas-regel van
    /// 2026-08-14 (één per persoon tot heractivatie) beslist market daar zelf niets meer
    /// mee; het veld blijft omdat het de stand van de tale-kant beschrijft. Was: om een
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
    /// Wie er volgens `playtime.json` nu in-game is (namen in kleine letters).
    online_now: HashSet<String>,
    /// Wanneer we die lijst voor het laatst uit een VERS bestand haalden. `None` = geen
    /// bruikbare spelerslijst; dan telt de oude gok (stijgende `used`).
    online_read: Option<Instant>,
}

#[derive(Clone, Copy)]
struct Entry {
    remaining: f64,
    test_remaining: f64,
    pass_remaining: f64,
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
    let key = hytale_name.to_lowercase();
    let e = st.passes.get(&key)?;
    // De spelerslijst weet het zeker; de stijgende `used`-teller is enkel de terugval als die
    // lijst er niet is. Beide potjes krijgen exact dezelfde behandeling — welk potje loopt,
    // beslist de tale-kant, niet de vraag of je binnen bent.
    let online = match st.online_read {
        Some(t) if t.elapsed() < PLAYTIME_STALE => st.online_now.contains(&key),
        _ => e.last_rise.is_some_and(|t| t.elapsed() < ONLINE_GRACE),
    };
    Some(Ledger {
        remaining: e.remaining.max(0.0),
        test_remaining: e.test_remaining.max(0.0),
        pass_remaining: e.pass_remaining.max(0.0),
        online,
        used: e.used,
        test: e.test,
    })
}

/// Leest de spelerslijst van de speeltijd-teller: `{"updated": …, "open": {"naam": start}}`.
/// Een te oud bestand telt als geen lijst — dan blijft de oude gok gelden.
fn sample_playtime(path: &str) -> Result<usize, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let updated = v.get("updated").and_then(|u| u.as_f64()).ok_or("geen updated-veld")?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_secs_f64();
    if now - updated > PLAYTIME_STALE.as_secs_f64() {
        let mut st = state().lock().map_err(|_| "state vergrendeld")?;
        st.online_read = None;
        return Err(format!("speeltijd-teller staat stil ({:.0}s oud)", now - updated));
    }
    let open = v.get("open").and_then(|o| o.as_object()).ok_or("geen open-object")?;
    let names: HashSet<String> = open.keys().map(|k| k.to_lowercase()).collect();
    let mut st = state().lock().map_err(|_| "state vergrendeld")?;
    st.online_now = names;
    st.online_read = Some(Instant::now());
    Ok(st.online_now.len())
}

/// Lees het bestand één keer in en werk de toestand bij. Geeft het aantal passen terug
/// plus elke **stijging** van een tegoed sinds de vorige lezing — de toekenningen die het
/// logboek moet vastleggen. De allereerste lezing (na een herstart van market) levert er
/// nooit: dan is er geen vorige stand, en zou elke lopende pas als "net toegekend" lezen.
fn sample(path: &str) -> Result<(usize, Vec<Grant>), String> {
    let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let passes = v.get("passes").and_then(|p| p.as_object()).ok_or("geen passes-object")?;

    let mut st = state().lock().map_err(|_| "state vergrendeld")?;
    // Zonder vorige lezing is er niets om mee te vergelijken: dan enkel ijken.
    let baseline = !st.have_data;
    let mut grants: Vec<Grant> = Vec::new();
    let mut fresh: HashMap<String, Entry> = HashMap::new();
    for (name, p) in passes {
        let used = p.get("used").and_then(|u| u.as_f64()).unwrap_or(0.0);
        // Formaat v3: twee potjes apart (testtijd + pastijd). Voor ons telt vooral wat er
        // samen nog op staat; `kind` zegt welke van de twee nu loopt.
        let f = |k: &str| p.get(k).and_then(|x| x.as_f64());
        let (test_remaining, pass_remaining) = match (f("test_remaining"), f("pass_remaining")) {
            (Some(t), Some(q)) => (t, q),
            // Ouder formaat (v2) of half ingevuld: één klok, en die telt als pastijd — zo
            // ziet niemand plots een testklok die er niet is, en gaat er geen tijd verloren.
            _ => (0.0, f("remaining").unwrap_or_else(|| f("granted").unwrap_or(0.0) - used)),
        };
        let remaining = test_remaining + pass_remaining;
        let test = p.get("kind").and_then(|k| k.as_str()) == Some("test");
        let key = name.to_lowercase();
        let prev = st.passes.get(&key);
        let last_rise = match prev {
            // `used` steeg ⇒ die speler is nu aan het spelen.
            Some(old) if used > old.used + 0.001 => Some(Instant::now()),
            Some(old) => old.last_rise,
            None => None,
        };
        // Een tegoed dat omhoog gaat, kan maar één oorzaak hebben: er is tijd bijgekocht,
        // ingewisseld of toegekend. (Spelen laat het zakken, een refund ook.) Beide potjes
        // apart, zodat testtijd niet als gewone pastijd in het logboek belandt.
        if !baseline {
            let (was_test, was_pass) = match prev {
                Some(old) => (old.test_remaining, old.pass_remaining),
                // Nieuw in het bestand ⇒ zijn eerste pas: alles wat erop staat is nieuw.
                None => (0.0, 0.0),
            };
            for (added, after, is_test) in [
                (pass_remaining - was_pass, pass_remaining, false),
                (test_remaining - was_test, test_remaining, true),
            ] {
                if added > GRANT_MIN {
                    grants.push(Grant {
                        name: name.clone(),
                        added,
                        after,
                        total_after: remaining,
                        test: is_test,
                    });
                }
            }
        }
        fresh.insert(key, Entry { remaining, test_remaining, pass_remaining, used, last_rise, test });
    }
    st.passes = fresh;
    st.have_data = true;
    Ok((st.passes.len(), grants))
}

/// Seconden als `2h 05m` / `45m` — het logboek moet in één oogopslag leesbaar zijn.
fn hm(secs: f64) -> String {
    let s = secs.round().max(0.0) as i64;
    let (h, m) = (s / 3600, (s % 3600) / 60);
    if h > 0 {
        format!("{h}h {m:02}m")
    } else if m > 0 {
        format!("{m}m")
    } else {
        format!("{s}s")
    }
}

/// Schrijf één toekenning in het serverlogboek (categorie `hytale`). De speler staat er
/// onder zijn Discord-naam als we die kennen — anders onder zijn in-game naam, want een
/// Twitch-pas van iemand zonder Discord-koppeling hoort evengoed in het logboek.
fn log_grant(pool: &DbPool, g: &Grant) {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());
    let member = db::member_by_hytale_name(pool, &g.name);
    let (uid, who) = match &member {
        Some((u, n)) => (u.as_str(), n.as_str()),
        None => ("", g.name.as_str()),
    };
    let pot = if g.test { "test time" } else { "pass time" };
    // Heeft hij maar één potje lopen, dan is "op die pas" en "in totaal" hetzelfde getal —
    // dat twee keer zetten leest als een fout. Enkel bij test + pas naast elkaar splitsen.
    let after = if (g.total_after - g.after).abs() < 1.0 {
        format!("{} left", hm(g.after))
    } else {
        format!("{} {pot} left, {} in total", hm(g.after), hm(g.total_after))
    };
    let detail = format!("{} +{} {pot} → {after}", g.name, hm(g.added));
    db::log_event(
        pool,
        now,
        &db::LogEntry::new("hytale", if g.test { "test_added" } else { "time_added" })
            .actor(uid, who)
            // Minuten, niet seconden: de bedragkolom moet leesbaar blijven.
            .amount((g.added / 60.0).round() as i64)
            .detail(detail),
    );
}

/// Achtergrondtaak: houdt de gegevens vers, en legt elke toekenning van speeltijd vast
/// in het serverlogboek (categorie `hytale`).
pub async fn run(pool: DbPool) {
    let path = std::env::var("MARKET_PASSES_JSON").unwrap_or_else(|_| DEFAULT_PATH.to_string());
    let ptpath =
        std::env::var("MARKET_PLAYTIME_JSON").unwrap_or_else(|_| DEFAULT_PLAYTIME.to_string());
    // Eén keer luid zeggen of dit werkt: zonder leesrecht is de pas-teller op de site de
    // oude wandklok, en dat wil je weten vóór iemand zich afvraagt waarom de tijd niet klopt.
    match sample(&path) {
        // Deze eerste lezing ijkt enkel (zie `sample`) — er staan dus nooit grants in.
        Ok((n, _)) => tracing::info!("Pas-grootboek: {path} gelezen — {n} pas(sen) van de tale-kant"),
        Err(e) => tracing::warn!(
            "Pas-grootboek NIET leesbaar ({path}: {e}) — de site valt terug op aftellen \
             op de aankoopdatum. Market heeft leesrecht op dat bestand nodig."
        ),
    }
    match sample_playtime(&ptpath) {
        Ok(n) => tracing::info!("Spelerslijst: {ptpath} gelezen — {n} speler(s) nu in-game"),
        Err(e) => tracing::warn!(
            "Spelerslijst NIET bruikbaar ({ptpath}: {e}) — de pas-klok op de site valt terug \
             op de gok 'stijgende teller = online', en blijft na het uitloggen dus even \
             doortellen."
        ),
    }
    // De spelerslijst vaker dan het grootboek: dát is wat bepaalt of de klok op de site
    // loopt of stilstaat, en het bestand is klein. Het grootboek zelf verandert toch maar
    // elke 15s (het ritme van de bot).
    let mut n: u32 = 0;
    loop {
        tokio::time::sleep(PLAYTIME_EVERY).await;
        if let Err(e) = sample_playtime(&ptpath) {
            tracing::debug!("spelerslijst lezen faalde: {e}");
        }
        n += 1;
        if n * PLAYTIME_EVERY.as_secs() as u32 >= SAMPLE_EVERY.as_secs() as u32 {
            n = 0;
            match sample(&path) {
                Ok((_, grants)) => {
                    for g in &grants {
                        tracing::info!(
                            "Speeltijd erbij: {} +{} (nu {} op die pas)",
                            g.name,
                            hm(g.added),
                            hm(g.after)
                        );
                        log_grant(&pool, g);
                    }
                }
                Err(e) => tracing::debug!("pas-grootboek lezen faalde: {e}"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &std::path::Path, body: &str) {
        std::fs::write(path, body).unwrap();
    }

    /// De toestand van dit bestand is proces-breed (één grootboek per market), dus twee
    /// tests die tegelijk bemonsteren kijken naar elkaars passen. Elke test neemt daarom
    /// deze sleutel én begint van een schone lei — anders hangt "is dit de eerste lezing?"
    /// van de toevallige volgorde af.
    fn begin() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        let g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut st = state().lock().unwrap_or_else(|e| e.into_inner());
        *st = State::default();
        g
    }

    /// Het formaat dat de tale-bot schrijft, en de afleiding van "speelt nu".
    #[test]
    fn leest_het_grootboek_en_ziet_wie_speelt() {
        let _g = begin();
        let p = std::env::temp_dir().join(format!("passes-{}.json", std::process::id()));
        write(
            &p,
            r#"{"version":2,"passes":{"Waldstein":{"granted":7200.0,"used":0.0,"remaining":7200.0}}}"#,
        );
        assert_eq!(sample(p.to_str().unwrap()).unwrap().0, 1);

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
        let _g = begin();
        let p = std::env::temp_dir().join(format!("passes-kind-{}.json", std::process::id()));
        write(
            &p,
            r#"{"version":2,"passes":{
                 "Tester":{"granted":900.0,"used":0.0,"remaining":900.0,"kind":"test"},
                 "Gewoon":{"granted":7200.0,"used":0.0,"remaining":7200.0,"kind":"normal"},
                 "Oud":{"granted":7200.0,"used":0.0,"remaining":7200.0}}}"#,
        );
        assert_eq!(sample(p.to_str().unwrap()).unwrap().0, 3);
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

    /// De spelerslijst: wie er nu in-game is, en de leeftijdsgrens erop. Een lijst van een uur
    /// geleden zou iedereen eeuwig online houden — dan liever terug naar de gok.
    #[test]
    fn leest_de_spelerslijst_en_weigert_een_oude() {
        let _g = begin();
        let p = std::env::temp_dir().join(format!("playtime-{}.json", std::process::id()));
        let nu = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64();
        write(&p, &format!(r#"{{"updated":{nu},"open":{{"Waldstein":{nu}}}}}"#));
        assert_eq!(sample_playtime(p.to_str().unwrap()).unwrap(), 1);
        {
            let st = state().lock().unwrap();
            assert!(st.online_now.contains("waldstein"), "naam in kleine letters opgeslagen");
            assert!(st.online_read.is_some());
        }

        // Niemand binnen: lege lijst, maar wél een geldige lijst.
        write(&p, &format!(r#"{{"updated":{nu},"open":{{}}}}"#));
        assert_eq!(sample_playtime(p.to_str().unwrap()).unwrap(), 0);
        assert!(state().lock().unwrap().online_read.is_some());

        // Teller staat stil (bestand van een uur oud) → geen lijst meer, terug naar de gok.
        let oud = nu - 3600.0;
        write(&p, &format!(r#"{{"updated":{oud},"open":{{"Waldstein":{oud}}}}}"#));
        assert!(sample_playtime(p.to_str().unwrap()).is_err(), "te oud = onbruikbaar");
        assert!(state().lock().unwrap().online_read.is_none());

        let _ = std::fs::remove_file(p);
    }

    /// Een onleesbaar of kapot bestand mag nooit een verkeerde tijd tonen — dan liever niets.
    #[test]
    fn kapot_of_afwezig_bestand_geeft_niets() {
        let _g = begin();
        assert!(sample("/bestaat/echt/niet.json").is_err());
        let p = std::env::temp_dir().join(format!("passes-bad-{}.json", std::process::id()));
        write(&p, "{geen json");
        assert!(sample(p.to_str().unwrap()).is_err());
        write(&p, r#"{"version":2}"#);
        assert!(sample(p.to_str().unwrap()).is_err(), "zonder passes-object: geen gegevens");
        let _ = std::fs::remove_file(p);
    }

    /// De kern van het logboek: een tegoed dat omhoog gaat = tijd toegekend. Spelen
    /// (tegoed zakt) en de eerste lezing na een herstart mogen nooit als toekenning tellen.
    #[test]
    fn ziet_toegekende_speeltijd_en_zwijgt_bij_de_rest() {
        let _g = begin();
        let p = std::env::temp_dir().join(format!("passes-grant-{}.json", std::process::id()));
        let f = |test: f64, pas: f64, used: f64| {
            format!(
                r#"{{"version":3,"passes":{{"Waldstein":{{"test_remaining":{test},
                     "pass_remaining":{pas},"used":{used},"kind":"normal"}}}}}}"#
            )
        };

        // Eerste lezing = ijkpunt. Er staat al een pas op, maar die is niet nét toegekend.
        write(&p, &f(0.0, 3600.0, 0.0));
        assert!(sample(p.to_str().unwrap()).unwrap().1.is_empty(), "herstart logt niets");

        // Hij speelt een half uur: het tegoed zakt. Geen toekenning.
        write(&p, &f(0.0, 1800.0, 1800.0));
        assert!(sample(p.to_str().unwrap()).unwrap().1.is_empty(), "spelen is geen toekenning");

        // Hij koopt 2u bij: het tegoed springt omhoog — dát is het bewijs.
        write(&p, &f(0.0, 9000.0, 1800.0));
        let g = sample(p.to_str().unwrap()).unwrap().1;
        assert_eq!(g.len(), 1, "één toekenning");
        assert_eq!(g[0].name, "Waldstein");
        assert_eq!(g[0].added, 7200.0, "+2u");
        assert_eq!(g[0].after, 9000.0, "stand erna");
        assert_eq!(g[0].total_after, 9000.0);
        assert!(!g[0].test, "gewone pastijd");

        // Testtijd zit in een eigen potje en wordt apart gemeld.
        write(&p, &f(900.0, 9000.0, 1800.0));
        let g = sample(p.to_str().unwrap()).unwrap().1;
        assert_eq!(g.len(), 1);
        assert!(g[0].test && g[0].added == 900.0);
        assert_eq!(g[0].total_after, 9900.0, "totaal = beide potjes");

        // Ruis van een paar seconden is geen toekenning.
        write(&p, &f(902.0, 9000.0, 1800.0));
        assert!(sample(p.to_str().unwrap()).unwrap().1.is_empty(), "ruis telt niet");

        let _ = std::fs::remove_file(p);
    }

    /// Wie voor het eerst in het bestand verschijnt, kreeg zijn eerste pas — dat is een
    /// echte toekenning, geen ijkpunt (het ijkpunt geldt enkel voor de eerste lezing).
    #[test]
    fn eerste_pas_van_een_nieuwe_speler_telt_wel() {
        let _g = begin();
        let p = std::env::temp_dir().join(format!("passes-nieuw-{}.json", std::process::id()));
        write(&p, r#"{"version":3,"passes":{"Oud":{"pass_remaining":60.0,"test_remaining":0.0}}}"#);
        sample(p.to_str().unwrap()).unwrap();

        write(
            &p,
            r#"{"version":3,"passes":{"Oud":{"pass_remaining":60.0,"test_remaining":0.0},
                 "Nieuw":{"pass_remaining":7200.0,"test_remaining":0.0}}}"#,
        );
        let g = sample(p.to_str().unwrap()).unwrap().1;
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].name, "Nieuw");
        assert_eq!(g[0].added, 7200.0, "alles wat erop staat is nieuw");

        let _ = std::fs::remove_file(p);
    }

    /// De hele weg: toekenning → regel in het serverlogboek, onder de Discord-naam van
    /// het lid. Dit is wat de admin op de Log-pagina onder "🎮 Hytale" te zien krijgt.
    #[test]
    fn schrijft_de_toekenning_in_het_logboek() {
        let dbp = std::env::temp_dir().join(format!("market-grant-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&dbp);
        let pool = db::init_pool(dbp.to_str().unwrap());
        db::set_hytale_name(&pool, "u1", "Waldstein#0", "Waldstein");

        log_grant(
            &pool,
            &Grant {
                name: "Waldstein".into(),
                added: 7200.0,
                after: 9000.0,
                total_after: 9000.0,
                test: false,
            },
        );

        let rows = db::recent_log(&pool, &["hytale"], 10);
        assert_eq!(rows.len(), 1, "één regel in de categorie hytale");
        assert_eq!(rows[0].event, "time_added");
        assert_eq!(rows[0].actor_uid, "u1");
        assert_eq!(rows[0].actor_name, "Waldstein#0", "het lid, niet enkel de in-game naam");
        assert_eq!(rows[0].amount, Some(120), "toegevoegde tijd in minuten");
        assert!(rows[0].detail.contains("+2h 00m"), "hoeveel erbij kwam: {}", rows[0].detail);
        assert!(rows[0].detail.contains("2h 30m"), "en wat er daarna op stond");

        // Een naam die aan geen enkel lid hangt, komt er nog steeds in — onder zichzelf.
        log_grant(
            &pool,
            &Grant {
                name: "Vreemde".into(),
                added: 900.0,
                after: 900.0,
                total_after: 900.0,
                test: true,
            },
        );
        let rows = db::recent_log(&pool, &["hytale"], 10);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].event, "test_added", "testtijd apart gemerkt");
        assert_eq!(rows[0].actor_name, "Vreemde");
        assert!(rows[0].actor_uid.is_empty());

        let _ = std::fs::remove_file(dbp);
    }

    /// De leesbare vorm in het logboek.
    #[test]
    fn toont_tijd_leesbaar() {
        assert_eq!(hm(7200.0), "2h 00m");
        assert_eq!(hm(9000.0), "2h 30m");
        assert_eq!(hm(900.0), "15m");
        assert_eq!(hm(42.0), "42s");
        assert_eq!(hm(-5.0), "0s");
    }
}
