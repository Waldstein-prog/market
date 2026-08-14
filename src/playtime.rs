//! Hoeveel tijd bracht iemand al op Meadowland door? — ook dat weet de **tale-kant**.
//!
//! De speeltijd wordt daar geteld uit de server-journal (`hytale-playtime.service`, zie
//! `tale/playtime/playtime.py`) en weggeschreven in `playtime.json`:
//!
//! ```json
//! {"version": 1, "seconds": {"faybelle": 286457.0}, "names": {"faybelle": "Faybelle"},
//!  "open": {"faybelle": 1786538000.0}}
//! ```
//!
//! `seconds` telt de **afgesloten** sessies; wie nu binnen is staat in `open` met zijn
//! starttijd. Wij tellen die twee op, zodat de lopende sessie meteen meetelt en er nooit
//! dubbel geboekt wordt. Dit is niet hetzelfde als het pas-verbruik uit [`crate::pass_ledger`]:
//! dát telt enkel tijd die van een pas ging, terwijl dit álle tijd op de server is.
//!
//! Market schrijft nooit in dit bestand. Is het er niet, dan geeft `lookup` niets terug en
//! blijft de regel gewoon weg — liever geen cijfer dan een verzonnen cijfer.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// Standaardpad op de VPS; te overschrijven met `MARKET_PLAYTIME_JSON` (voor tests).
const DEFAULT_PATH: &str = "/opt/hytale/playtime.json";
/// Zelfde ritme als het pas-grootboek: vers genoeg, en een paginabezoek wacht er nooit op.
const SAMPLE_EVERY: Duration = Duration::from_secs(20);

#[derive(Default)]
struct State {
    /// Naam (kleine letters) → (afgesloten seconden, start van de lopende sessie).
    players: HashMap<String, (f64, Option<f64>)>,
    have_data: bool,
}

fn state() -> &'static Mutex<State> {
    static S: OnceLock<Mutex<State>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(State::default()))
}

/// Totale speeltijd van deze speler in seconden, lopende sessie inbegrepen.
/// None = geen gegevens (bestand onleesbaar, of die naam speelde nog nooit).
pub fn lookup(hytale_name: &str) -> Option<f64> {
    let st = state().lock().ok()?;
    if !st.have_data {
        return None;
    }
    let (closed, open_since) = st.players.get(&hytale_name.trim().to_lowercase())?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    // Een sessie die volgens het bestand nog loopt: tel bij tot nu. Ligt de tracker plat,
    // dan blijft die start staan — vandaar de ondergrens 0 i.p.v. blind optellen.
    let running = open_since.map_or(0.0, |s| (now - s).max(0.0));
    Some(closed + running)
}

/// "79h 38m" / "12m" — dezelfde vorm als de pas-teller ernaast.
pub fn human(secs: f64) -> String {
    let s = secs.max(0.0) as i64;
    if s >= 3600 {
        format!("{}h {}m", s / 3600, (s % 3600) / 60)
    } else {
        format!("{}m", s / 60)
    }
}

fn sample(path: &str) -> Result<usize, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let secs = v.get("seconds").and_then(|s| s.as_object()).ok_or("geen seconds-object")?;
    let open = v.get("open").and_then(|o| o.as_object());

    let mut players: HashMap<String, (f64, Option<f64>)> = HashMap::new();
    for (name, val) in secs {
        let key = name.to_lowercase();
        let closed = val.as_f64().unwrap_or(0.0);
        players.insert(key, (closed, None));
    }
    if let Some(open) = open {
        for (name, val) in open {
            let key = name.to_lowercase();
            let start = val.as_f64();
            // Iemand die nu voor het eerst binnen is, staat nog niet in `seconds`.
            players.entry(key).or_insert((0.0, None)).1 = start;
        }
    }
    let mut st = state().lock().map_err(|_| "state vergrendeld")?;
    st.players = players;
    st.have_data = true;
    Ok(st.players.len())
}

/// Achtergrondtaak: houdt de gegevens vers.
pub async fn run() {
    let path = std::env::var("MARKET_PLAYTIME_JSON").unwrap_or_else(|_| DEFAULT_PATH.to_string());
    match sample(&path) {
        Ok(n) => tracing::info!("Speeltijd: {path} gelezen — {n} speler(s) van de tale-kant"),
        Err(e) => tracing::warn!(
            "Speeltijd NIET leesbaar ({path}: {e}) — de inventaris toont dan geen speeltijd. \
             Draait hytale-playtime.service, en mag market dat bestand lezen?"
        ),
    }
    loop {
        tokio::time::sleep(SAMPLE_EVERY).await;
        if let Err(e) = sample(&path) {
            tracing::debug!("speeltijd lezen faalde: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lopende_sessie_telt_mee_maar_nooit_dubbel() {
        let p = std::env::temp_dir().join(format!("playtime-{}.json", std::process::id()));
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();
        std::fs::write(
            &p,
            format!(
                r#"{{"version":1,"seconds":{{"Faybelle":3600.0,"heiji_cat":120.0}},
                     "names":{{"faybelle":"Faybelle"}},"open":{{"faybelle":{}}}}}"#,
                now - 600.0
            ),
        )
        .unwrap();
        assert_eq!(sample(p.to_str().unwrap()), Ok(2));

        // 1 uur afgesloten + 10 minuten lopend, hoofdletters doen niet mee.
        let f = lookup("FAYBELLE").expect("Faybelle staat erin");
        assert!((f - 4200.0).abs() < 5.0, "kreeg {f}");
        // Wie niet binnen is, krijgt er niets bij.
        assert_eq!(lookup("Heiji_Cat"), Some(120.0));
        assert_eq!(lookup("Onbekend"), None);

        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn uren_en_minuten() {
        assert_eq!(human(4200.0), "1h 10m");
        assert_eq!(human(720.0), "12m");
        assert_eq!(human(-5.0), "0m");
    }
}
