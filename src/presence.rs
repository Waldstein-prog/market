//! Wie staat er op de Hytale-server? — en dus: welke pas loopt en welke staat stil.
//!
//! Een pas van N uur is N uur **speeltijd**. Om die klok te kunnen stilzetten moet market
//! weten wanneer iemand in- en uitlogt. De server schrijft dat al weg in
//! `chat_mirror.log` (dezelfde bron die de Discord-chatbrug voedt), tab-gescheiden:
//!
//! ```text
//! 1785663953	join	TechHeadFred joined the game
//! 1785667300	leave	TechHeadFred left the game
//! 1785669116	server	Server is going offline
//! ```
//!
//! Waarom dit bestand en niet de Discord-embeds van de chatbrug: dit is **herspeelbaar**.
//! Market onthoudt hoever hij las, dus na een herstart of een storing haalt hij de gemiste
//! in- en uitloggen gewoon in. Bij Discord meelezen zou elke gemiste `leave` betekenen dat
//! iemands pas eeuwig doortelt terwijl hij niet speelt.
//!
//! **Faalt veilig.** Is het logbestand onleesbaar (rechten) of afwezig, dan komt er nooit
//! een gebeurtenis binnen, pauzeert er niets, en gedraagt een pas zich exact zoals vroeger:
//! een gewone wandklok. Market raakt de tale-kant niet aan — het leest enkel mee.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::db::{self, DbPool};

fn now_secs() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64()
}

/// Standaardpad op de VPS; te overschrijven met `MARKET_PRESENCE_LOG` (handig om te testen).
const DEFAULT_LOG: &str = "/opt/hytale/Server/chat_mirror.log";
/// Hoe vaak we naar nieuwe regels kijken. Een pas telt in uren; seconden-precisie volstaat ruim.
const POLL: Duration = Duration::from_secs(5);
/// Hoe vaak een gepauzeerde pas z'n `expires` vooruit geschoven krijgt, zodat de tale-bot
/// hem op de whitelist houdt. Ruim onder `db::PAUSE_KEEPALIVE_FLOOR`.
const KEEPALIVE_EVERY: Duration = Duration::from_secs(300);
/// Waar we gebleven waren in het logbestand (byte-offset), zodat een herstart bijbeent.
/// Interne boekhouding — bewust géén Settings-veld: hier valt voor de user niets te kiezen.
const OFFSET_KEY: &str = "presence_log_offset";

/// Eén regel uit het spiegel-log, voor zover ze ons aangaat.
#[derive(Debug, PartialEq)]
enum Event {
    Join(String),
    Leave(String),
    /// Server gaat plat: iedereen is per definitie offline.
    ServerDown,
}

/// Ontleed één logregel: `<epoch>\t<soort>\t<tekst>`. De naam is het eerste woord van de
/// tekst — Hytale-namen bevatten geen spaties (`^[A-Za-z0-9_]{1,32}$`), dus dat is eenduidig.
/// We vertrouwen op het soort-veld, niet op de Engelse zin erachter: die is van de tale-kant
/// en mag veranderen zonder dat dit stuk breekt.
fn parse_line(line: &str) -> Option<Event> {
    let mut f = line.split('\t');
    let _ts = f.next()?;
    let kind = f.next()?;
    let text = f.next().unwrap_or_default().trim();
    let first = text.split_whitespace().next().unwrap_or_default();
    match kind {
        "join" if crate::web::valid_hytale_name(first) => Some(Event::Join(first.to_string())),
        "leave" if crate::web::valid_hytale_name(first) => Some(Event::Leave(first.to_string())),
        // "Server is going offline" wél, "Server is online" niet: bij het opstarten is er
        // nog niemand binnen, en de joins die volgen vertellen ons de rest.
        "server" if text.contains("going offline") => Some(Event::ServerDown),
        _ => None,
    }
}

/// Voer één gebeurtenis uit op de passen.
fn apply(pool: &DbPool, ev: Event, now: f64) {
    match ev {
        Event::Join(name) => {
            let n = db::resume_pass(pool, &name, now);
            if n > 0 {
                tracing::info!("pas hervat: {name} is online ({n} grant(s))");
            }
        }
        Event::Leave(name) => {
            let n = db::pause_pass(pool, &name, now);
            if n > 0 {
                tracing::info!("pas op pauze: {name} is offline ({n} grant(s))");
            }
        }
        Event::ServerDown => {
            let n = db::pause_all_passes(pool, now);
            if n > 0 {
                tracing::info!("server offline — {n} pas(sen) op pauze");
            }
        }
    }
}

/// Achtergrondtaak: volgt het log en houdt gepauzeerde passen whitelistbaar.
pub async fn run(pool: DbPool) {
    let path = std::env::var("MARKET_PRESENCE_LOG").unwrap_or_else(|_| DEFAULT_LOG.to_string());

    // Eén keer luid zeggen of dit werkt. Zonder leesrecht draait market gewoon door met
    // wandklok-passen — dat is een geldige toestand, geen fout, maar je moet het wel weten.
    match std::fs::metadata(&path) {
        Ok(_) => tracing::info!("Aanwezigheid: volg {path} — passen tellen enkel speeltijd"),
        Err(e) => {
            tracing::warn!(
                "Aanwezigheid UIT: {path} niet leesbaar ({e}). Passen blijven op wandklok \
                 aftellen; pauzeren vraagt leesrecht op dat bestand."
            );
            return;
        }
    }

    let mut offset: u64 =
        db::setting_get(&pool, OFFSET_KEY).and_then(|v| v.parse().ok()).unwrap_or(0);
    let mut last_keepalive = std::time::Instant::now();

    loop {
        let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        // Bestand korter dan onze offset ⇒ het is vervangen of afgekapt. Vanaf nul herlezen
        // is veilig: elke join wordt gevolgd door de bijbehorende leave, en die twee heffen
        // elkaar in speeltijd praktisch op (de tijd tússen beide is hier bijna nul).
        if len < offset {
            tracing::warn!("Aanwezigheid: {path} is korter dan verwacht — opnieuw vanaf het begin");
            offset = 0;
        }
        if len > offset {
            match read_from(&path, offset) {
                Ok((text, new_offset)) => {
                    let now = now_secs();
                    for line in text.lines() {
                        if let Some(ev) = parse_line(line) {
                            apply(&pool, ev, now);
                        }
                    }
                    offset = new_offset;
                    db::setting_set(&pool, OFFSET_KEY, &offset.to_string());
                }
                Err(e) => tracing::warn!("Aanwezigheid: kon {path} niet lezen: {e}"),
            }
        }

        if last_keepalive.elapsed() >= KEEPALIVE_EVERY {
            last_keepalive = std::time::Instant::now();
            db::keepalive_paused_passes(&pool, now_secs());
        }
        tokio::time::sleep(POLL).await;
    }
}

/// Lees vanaf `offset` tot het einde. Geeft de tekst plus de nieuwe offset, waarbij een
/// half geschreven laatste regel bewust blijft liggen tot ze compleet is.
fn read_from(path: &str, offset: u64) -> std::io::Result<(String, u64)> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path)?;
    f.seek(SeekFrom::Start(offset))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    // Enkel tot en met de laatste volledige regel verwerken.
    let cut = match buf.iter().rposition(|b| *b == b'\n') {
        Some(i) => i + 1,
        None => return Ok((String::new(), offset)),
    };
    let text = String::from_utf8_lossy(&buf[..cut]).into_owned();
    Ok((text, offset + cut as u64))
}

#[cfg(test)]
mod tests {
    use super::{parse_line, Event};

    #[test]
    fn leest_de_soorten_die_we_nodig_hebben() {
        assert_eq!(
            parse_line("1785663953\tjoin\tTechHeadFred joined the game"),
            Some(Event::Join("TechHeadFred".into()))
        );
        assert_eq!(
            parse_line("1785667300\tleave\tWaldstein left the game"),
            Some(Event::Leave("Waldstein".into()))
        );
        assert_eq!(
            parse_line("1785669116\tserver\tServer is going offline"),
            Some(Event::ServerDown)
        );
    }

    #[test]
    fn negeert_de_rest() {
        // Een dood is geen uitlog: die speler staat gewoon nog op de server.
        assert_eq!(
            parse_line("1785665188\tdeath\tTechHeadFred was killed by Magma Rhino Toad!"),
            None
        );
        // "Server is online" zegt niets over wie er binnen is.
        assert_eq!(parse_line("1785669129\tserver\tServer is online"), None);
        // Rommel mag nooit tot een naam leiden.
        assert_eq!(parse_line("kapot"), None);
        assert_eq!(parse_line("1\tjoin\t"), None);
        assert_eq!(parse_line("1\tjoin\tnaam met spatie joined"), Some(Event::Join("naam".into())));
        // Een naam die market nooit zou whitelisten hoort ook hier geweigerd te worden.
        assert_eq!(parse_line("1\tjoin\tfoute-naam joined the game"), None);
    }
}
