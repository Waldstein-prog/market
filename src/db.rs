//! SQLite-persistentie voor de coin-economy (rusqlite + r2d2, zoals cyd).
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rand::Rng;
use rusqlite::{OptionalExtension, TransactionBehavior, params};

pub type DbPool = Pool<SqliteConnectionManager>;

pub fn init_pool(path: &str) -> DbPool {
    // Elke pooled verbinding krijgt `busy_timeout` (per-connectie, vandaar via `with_init` en
    // niet één keer na Pool::new): wacht tot 5s op de write-lock i.p.v. meteen SQLITE_BUSY (=
    // panic) te geven wanneer bot-, web- en twitch-taak gelijktijdig in dezelfde DB schrijven.
    //
    // BEWUST GEEN WAL: het Hytale-panel (user `hytale`) leest `coins.db` RECHTSTREEKS read-only
    // uit `/opt/market/`, een map waar het niet kan schrijven. Onder WAL faalt zo'n read tijdens
    // het deploy-venster — na een clean shutdown ruimt SQLite `-wal`/`-shm` op maar houdt de
    // header op WAL, en de read-only opener kan de `-shm` niet heraanmaken → "attempt to write a
    // readonly database" (empirisch gereproduceerd, 2026-07-19). Het rollback-journal laat read-
    // only-lezers altijd door; `busy_timeout` volstaat om de panics weg te nemen.
    let manager = SqliteConnectionManager::file(path)
        .with_init(|c| c.execute_batch("PRAGMA busy_timeout = 5000;"));
    let pool = Pool::new(manager).expect("kan SQLite-pool niet aanmaken");
    let conn = pool.get().expect("kan DB-verbinding niet ophalen");
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS coins (
            user_id     TEXT PRIMARY KEY,
            username    TEXT NOT NULL,
            coins       INTEGER NOT NULL DEFAULT 0,
            last_award  REAL NOT NULL DEFAULT 0,
            last_daily  REAL NOT NULL DEFAULT 0,
            max_balance  INTEGER NOT NULL DEFAULT 0,
            is_public    INTEGER NOT NULL DEFAULT 0,
            total_earned INTEGER NOT NULL DEFAULT 0,
            name_color   TEXT NOT NULL DEFAULT '',
            perma_access INTEGER NOT NULL DEFAULT 0,
            discord_color TEXT NOT NULL DEFAULT '',
            daily_streak INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS sessions (
            token    TEXT PRIMARY KEY,
            user_id  TEXT NOT NULL,
            username TEXT NOT NULL,
            created  REAL NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS shelves (
            id       INTEGER PRIMARY KEY AUTOINCREMENT,
            title    TEXT NOT NULL,
            position INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS items (
            id       INTEGER PRIMARY KEY AUTOINCREMENT,
            zone     TEXT NOT NULL DEFAULT 'shelf',   -- 'shelf' | 'lucky'
            shelf_id INTEGER,                          -- NULL voor lucky
            name     TEXT NOT NULL DEFAULT '',
            price    INTEGER NOT NULL DEFAULT 0,
            image    TEXT NOT NULL DEFAULT '',
            color    TEXT NOT NULL DEFAULT '',
            position INTEGER NOT NULL DEFAULT 0,
            role_id  TEXT NOT NULL DEFAULT '',       -- kent deze rol toe bij aankoop
            duration INTEGER NOT NULL DEFAULT 0,     -- 0 = permanent, >0 = seconden (bv 24u)
            category TEXT NOT NULL DEFAULT '',        -- 'primary'|'secondary'|'prism' voor gems
            description TEXT NOT NULL DEFAULT ''
        );
        CREATE TABLE IF NOT EXISTS role_grants (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id    TEXT NOT NULL,
            role_id    TEXT NOT NULL,
            expires_at REAL NOT NULL,
            label      TEXT NOT NULL DEFAULT ''
        );
        CREATE TABLE IF NOT EXISTS inventory (
            id       INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id  TEXT NOT NULL,
            item_id  INTEGER NOT NULL DEFAULT 0,
            name     TEXT NOT NULL DEFAULT '',
            image    TEXT NOT NULL DEFAULT '',
            price    INTEGER NOT NULL DEFAULT 0,
            acquired REAL NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS daily_shop (
            day     INTEGER NOT NULL,
            item_id INTEGER NOT NULL,
            PRIMARY KEY (day, item_id)
        );
        CREATE TABLE IF NOT EXISTS hytale_whitelist (
            user_id     TEXT PRIMARY KEY,   -- Discord-user (één Hytale-account per lid)
            hytale_name TEXT NOT NULL,      -- de in-game naam om te whitelisten
            expires     REAL                -- NULL = permanent, anders epoch-seconden
        );
        CREATE TABLE IF NOT EXISTS earn_log (
            user_id     TEXT NOT NULL,      -- wie verdiende
            amount      INTEGER NOT NULL,   -- hoeveel (chat/daily/chest)
            ts          REAL NOT NULL       -- epoch-seconden van de verdienste
        );
        CREATE INDEX IF NOT EXISTS idx_earn_log_ts ON earn_log(ts);
        -- Laatste activiteit per lid (message OF reactie) in de prod-guild. Voedt
        -- Manage → Inactives (leden die lang niks deden). NB: last_seen wordt vooruit
        -- opgebouwd vanaf uitrol — er is geen retro-historiek in Discord/earn_log.
        CREATE TABLE IF NOT EXISTS member_activity (
            user_id   TEXT PRIMARY KEY,   -- Discord-user
            name      TEXT NOT NULL,      -- laatst gekende weergavenaam ('' = onbekend)
            last_seen REAL NOT NULL       -- epoch-seconden van laatste message/reactie
        );
        CREATE INDEX IF NOT EXISTS idx_member_activity_seen ON member_activity(last_seen);
        -- Vrije sleutel/waarde voor kleine stukjes state (bv. status + tijd van de laatste
        -- Absent-backfill). Los van de getypte `settings`-tabel.
        CREATE TABLE IF NOT EXISTS kv (
            k TEXT PRIMARY KEY,
            v TEXT NOT NULL
        );
        -- Retroactieve inhaalslag voor gemiste coins in THREADS (threads leverden vroeger
        -- niks op, zie thread_parent-fix). Eén rij per gescand bericht: het gerolde bedrag
        -- wordt hier bevroren zodat de preview exact overeenkomt met de uitbetaling. PK op
        -- message_id ⇒ een preview her-scannen rolt nooit opnieuw en betaalt nooit dubbel.
        CREATE TABLE IF NOT EXISTS thread_backfill (
            message_id TEXT PRIMARY KEY,   -- Discord-bericht in een thread
            user_id    TEXT NOT NULL,      -- auteur
            name       TEXT NOT NULL,      -- weergavenaam bij de scan
            amount     INTEGER NOT NULL,   -- gerold bedrag (coin_weights, per bericht)
            applied    INTEGER NOT NULL DEFAULT 0, -- 1 = al uitbetaald (idempotent)
            ts         REAL NOT NULL        -- epoch-seconden van de scan-rol
        );
        CREATE INDEX IF NOT EXISTS idx_thread_backfill_applied ON thread_backfill(applied);
        CREATE TABLE IF NOT EXISTS admin_undo (
            id         INTEGER PRIMARY KEY CHECK(id = 1), -- max één rij: de laatste ingreep
            user_id    TEXT NOT NULL,
            username   TEXT NOT NULL,
            prev_coins INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS coin_archive (
            user_id      TEXT PRIMARY KEY,   -- lid dat de server verliet
            coins        INTEGER NOT NULL,   -- saldo bij vertrek
            total_earned INTEGER NOT NULL,   -- all-time verdiend bij vertrek
            username     TEXT NOT NULL,
            ts           REAL NOT NULL
        );
        CREATE TABLE IF NOT EXISTS coin_channels (
            channel_id TEXT PRIMARY KEY,     -- enkel hier leveren berichten coins op
            name       TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS server_log (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            ts         REAL NOT NULL,               -- epoch-seconden
            category   TEXT NOT NULL,               -- 'chest', later 'coins'|'daily'|'admin'|...
            event      TEXT NOT NULL,               -- 'spawn'|'join'|'already_in'|'too_late'|'win'|'despawn'|...
            actor_uid  TEXT NOT NULL DEFAULT '',    -- wie de actie deed (leeg = systeem)
            actor_name TEXT NOT NULL DEFAULT '',
            channel_id TEXT NOT NULL DEFAULT '',
            ref_id     TEXT NOT NULL DEFAULT '',     -- groepeer-id (bv. chest-bericht-id)
            amount     INTEGER,                       -- prijs/aantal, NULL indien nvt
            detail     TEXT NOT NULL DEFAULT ''       -- vrije tekst / lijst deelnemers
        );
        CREATE INDEX IF NOT EXISTS idx_server_log_ts  ON server_log(ts);
        CREATE INDEX IF NOT EXISTS idx_server_log_cat ON server_log(category, ts);
        CREATE TABLE IF NOT EXISTS chest_cooldowns (
            channel_id TEXT PRIMARY KEY,   -- kanaal met chest-rust
            until      REAL NOT NULL        -- epoch-seconden: geen nieuwe chest vóór dit tijdstip
        );
        CREATE TABLE IF NOT EXISTS live_chests (
            message_id TEXT PRIMARY KEY,   -- Discord-bericht-id van de open chest
            channel_id TEXT NOT NULL,
            pop_ts     INTEGER NOT NULL     -- epoch-seconden waarop de chest hoort te poppen
        );
        -- Admin-instelbare spelparameters. Bot én site lezen deze LIVE (zoals
        -- coin_channels), dus een wijziging via Manage → Settings werkt meteen,
        -- zonder herstart. Waarde als TEXT; `settings.rs` kent het type + default.
        -- De unit zit in de KEY (_sec/_min/_hours) — zo is een eenheidsfout
        -- zichtbaar op de call-site i.p.v. verstopt in een conversie.
        CREATE TABLE IF NOT EXISTS settings (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        -- Gewogen coin-award per bericht: één rij per uitkomst. `weight` is
        -- RELATIEF (som hoeft geen 100 te zijn) en REAL, zodat 'half zoveel
        -- kans' letterlijk 0.5 is. Leeg = vangnet in coin_amount().
        CREATE TABLE IF NOT EXISTS coin_weights (
            amount INTEGER PRIMARY KEY,     -- hoeveel coins deze uitkomst geeft (0 mag)
            weight REAL NOT NULL            -- relatief gewicht, > 0
        );
        -- Prijsverdeling van een chest: gewicht + coin-bereik per tier.
        CREATE TABLE IF NOT EXISTS chest_tiers (
            id       INTEGER PRIMARY KEY AUTOINCREMENT,
            weight   REAL NOT NULL,          -- relatief gewicht, > 0
            lo       INTEGER NOT NULL,       -- min coins (inclusief)
            hi       INTEGER NOT NULL,       -- max coins (inclusief)
            position INTEGER NOT NULL DEFAULT 0
        );
        -- Openstaande level-up-cadeaus: één rij per te claimen cadeau. De speler
        -- claimt via een knop in de embed; pas dán komen de coins op zijn saldo
        -- (claimed 0→1, atomisch). `kind` = 'levelup' of 'correction' (de eenmalige
        -- inhaalslag van 2026-07-18b, commando's nadien verwijderd — data blijft).
        -- `level` = het bereikte level.
        CREATE TABLE IF NOT EXISTS level_gifts (
            id      INTEGER PRIMARY KEY AUTOINCREMENT,
            uid     TEXT NOT NULL,
            amount  INTEGER NOT NULL,
            level   INTEGER NOT NULL,
            kind    TEXT NOT NULL DEFAULT 'levelup',
            claimed INTEGER NOT NULL DEFAULT 0,
            ts      REAL NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_level_gifts_uid ON level_gifts(uid, claimed);
        -- Namenlijst PER ITEM: enkel de leden die hier voor dat item op staan kunnen het
        -- kopen. Gebruikt door de Test Pass (het vakje 'Naam' op de item-kaart in Manage →
        -- Shop). Geen schakelaar: de lijst ís de regel, dus een testpas zonder rijen kan
        -- door niemand gekocht worden — de rest ziet op de koopplek een 🔒.
        -- `username` is enkel de naam zoals ze bij het toevoegen bekend was (weergave);
        -- (item_id, user_id) is de sleutel.
        CREATE TABLE IF NOT EXISTS item_allow (
            item_id  INTEGER NOT NULL,
            user_id  TEXT NOT NULL,
            username TEXT NOT NULL DEFAULT '',
            added    REAL NOT NULL DEFAULT 0,
            PRIMARY KEY (item_id, user_id)
        );
        -- Eén rij per lid: de testpas die hij als laatste kocht en nog moet opbranden.
        -- `used_at` is de stand van de speeltijd-teller (`used` uit passes.json) op het
        -- moment van de aankoop; pas als die met `duration` seconden gestegen is, is de
        -- pas uitgewerkt en mag er een volgende gekocht worden. Bewust op speeltijd en
        -- niet op wandkloktijd: een pas loopt enkel leeg terwijl je in-game bent.
        CREATE TABLE IF NOT EXISTS test_pass_hold (
            user_id  TEXT PRIMARY KEY,
            item_id  INTEGER NOT NULL,
            name_lc  TEXT NOT NULL,
            used_at  REAL NOT NULL,
            duration INTEGER NOT NULL,
            bought   REAL NOT NULL
        );",
    )
    .expect("kan tabel niet aanmaken");

    // Migratie voor bestaande DB's: kolommen toevoegen indien nog afwezig,
    // en max_balance backfillen op het huidige saldo (anders start het op 0).
    ensure_column(&conn, "coins", "last_daily", "REAL NOT NULL DEFAULT 0");
    ensure_column(&conn, "coins", "max_balance", "INTEGER NOT NULL DEFAULT 0");
    ensure_column(&conn, "coins", "is_public", "INTEGER NOT NULL DEFAULT 0");
    ensure_column(&conn, "coins", "total_earned", "INTEGER NOT NULL DEFAULT 0");
    ensure_column(&conn, "coins", "name_color", "TEXT NOT NULL DEFAULT ''");
    ensure_column(&conn, "coins", "perma_access", "INTEGER NOT NULL DEFAULT 0");
    ensure_column(&conn, "coins", "discord_color", "TEXT NOT NULL DEFAULT ''");
    ensure_column(&conn, "coins", "hytale_name", "TEXT NOT NULL DEFAULT ''");
    ensure_column(&conn, "coins", "daily_streak", "INTEGER NOT NULL DEFAULT 0");
    ensure_column(&conn, "coins", "equipped_gem", "TEXT NOT NULL DEFAULT ''");
    // Het Twitch-account dat dit lid in Discord aan zijn profiel hing (Discord →
    // Instellingen → Verbindingen). Gelezen bij de login via de OAuth-scope
    // `connections`; leeg = niet gekoppeld of nog niet opnieuw ingelogd. Dit is de
    // enige betrouwbare brug tussen een `twitch:<id>`-pas en een Discord-lid.
    ensure_column(&conn, "coins", "twitch_id", "TEXT NOT NULL DEFAULT ''");
    // Hoogste level waarvoor al een cadeau-embed gepost is (per lid). Nieuwe level-ups
    // vuren enkel voor levels bóven deze marker → geen dubbele of gemiste cadeaus.
    ensure_column(&conn, "coins", "gifted_level", "INTEGER NOT NULL DEFAULT 0");
    // EENMALIGE baseline: bestaande leden krijgen gifted_level = hun HUIDIGE level, zodat de
    // nieuwe level-up-embed NIET met terugwerkende kracht de hele backlog naar #coins post.
    // Draait exact één keer (gemarkeerd in settings). De aparte inhaalslag voor het verleden
    // (uitgevoerd 2026-07-18b, commando's nadien verwijderd) stond hier volledig los van.
    {
        let done: Option<String> = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'levelgift_baseline_v1'",
                [],
                |r| r.get(0),
            )
            .optional()
            .expect("q baseline flag");
        if done.is_none() {
            let members: Vec<(String, i64)> = {
                let mut stmt = conn
                    .prepare("SELECT user_id, total_earned FROM coins")
                    .expect("prep baseline");
                let it = stmt
                    .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
                    .expect("q baseline");
                it.filter_map(Result::ok).collect()
            };
            for (uid, earned) in members {
                conn.execute(
                    "UPDATE coins SET gifted_level = ?2 WHERE user_id = ?1",
                    params![uid, level_of(earned)],
                )
                .ok();
            }
            conn.execute(
                "INSERT INTO settings (key, value) VALUES ('levelgift_baseline_v1', '1')",
                [],
            )
            .ok();
        }
    }
    // Lucky Horseshoe: 0 = geen boost, 1 = dubbele lot-kans bij de eerstvolgende
    // uitbetalende treasure chest waaraan het lid meedoet (nadien terug op 0).
    ensure_column(&conn, "coins", "chest_luck", "INTEGER NOT NULL DEFAULT 0");
    ensure_column(&conn, "admin_undo", "prev_earned", "INTEGER NOT NULL DEFAULT 0");
    ensure_column(&conn, "items", "role_id", "TEXT NOT NULL DEFAULT ''");
    ensure_column(&conn, "items", "duration", "INTEGER NOT NULL DEFAULT 0");
    ensure_column(&conn, "items", "category", "TEXT NOT NULL DEFAULT ''");
    ensure_column(&conn, "items", "description", "TEXT NOT NULL DEFAULT ''");
    ensure_column(&conn, "items", "image2", "TEXT NOT NULL DEFAULT ''");
    // Uitverkocht: item blijft zichtbaar in de shop, maar de Buy-knop wordt grijs
    // ("Out of Stock"). Default 0 = gewoon te koop.
    ensure_column(&conn, "items", "sold_out", "INTEGER NOT NULL DEFAULT 0");
    // Voorraad: hoeveel exemplaren er nog te koop zijn. **-1 = onbeperkt** (en dus de
    // default: bestaande items — gems e.d. — zijn niet voorraad-gestuurd en moeten dat
    // ook niet plots worden). Een admin vult "Add stock" in en telt er zo bij op; elke
    // aankoop telt er één af, en op 0 staat het item op Out of Stock.
    ensure_column(&conn, "items", "stock", "INTEGER NOT NULL DEFAULT -1");
    // Dagrotatie per item (vervangt de aparte `horseshoe_shop_odds_days`-instelling):
    // `shop_weight` = relatief lot-gewicht bij de dagelijkse trekking, `in_rotation` = doet
    // dit item überhaupt mee. Twee velden i.p.v. één, zodat een item tijdelijk uit de shop
    // kan zonder dat het ingestelde gewicht verloren gaat. Default: gewicht 10 (ruimte om
    // met gehele getallen fijner te regelen dan met 1) en meedoen — behalve de passen, die
    // hun eigen vaste plek op de shop hebben; die zet de seed hieronder op 0.
    let rotation_is_new = !column_exists(&conn, "items", "shop_weight");
    ensure_column(&conn, "items", "shop_weight", "REAL NOT NULL DEFAULT 10.0");
    ensure_column(&conn, "items", "in_rotation", "INTEGER NOT NULL DEFAULT 1");
    if rotation_is_new {
        // Passen (category 'boost') staan al permanent op de shop → niet in de dagtrekking.
        conn.execute("UPDATE items SET in_rotation = 0 WHERE category = 'boost'", []).ok();
        // De booster hield vóór deze kolommen zijn zeldzaamheid uit `horseshoe_shop_odds_days`
        // (1-op-14 per dag ≈ 7% van de dagen zichtbaar). Gewicht 2 tegen 10 voor de gems komt
        // daar bij 12 gems + 4 slots vlak bij uit, zodat er bij de overgang niets verspringt.
        conn.execute("UPDATE items SET shop_weight = 2.0 WHERE category = 'booster'", []).ok();
    }
    // Testpas: een pas die enkel voor genodigden is (de namenlijst op zijn eigen kaart
    // in Manage → Shop). Dit is een eigenschap van het ítem, niet van de categorie:
    // de gewone Meadowland Pass staat gewoon te koop en heeft aan `sold_out`/`stock`
    // genoeg als rem — vóór deze kolom hing de testerspoort aan `category = 'boost'`,
    // waardoor een lege testerslijst óók de gewone pas op Out of Stock zette.
    let testpas_is_new = !column_exists(&conn, "items", "test_pass");
    ensure_column(&conn, "items", "test_pass", "INTEGER NOT NULL DEFAULT 0");
    if testpas_is_new {
        // Eenmalige overname bij het invoeren van de kolom: de bestaande testpas was de
        // gratis pas (prijs 0) — een pas die niets kost is er nooit een om te verkopen.
        // Alleen hier, één keer; daarna beslist het vinkje in Manage → Shop.
        conn.execute("UPDATE items SET test_pass = 1 WHERE category = 'boost' AND price = 0", [])
            .ok();
    }
    // De vaste vorm van een testpas: gratis, onbeperkt, niet in de dagtrekking en nooit
    // "uitverkocht". Die vier vakjes staan niet meer op zijn beheerkaart, dus mag er ook
    // geen oude waarde uit een vorige opzet stil blijven hangen — de testpas op prod stond
    // bijvoorbeeld nog op Out of stock, en dat zou hem dicht houden zonder dat er op zijn
    // kaart nog iets te zien is dat dat verklaart. Eén UPDATE bij elke start: zo kan de DB
    // niets zeggen wat de kaart niet toont.
    conn.execute(
        "UPDATE items SET price = 0, stock = -1, sold_out = 0, in_rotation = 0
           WHERE test_pass = 1",
        [],
    )
    .ok();
    // Migratie (2026-08-13): de testerslijst was server-breed (`pass_allow`) met een
    // schakelaar (`pass_allowlist_on`) die bepaalde of ze gold. Ze hangt nu aan het ítem
    // zelf — de Test Pass heeft zijn eigen namenlijst op zijn kaart. De bestaande namen
    // verhuizen naar elke testpas, daarna gaan tabel én schakelaar weg: één lijst op één
    // plaats, geen tweede systeem dat stil naast het nieuwe blijft staan.
    let oude_lijst = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'pass_allow'",
            [],
            |_| Ok(()),
        )
        .optional()
        .unwrap_or(None)
        .is_some();
    if oude_lijst {
        conn.execute(
            "INSERT OR IGNORE INTO item_allow (item_id, user_id, username, added)
               SELECT i.id, a.user_id, a.username, a.added
                 FROM items i, pass_allow a WHERE i.test_pass = 1",
            [],
        )
        .ok();
        conn.execute("DROP TABLE pass_allow", []).ok();
        conn.execute("DELETE FROM settings WHERE key = 'pass_allowlist_on'", []).ok();
    }
    // Voorloper van `stock` (2026-07-15, één sessie geleefd): een vinkje dat na élke
    // aankoop Out of stock aanzette. Vervangen door een echte teller — die toont de speler
    // ook wát er nog is. Kolom weg, anders staan er twee mechanismen naast elkaar.
    if column_exists(&conn, "items", "auto_sold_out") {
        conn.execute("ALTER TABLE items DROP COLUMN auto_sold_out", []).ok();
    }
    ensure_column(&conn, "inventory", "item_id", "INTEGER NOT NULL DEFAULT 0");
    ensure_column(&conn, "role_grants", "label", "TEXT NOT NULL DEFAULT ''");
    // Refund-vlag op shop-aankopen in het logboek: 0 = nog terug te draaien, 1 = al gerefund.
    ensure_column(&conn, "server_log", "refunded", "INTEGER NOT NULL DEFAULT 0");
    // Hytale-tickets zijn boosts (voor de Boosts-tab).
    conn.execute(
        "UPDATE items SET category='boost' WHERE name IN ('Hytale Day Pass','Hytale Permanent Pass')",
        [],
    )
    .ok();
    // Categorie-model vereenvoudigd naar: 'inventory' (verzamelbaar → kaart in de Inventory),
    // 'noninv' (gewoon shop-item, geen kaart) en 'boost' (Hytale-pas). Alle oude categorieën
    // (gem-categorieën primary/secondary/prism + het lege 'plain') worden verzameldbaar.
    // Idempotent: draait enkel op nog niet-gemigreerde rijen.
    conn.execute(
        "UPDATE items SET category='inventory' WHERE category NOT IN ('boost','noninv','inventory','booster')",
        [],
    )
    .ok();
    // Lucky Horseshoe is ALTIJD een 'booster'. Sinds 2026-07-17 is dat een PERMANENT
    // verzamel-item (koop 1×, grey-out zoals de gems op de Boosters-tab); bezit = altijd
    // dubbele chest-kans, geen Use en niets te verbruiken. Fix zowel de oude 'inventory'-
    // migratie ALS een handmatige mis-configuratie naar 'boost' — die laatste is de Hytale-
    // pás-categorie: met duration=0 zou kopen van een hoefijzer anders permanente Hytale-
    // toegang geven! Enkel de categorie wordt gecorrigeerd; prijs/afbeelding blijven behouden.
    conn.execute(
        "UPDATE items SET category='booster' WHERE name='Lucky Horseshoe' AND category != 'booster'",
        [],
    )
    .ok();
    // Oude auto-seed gem-schappen opruimen (vervangen door de gem-catalogus).
    conn.execute(
        "DELETE FROM items WHERE shelf_id IN
           (SELECT id FROM shelves WHERE title IN ('Yellow Gems','Red Gems','Blue Gems','Green Gems'))",
        [],
    )
    .ok();
    conn.execute(
        "DELETE FROM shelves WHERE title IN ('Yellow Gems','Red Gems','Blue Gems','Green Gems')",
        [],
    )
    .ok();
    // max_balance = hoogste saldo ooit, dus dit is een echte invariant: veilig bij elke start.
    conn.execute("UPDATE coins SET max_balance = coins WHERE max_balance < coins", [])
        .expect("backfill max_balance");
    // EENMALIGE migratie (toen `total_earned` als kolom bijkwam en niet te reconstrueren was:
    // als ondergrens het hoogste saldo ooit). Draaide vroeger bij ELKE start, en dat was een
    // lek: een refund verhoogt `coins` zonder verdiensten, `max_balance` volgt dat saldo, en
    // de eerstvolgende herstart promoveerde die refund dan stil tot "all-time verdiend" —
    // waar het levelsysteem op draait. Nu gated op user_version, dus enkel op een DB die de
    // migratie nog nooit zag.
    let migrated: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap_or(0);
    if migrated < 1 {
        conn.execute(
            "UPDATE coins SET total_earned = max_balance WHERE total_earned < max_balance",
            [],
        )
        .expect("backfill total_earned");
        conn.execute("PRAGMA user_version = 1", []).expect("set user_version");
    }
    drop(conn);
    drop_legacy_hytale_shelf(&pool);
    // seed_gems is bewust NIET meer aangeroepen: items worden nu manueel beheerd in Manage
    // Shop, en de categorie-migratie hierboven zou een re-seed telkens naar 'inventory'
    // omzetten. Bestaande (geseede + eigen) items blijven gewoon staan.
    seed_horseshoe(&pool);
    seed_weights(&pool);
    pool
}

/// Seed de twee weegtabellen als ze leeg zijn — enkel dán, zodat een admin een
/// rij mag wegdoen zonder dat de volgende start hem terugzet. Een tabel volledig
/// leegmaken is wél een re-seed: dat is de "geef me de standaardverdeling terug"-weg.
fn seed_weights(pool: &DbPool) {
    let conn = pool.get().expect("db");
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM coin_weights", [], |r| r.get(0)).unwrap_or(0);
    if n == 0 {
        // Coins per bericht: 0..3 even waarschijnlijk, 4 half zo vaak, 5 een tiende
        // (user-beslissing 2026-07-17). Gewichten zijn RELATIEF — som 4.6, geen 100.
        for (amount, weight) in [(0, 1.0), (1, 1.0), (2, 1.0), (3, 1.0), (4, 0.5), (5, 0.1)] {
            conn.execute(
                "INSERT INTO coin_weights (amount, weight) VALUES (?1, ?2)",
                params![amount as i64, weight as f64],
            )
            .expect("seed coin_weights");
        }
    }
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM chest_tiers", [], |r| r.get(0)).unwrap_or(0);
    if n == 0 {
        // De 10-tier chest-verdeling zoals ze sinds 2026-07-14 live stond, 1:1
        // overgenomen uit de oude CHEST_TIERS-const (gewichten waren ‰ van 1000).
        let tiers: [(f64, i64, i64); 10] = [
            (400.0, 50, 80),
            (220.0, 80, 120),
            (140.0, 120, 180),
            (90.0, 180, 260),
            (60.0, 260, 360),
            (40.0, 360, 480),
            (25.0, 480, 620),
            (15.0, 620, 760),
            (7.0, 760, 880),
            (3.0, 880, 1000),
        ];
        for (i, (weight, lo, hi)) in tiers.iter().enumerate() {
            conn.execute(
                "INSERT INTO chest_tiers (weight, lo, hi, position) VALUES (?1, ?2, ?3, ?4)",
                params![weight, lo, hi, i as i64],
            )
            .expect("seed chest_tiers");
        }
    }
}

/// Seed de Lucky Horseshoe (idempotent op naam) op een eigen 'Boosters'-schap.
fn seed_horseshoe(pool: &DbPool) {
    let conn = pool.get().expect("db");
    let exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM items WHERE name = 'Lucky Horseshoe'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if exists > 0 {
        return;
    }
    conn.execute(
        "INSERT INTO shelves (title, position) VALUES ('Boosters', 20)",
        [],
    )
    .expect("seed booster shelf");
    let shelf_id = conn.last_insert_rowid();
    // shop_weight 2 tegen de 10 van een gem: dezelfde zeldzaamheid als de migratie op een
    // bestaande database zet, zodat een verse DB en prod niet uiteenlopen.
    conn.execute(
        "INSERT INTO items (zone, shelf_id, name, price, color, category, description, position,
                            shop_weight)
         VALUES ('shelf', ?1, 'Lucky Horseshoe', 7777, '#c9a227', 'booster',
                 'You will have twice as much chance to open Fortuna''s Favor.', 0, 2.0)",
        params![shelf_id],
    )
    .expect("seed horseshoe");
}

/// De opgeslagen dagselectie voor `day`, in trekkingsvolgorde. ORDER BY rowid = insertie- =
/// (random) trekkingsvolgorde; zonder dit gebruikt SQLite de PK-index (day, item_id) → gesorteerd
/// op item_id, waardoor de shop bij elke her-lees "geherordend" lijkt i.p.v. random.
fn daily_ids(conn: &rusqlite::Connection, day: i64) -> Vec<i64> {
    let mut stmt = conn
        .prepare("SELECT item_id FROM daily_shop WHERE day = ?1 ORDER BY rowid")
        .expect("prepare daily_shop");
    stmt.query_map(params![day], |r| r.get::<_, i64>(0))
        .expect("query daily_shop")
        .filter_map(Result::ok)
        .collect()
}

/// De meedoende items van de dagrotatie: (id, gewicht), op id. Meedoen vergt de vlag
/// `in_rotation` **en** een gewicht > 0 — een gewicht van 0 zou anders een item opleveren
/// dat wel meetelt in de lijst maar nooit getrokken wordt (en een deling door nul in de
/// kansberekening als álles op 0 staat).
pub fn rotation_pool(pool: &DbPool) -> Vec<(i64, f64)> {
    let conn = pool.get().expect("db");
    let mut stmt = conn
        .prepare(
            "SELECT id, shop_weight FROM items
              WHERE in_rotation = 1 AND shop_weight > 0 ORDER BY id",
        )
        .expect("prepare rotation_pool");
    stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?)))
        .expect("query rotation_pool")
        .filter_map(Result::ok)
        .collect()
}

/// Trek `n` verschillende items uit `pool` (id, gewicht), gewogen en zonder teruglegging.
///
/// Methode = de **exponentiële race** (Efraimidis–Spirakis): elk item krijgt een sleutel
/// `-ln(u)/w` met u uniform in (0,1] en de `n` kleinste sleutels winnen. Dat is aantoonbaar
/// hetzelfde als "trek er één op gewicht, haal hem eruit, trek de volgende" — maar in één
/// pass, en het is de vorm waarvoor `rotation_odds` een exacte kans kan uitrekenen.
fn draw_weighted(items: &[(i64, f64)], n: usize) -> Vec<i64> {
    let mut rng = rand::thread_rng();
    let mut keyed: Vec<(f64, i64)> = items
        .iter()
        .filter(|(_, w)| *w > 0.0)
        .map(|(id, w)| {
            // gen_range is [0,1), dus 1.0 - x ligt in (0,1] → ln() nooit op 0.
            let u: f64 = 1.0 - rng.gen_range(0.0f64..1.0);
            (-u.ln() / w, *id)
        })
        .collect();
    keyed.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    keyed.truncate(n);
    keyed.into_iter().map(|(_, id)| id).collect()
}

/// De kans dat elk item **vandaag in de shop staat** (dus: bij de `n` getrokken slots zit),
/// in dezelfde volgorde als `weights`. Niet hetzelfde getal als het aandeel `w/Σw`: er worden
/// `n` slots uit dezelfde pot getrokken, dus een item met 10% aandeel staat er véél vaker dan
/// 10% van de dagen. Dít is het getal waarop een admin stuurt ("hoe vaak zie ik dit?").
///
/// Exact gerekend, niet bemonsterd. Via de race-vorm uit `draw_weighted`: item *i* zit in de
/// selectie zodra hoogstens `n-1` anderen een kleinere sleutel trekken. Conditioneel op de
/// sleutel van *i* zijn die "anderen" onderling onafhankelijk, en met de substitutie
/// `u = e^{-w_i·t}` valt de exponentiële vorm helemaal weg:
/// `P(j sneller dan i) = 1 - u^(w_j/w_i)`. Wat overblijft is een integraal over u ∈ [0,1] van
/// een Poisson-binomiale staartkans, die met Gauss–Legendre in één pass wordt uitgerekend.
/// (Getoetst tegen een simulatie van de échte trekking, zie `mod rotation_odds_tests`.)
pub fn rotation_odds(weights: &[f64], n: usize) -> Vec<f64> {
    let live: Vec<f64> = weights.iter().map(|w| w.max(0.0)).collect();
    let meedoen = live.iter().filter(|w| **w > 0.0).count();
    // Passen alle meedoende items in de slots, dan staat elk van hen er sowieso.
    if meedoen <= n {
        return live.iter().map(|w| if *w > 0.0 { 1.0 } else { 0.0 }).collect();
    }
    if n == 0 {
        return vec![0.0; live.len()];
    }
    live.iter()
        .map(|wi| {
            if *wi <= 0.0 {
                return 0.0;
            }
            // De integrand: de kans dat hoogstens n-1 anderen sneller zijn, gegeven u.
            let integrand = |u: f64| {
                // Poisson-binomiaal: de verdeling van "aantal snellere items", afgekapt
                // op n (verder tellen hoeft niet — alles daarboven is toch verlies).
                let mut dist = vec![0.0f64; n + 1];
                dist[0] = 1.0;
                for wj in live.iter() {
                    if std::ptr::eq(wj, wi) || *wj <= 0.0 {
                        continue;
                    }
                    let p = 1.0 - u.powf(wj / wi); // kans dat j sneller is dan i
                    for k in (1..=n).rev() {
                        dist[k] = dist[k] * (1.0 - p) + dist[k - 1] * p;
                    }
                    dist[0] *= 1.0 - p;
                }
                dist[..n].iter().sum::<f64>() // hoogstens n-1 sneller
            };
            // Samengestelde Simpson over [0,1]. Ruim genomen (1024 panelen): bij sterk
            // uiteenlopende gewichten wordt de integrand vlak bij u = 0 steil, en dat is
            // net waar een te grove stap zichtbaar zou gaan afwijken.
            const PANELEN: usize = 1024;
            let h = 1.0 / PANELEN as f64;
            let mut acc = integrand(0.0) + integrand(1.0);
            for k in 1..PANELEN {
                let coef = if k % 2 == 1 { 4.0 } else { 2.0 };
                acc += coef * integrand(k as f64 * h);
            }
            (acc * h / 3.0).clamp(0.0, 1.0)
        })
        .collect()
}

/// De dagelijkse shop-selectie: `n` items voor `day`, stabiel bewaard in `daily_shop`.
/// Pool + verhoudingen komen volledig uit de items zelf (`in_rotation` + `shop_weight`),
/// live te regelen in Manage → Shop. Wie een groter gewicht heeft, verschijnt vaker; wie
/// niet meedoet (bv. de Hytale-passen, die staan al permanent te koop) blijft eruit.
/// De selectie is voor iedereen dezelfde — dat maakt verzamelen spannender.
pub fn shop_offers(pool: &DbPool, day: i64, n: i64) -> Vec<Item> {
    let conn = pool.get().expect("db");
    // Snelle weg: bestaat de dagselectie al, lees ze lockloos (de shop wordt vaak herladen).
    let ids = daily_ids(&conn, day);
    if !ids.is_empty() {
        return ids.iter().filter_map(|id| get_item(pool, *id)).collect();
    }
    drop(conn);
    // Doet er niets mee (alles uitgevinkt of op gewicht 0), dan valt er niets te trekken en
    // ook niets op te slaan. Zonder deze uitweg zou élke paginaweergave opnieuw een lege
    // trekking onder een schrijf-lock proberen, want er komt nooit iets in `daily_shop`.
    if rotation_pool(pool).is_empty() {
        return Vec::new();
    }

    // Trage weg: nieuwe dag → trek + schrijf ONDER een write-lock, zodat twee gelijktijdige
    // eerste-bezoekers niet elk een eigen set rollen en er een mengeling (mogelijk > n items)
    // gepersisteerd wordt. Her-check binnen de lock: won een ander net de race, neem díe set over.
    let mut conn = pool.get().expect("db");
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .expect("tx daily_shop");
    let final_ids = {
        let existing = daily_ids(&tx, day);
        if !existing.is_empty() {
            existing // een gelijktijdige aanroep was ons voor
        } else {
            // Gewogen trekking uit de meedoende items — binnen dezelfde transactie
            // gelezen, zodat een gewichtswijziging tijdens de trekking er niet half
            // tussen kan vallen.
            let mut stmt = tx
                .prepare(
                    "SELECT id, shop_weight FROM items
                      WHERE in_rotation = 1 AND shop_weight > 0 ORDER BY id",
                )
                .expect("prepare rotation pool");
            let kandidaten: Vec<(i64, f64)> = stmt
                .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?)))
                .expect("query rotation pool")
                .filter_map(Result::ok)
                .collect();
            drop(stmt);
            let ids = draw_weighted(&kandidaten, n.max(0) as usize);
            for id in &ids {
                tx.execute(
                    "INSERT OR IGNORE INTO daily_shop (day, item_id) VALUES (?1, ?2)",
                    params![day, id],
                )
                .expect("insert daily_shop");
            }
            // Lees de canoniek opgeslagen set terug (wat getoond wordt == wat opgeslagen is).
            daily_ids(&tx, day)
        }
    };
    tx.commit().expect("commit daily_shop");
    final_ids.iter().filter_map(|id| get_item(pool, *id)).collect()
}

/// Gooi de dagselectie van `day` weg; de eerstvolgende `shop_offers` trekt opnieuw.
/// (Admin-knopje naast de dagitems — handig om te testen zonder een dag te wachten.)
pub fn clear_shop_day(pool: &DbPool, day: i64) {
    let conn = pool.get().expect("db");
    conn.execute("DELETE FROM daily_shop WHERE day = ?1", params![day])
        .expect("clear daily_shop");
}

/// De Hytale-passen (dagpas + permanent). Staan altijd te koop, los van de dagrotatie.
/// Dagpas eerst (duration > 0), dan de permanente.
pub fn boost_items(pool: &DbPool) -> Vec<Item> {
    let conn = pool.get().expect("db");
    let mut stmt = conn
        .prepare("SELECT id FROM items WHERE category = 'boost' ORDER BY duration DESC, id")
        .expect("prepare boost_items");
    let ids: Vec<i64> = stmt
        .query_map([], |r| r.get::<_, i64>(0))
        .expect("query boost_items")
        .filter_map(Result::ok)
        .collect();
    ids.iter().filter_map(|id| get_item(pool, *id)).collect()
}

// --- namenlijst per item (de Test Pass) -----------------------------------
//
// Eén item kan een eigen namenlijst hebben: staat er iemand op, dan kunnen enkel die
// leden het kopen. Vandaag gebruikt de Test Pass dit; de gewone Meadowland Pass heeft
// geen lijst en staat dus voor iedereen te koop. De lijst zelf is de regel — er is geen
// aparte schakelaar die haar aan of uit zet.

/// De leden op de lijst van één item: (uid, Discord-naam, Hytale-naam), alfabetisch op
/// Discord-naam (NOCASE). De naam komt bij voorkeur uit `coins` (die volgt hernoemingen
/// op Discord); wat bij het toevoegen bewaard werd, is de terugval voor een uid die niet
/// (meer) in `coins` staat.
pub fn item_allow_list(pool: &DbPool, item_id: i64) -> Vec<(String, String, String)> {
    let conn = pool.get().expect("db");
    let mut stmt = conn
        .prepare(
            "SELECT a.user_id,
                    COALESCE(NULLIF(c.username, ''), NULLIF(a.username, ''), a.user_id),
                    COALESCE(c.hytale_name, '')
               FROM item_allow a LEFT JOIN coins c ON c.user_id = a.user_id
              WHERE a.item_id = ?1
              ORDER BY 2 COLLATE NOCASE",
        )
        .expect("prepare item_allow_list");
    stmt.query_map(params![item_id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
    })
    .expect("query item_allow_list")
    .filter_map(Result::ok)
    .collect()
}

/// Zet één lid op de lijst van dit item. `false` = niets gedaan (lege uid, of stond er
/// al op) — de GUI toont dan geen "toegevoegd"-melding die niet klopt.
pub fn item_allow_add(pool: &DbPool, item_id: i64, uid: &str, username: &str, ts: f64) -> bool {
    let uid = uid.trim();
    if uid.is_empty() {
        return false;
    }
    let conn = pool.get().expect("db");
    conn.execute(
        "INSERT OR IGNORE INTO item_allow (item_id, user_id, username, added)
           VALUES (?1, ?2, ?3, ?4)",
        params![item_id, uid, username.trim(), ts],
    )
    .map(|n| n > 0)
    .unwrap_or(false)
}

/// Haal één lid van de lijst van dit item. `false` = stond er niet op.
pub fn item_allow_remove(pool: &DbPool, item_id: i64, uid: &str) -> bool {
    let conn = pool.get().expect("db");
    conn.execute(
        "DELETE FROM item_allow WHERE item_id = ?1 AND user_id = ?2",
        params![item_id, uid.trim()],
    )
    .map(|n| n > 0)
    .unwrap_or(false)
}

/// Staat dit lid op de lijst van dit item?
pub fn item_allow_has(pool: &DbPool, item_id: i64, uid: &str) -> bool {
    let conn = pool.get().expect("db");
    conn.query_row(
        "SELECT 1 FROM item_allow WHERE item_id = ?1 AND user_id = ?2",
        params![item_id, uid.trim()],
        |_| Ok(()),
    )
    .optional()
    .unwrap_or(None)
    .is_some()
}

/// Noteer welke testpas dit lid net kocht, met de stand van zijn speeltijd-teller erbij.
/// Eén rij per lid: een nieuwe aankoop vervangt de vorige (die is dan per definitie
/// opgebrand, anders had hij niet kunnen kopen).
pub fn test_pass_hold_set(
    pool: &DbPool,
    uid: &str,
    item_id: i64,
    hytale_name: &str,
    used_at: f64,
    duration: i64,
    ts: f64,
) {
    let conn = pool.get().expect("db");
    conn.execute(
        "INSERT INTO test_pass_hold (user_id, item_id, name_lc, used_at, duration, bought)
              VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(user_id) DO UPDATE SET
              item_id = ?2, name_lc = ?3, used_at = ?4, duration = ?5, bought = ?6",
        params![uid.trim(), item_id, hytale_name.trim().to_lowercase(), used_at, duration, ts],
    )
    .ok();
}

/// De testpas die dit lid nog moet opbranden: (Hytale-naam in kleine letters, stand van
/// de speeltijd-teller bij de aankoop, duur in seconden). None = hij kocht er nog nooit een.
pub fn test_pass_hold_get(pool: &DbPool, uid: &str) -> Option<(String, f64, i64)> {
    let conn = pool.get().expect("db");
    conn.query_row(
        "SELECT name_lc, used_at, duration FROM test_pass_hold WHERE user_id = ?1",
        params![uid.trim()],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?, r.get::<_, i64>(2)?)),
    )
    .optional()
    .unwrap_or(None)
}

/// De keuzelijst voor zo'n namenlijst: elk gekend lid **met een Hytale-naam**, als
/// (uid, Discord-naam, Hytale-naam), alfabetisch. Zonder Hytale-naam valt er niets te
/// whitelisten, dus zo iemand hoort niet in de keuzelijst van een pas te staan.
pub fn members_with_hytale_name(pool: &DbPool) -> Vec<(String, String, String)> {
    let conn = pool.get().expect("db");
    let mut stmt = conn
        .prepare(
            "SELECT user_id, COALESCE(NULLIF(username, ''), user_id), hytale_name FROM coins
              WHERE hytale_name <> '' ORDER BY 2 COLLATE NOCASE",
        )
        .expect("prepare members_with_hytale_name");
    stmt.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
    })
    .expect("query members_with_hytale_name")
    .filter_map(Result::ok)
    .collect()
}

/// Seed de gem-catalogus één keer (idempotent): 3 primary, 5 secondary, 5 prism.
/// Elke gem is een shop-item met een categorie, kleur en omschrijving.
/// NIET meer aangeroepen (items worden manueel beheerd); bewaard als referentie.
#[allow(dead_code)]
fn seed_gems(pool: &DbPool) {
    let conn = pool.get().expect("db");
    let exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM items WHERE category IN ('primary','secondary','prism')",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if exists > 0 {
        return;
    }
    // (schaptitel, categorie, [(naam, kleur, prijs, uitleg)])
    let cats: [(&str, &str, &[(&str, &str, i64, &str)]); 3] = [
        (
            "Primary Gems",
            "primary",
            &[
                ("Ruby", "#d1543f", 40, "A fiery red primary gem."),
                ("Topaz", "#e8c34a", 40, "A sunny yellow primary gem."),
                ("Sapphire", "#4a86e8", 40, "A deep blue primary gem."),
            ],
        ),
        (
            "Secondary Gems",
            "secondary",
            &[
                ("Emerald", "#2ecc71", 80, "A lush green secondary gem."),
                ("Amethyst", "#9b59b6", 80, "A royal purple secondary gem."),
                ("Citrine", "#f39c12", 80, "A warm amber secondary gem."),
                ("Garnet", "#c0392b", 80, "A dark crimson secondary gem."),
                ("Onyx", "#34495e", 80, "A shadowy slate secondary gem."),
            ],
        ),
        (
            "Prism Gems",
            "prism",
            &[
                ("Prism Aurora", "#e056fd", 160, "A shifting magenta prism gem."),
                ("Prism Frost", "#7ed6df", 160, "An icy cyan prism gem."),
                ("Prism Ember", "#ff7979", 160, "A glowing coral prism gem."),
                ("Prism Verdant", "#badc58", 160, "A vivid lime prism gem."),
                ("Prism Dusk", "#686de0", 160, "A twilight indigo prism gem."),
            ],
        ),
    ];
    for (pos, (title, cat, gems)) in cats.iter().enumerate() {
        conn.execute(
            "INSERT INTO shelves (title, position) VALUES (?1, ?2)",
            params![title, pos as i64 + 10],
        )
        .expect("seed gem shelf");
        let shelf_id = conn.last_insert_rowid();
        for (i, (name, color, price, desc)) in gems.iter().enumerate() {
            conn.execute(
                "INSERT INTO items (zone, shelf_id, name, price, color, category, description, position)
                 VALUES ('shelf', ?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![shelf_id, name, price, color, cat, desc, i as i64],
            )
            .expect("seed gem");
        }
    }
}

/// Ruimt het oude, geseede schap **Hytale Access** op (eenmalig, gemarkeerd in
/// `settings`). Vroeger zette een seeder dat schap met 'Hytale Day Pass' +
/// 'Hytale Permanent Pass' bij elke start terug zodra die twee namen weg waren —
/// dus ook nadat de admin ze in Manage Shop had verwijderd of hernoemd. De passen
/// worden nu manueel beheerd (zoals de gems, zie `seed_gems`), dus het schap moet
/// definitief weg kunnen. Enkel niet-bezeten seed-items sneuvelen; het schap zelf
/// verdwijnt pas als het daarna leeg is.
fn drop_legacy_hytale_shelf(pool: &DbPool) {
    let conn = pool.get().expect("db");
    let done: Option<String> = conn
        .query_row("SELECT value FROM settings WHERE key = 'hytale_shelf_dropped_v1'", [], |r| {
            r.get(0)
        })
        .optional()
        .unwrap_or(None);
    if done.is_some() {
        return;
    }
    conn.execute(
        "DELETE FROM items
          WHERE name IN ('Hytale Day Pass','Hytale Permanent Pass')
            AND shelf_id IN (SELECT id FROM shelves WHERE title = 'Hytale Access')
            AND id NOT IN (SELECT item_id FROM inventory)",
        [],
    )
    .ok();
    conn.execute(
        "DELETE FROM shelves
          WHERE title = 'Hytale Access'
            AND id NOT IN (SELECT shelf_id FROM items WHERE shelf_id IS NOT NULL)",
        [],
    )
    .ok();
    conn.execute(
        "INSERT INTO settings (key, value) VALUES ('hytale_shelf_dropped_v1', '1')
         ON CONFLICT(key) DO UPDATE SET value = '1'",
        [],
    )
    .ok();
}

/// Bestaat kolom `col` in `table`? (voor idempotente migraties.)
fn column_exists(conn: &rusqlite::Connection, table: &str, col: &str) -> bool {
    let Ok(mut stmt) = conn.prepare(&format!("PRAGMA table_info({table})")) else {
        return false;
    };
    let names: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default();
    names.iter().any(|n| n == col)
}

/// Voeg een kolom toe als hij nog niet bestaat (SQLite ALTER TABLE ADD COLUMN).
fn ensure_column(conn: &rusqlite::Connection, table: &str, col: &str, decl: &str) {
    if !column_exists(conn, table, col) {
        conn.execute(&format!("ALTER TABLE {table} ADD COLUMN {col} {decl}"), [])
            .expect("kan kolom niet toevoegen");
    }
}

/// Bewaar een nieuwe login-sessie (cookie-token → Discord-identiteit).
pub fn create_session(pool: &DbPool, token: &str, user_id: &str, username: &str, ts: f64) {
    let conn = pool.get().expect("db");
    conn.execute(
        "INSERT OR REPLACE INTO sessions (token, user_id, username, created)
         VALUES (?1, ?2, ?3, ?4)",
        params![token, user_id, username, ts],
    )
    .expect("insert session");
}

/// (user_id, username) horend bij een sessie-token, mits ze niet verlopen is: `created` moet
/// binnen `max_age` seconden van `now` liggen. Een verlopen sessie wordt meteen opgeruimd
/// (server-side TTL — een gelekt token blijft zo niet eeuwig bruikbaar, i.t.t. een cookie-Max-Age
/// die enkel de client stuurt).
pub fn get_session(pool: &DbPool, token: &str, now: f64, max_age: f64) -> Option<(String, String)> {
    let conn = pool.get().expect("db");
    let row: Option<(String, String, f64)> = conn
        .query_row(
            "SELECT user_id, username, created FROM sessions WHERE token = ?1",
            params![token],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, f64>(2)?)),
        )
        .optional()
        .expect("query session");
    match row {
        Some((uid, name, created)) if created >= now - max_age => Some((uid, name)),
        Some(_) => {
            let _ = conn.execute("DELETE FROM sessions WHERE token = ?1", params![token]);
            None
        }
        None => None,
    }
}

pub fn delete_session(pool: &DbPool, token: &str) {
    let conn = pool.get().expect("db");
    conn.execute("DELETE FROM sessions WHERE token = ?1", params![token])
        .expect("delete session");
}

/// Unix-tijdstip van de laatste toekenning (0.0 als de user nog niks heeft).
pub fn get_last_award(pool: &DbPool, user_id: &str) -> f64 {
    let conn = pool.get().expect("db");
    conn.query_row(
        "SELECT last_award FROM coins WHERE user_id = ?1",
        params![user_id],
        |r| r.get(0),
    )
    .optional()
    .expect("query last_award")
    .unwrap_or(0.0)
}

/// Tel `amount` coins bij, update username + last_award, houd max_balance bij.
/// Returnt het nieuwe totaal.
pub fn award(pool: &DbPool, user_id: &str, username: &str, amount: i64, ts: f64) -> i64 {
    let conn = pool.get().expect("db");
    conn.execute(
        "INSERT INTO coins (user_id, username, coins, last_award, max_balance, total_earned)
         VALUES (?1, ?2, ?3, ?4, ?3, ?3)
         ON CONFLICT(user_id) DO UPDATE SET
             coins        = coins + excluded.coins,
             username     = excluded.username,
             last_award   = excluded.last_award,
             max_balance  = MAX(max_balance, coins + excluded.coins),
             total_earned = total_earned + excluded.coins",
        params![user_id, username, amount, ts],
    )
    .expect("insert award");
    log_earn_event(&conn, user_id, amount, ts);
    conn.query_row(
        "SELECT coins FROM coins WHERE user_id = ?1",
        params![user_id],
        |r| r.get(0),
    )
    .expect("query totaal")
}

/// Bericht-award met **atomische cooldown-guard**: boekt enkel als `last_award <= guard_ts`
/// (= de cooldown is écht verstreken). Vangt de race van twee snelle berichten die beide de
/// Rust-cooldowncheck passeren vóór er geschreven is → geen dubbele award. `guard_ts` =
/// `now - cooldown`. Voor een gloednieuw lid vuurt de INSERT (geen conflict) → eerste bericht
/// boekt altijd. Returnt `Some(nieuw_saldo)` bij een geboekte award, `None` als de guard weigerde.
pub fn award_if_ready(
    pool: &DbPool,
    user_id: &str,
    username: &str,
    amount: i64,
    ts: f64,
    guard_ts: f64,
) -> Option<i64> {
    let conn = pool.get().expect("db");
    let changed = conn
        .execute(
            "INSERT INTO coins (user_id, username, coins, last_award, max_balance, total_earned)
         VALUES (?1, ?2, ?3, ?4, ?3, ?3)
         ON CONFLICT(user_id) DO UPDATE SET
             coins        = coins + excluded.coins,
             username     = excluded.username,
             last_award   = excluded.last_award,
             max_balance  = MAX(max_balance, coins + excluded.coins),
             total_earned = total_earned + excluded.coins
         WHERE last_award <= ?5",
            params![user_id, username, amount, ts, guard_ts],
        )
        .expect("insert award");
    if changed == 0 {
        return None; // race verloren: een gelijktijdig bericht boekte net vóór dit
    }
    log_earn_event(&conn, user_id, amount, ts);
    let total = conn
        .query_row(
            "SELECT coins FROM coins WHERE user_id = ?1",
            params![user_id],
            |r| r.get(0),
        )
        .expect("query totaal");
    Some(total)
}

/// Log één verdienste in earn_log (voor het "≥100 coins dit uur"-overzicht).
fn log_earn_event(conn: &rusqlite::Connection, user_id: &str, amount: i64, ts: f64) {
    let _ = conn.execute(
        "INSERT INTO earn_log (user_id, amount, ts) VALUES (?1, ?2, ?3)",
        params![user_id, amount, ts],
    );
}

/// Verdieners van ≥`min` coins in het venster [since, until): (user_id, naam, totaal),
/// aflopend, hoogstens `limit` rijen. Gebruikt voor het uurlijkse top-embed.
pub fn hourly_earners(
    pool: &DbPool,
    since: f64,
    until: f64,
    min: i64,
    limit: i64,
) -> Vec<(String, String, i64)> {
    let conn = pool.get().expect("db");
    let mut stmt = conn
        .prepare(
            "SELECT e.user_id, COALESCE(c.username, e.user_id), SUM(e.amount) AS total
             FROM earn_log e LEFT JOIN coins c ON c.user_id = e.user_id
             WHERE e.ts >= ?1 AND e.ts < ?2
             GROUP BY e.user_id
             HAVING total >= ?3
             ORDER BY total DESC
             LIMIT ?4",
        )
        .expect("prepare hourly_earners");
    let rows = stmt
        .query_map(params![since, until, min, limit], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))
        })
        .expect("query hourly_earners");
    rows.filter_map(|r| r.ok()).collect()
}

/// Ruim earn_log-rijen ouder dan `before` op (rollend venster hoeft niet meer).
pub fn prune_earn_log(pool: &DbPool, before: f64) {
    let conn = pool.get().expect("db");
    let _ = conn.execute("DELETE FROM earn_log WHERE ts < ?1", params![before]);
}

// --- lid-activiteit (Manage → Inactives) --------------------------------

/// Ververs de laatste activiteit van een lid (message of reactie in de prod-guild).
/// `name` leeg → last_seen wordt bijgewerkt maar de bestaande naam blijft (bv. bij een
/// reactie waar we de weergavenaam niet kennen). Upsert, dus altijd goedkoop.
pub fn touch_activity(pool: &DbPool, uid: &str, name: &str, ts: f64) {
    let conn = pool.get().expect("db");
    let _ = conn.execute(
        "INSERT INTO member_activity (user_id, name, last_seen) VALUES (?1, ?2, ?3)
         ON CONFLICT(user_id) DO UPDATE SET last_seen = excluded.last_seen,
           name = CASE WHEN excluded.name != '' THEN excluded.name ELSE member_activity.name END",
        params![uid, name, ts],
    );
}

/// Zet de startklok voor een lid **enkel als het nog niet bestaat** (INSERT OR IGNORE):
/// bij uitrol/CacheReady krijgt elk huidig lid `last_seen = nu`, zonder al gemeten
/// activiteit te overschrijven.
pub fn seed_activity(pool: &DbPool, uid: &str, name: &str, ts: f64) {
    let conn = pool.get().expect("db");
    let _ = conn.execute(
        "INSERT OR IGNORE INTO member_activity (user_id, name, last_seen) VALUES (?1, ?2, ?3)",
        params![uid, name, ts],
    );
}

/// Vrije KV-lees. None = sleutel bestaat niet.
pub fn kv_get(pool: &DbPool, k: &str) -> Option<String> {
    let conn = pool.get().expect("db");
    conn.query_row("SELECT v FROM kv WHERE k = ?1", params![k], |r| r.get(0))
        .ok()
}

/// Vrije KV-schrijf (upsert).
pub fn kv_set(pool: &DbPool, k: &str, v: &str) {
    let conn = pool.get().expect("db");
    let _ = conn.execute(
        "INSERT INTO kv (k, v) VALUES (?1, ?2)
         ON CONFLICT(k) DO UPDATE SET v = excluded.v",
        params![k, v],
    );
}

/// Eén rij in de Inactives-lijst: lid + laatste activiteit + huidig saldo (relevant voor
/// de latere verdeel-kist).
pub struct Inactive {
    pub user_id: String,
    pub name: String,
    pub last_seen: f64,
    pub coins: i64,
}

/// Alle gevolgde leden, **aflopend op afwezigheid** (langst inactief eerst). Saldo komt
/// uit de coins-tabel (0 als er nog geen coins-rij is).
pub fn list_inactives(pool: &DbPool) -> Vec<Inactive> {
    let conn = pool.get().expect("db");
    let mut stmt = conn
        .prepare(
            "SELECT a.user_id, a.name, a.last_seen, COALESCE(c.coins, 0)
               FROM member_activity a LEFT JOIN coins c ON c.user_id = a.user_id
              ORDER BY a.last_seen ASC",
        )
        .expect("prep inactives");
    let rows = stmt
        .query_map([], |r| {
            Ok(Inactive {
                user_id: r.get(0)?,
                name: r.get(1)?,
                last_seen: r.get(2)?,
                coins: r.get(3)?,
            })
        })
        .expect("query inactives");
    rows.filter_map(|r| r.ok()).collect()
}

// --- admin coins-beheer -------------------------------------------------

/// Alle saldi (user_id → coins) uit de coins-tabel.
pub fn all_balances(pool: &DbPool) -> std::collections::HashMap<String, i64> {
    let conn = pool.get().expect("db");
    let mut stmt = conn
        .prepare("SELECT user_id, coins FROM coins")
        .expect("prepare all_balances");
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
        .expect("query all_balances");
    rows.filter_map(|r| r.ok()).collect()
}


/// Zet het saldo op een absolute waarde (startwaarde): coins + total_earned +
/// max_balance = `value`, zodat het lid meteen het juiste level heeft en op het
/// All-time-leaderboard verschijnt. Returnt het vorige saldo.
/// Alle all-time-verdiensten (user_id → total_earned).
pub fn all_earned(pool: &DbPool) -> std::collections::HashMap<String, i64> {
    let conn = pool.get().expect("db");
    let mut stmt = conn
        .prepare("SELECT user_id, total_earned FROM coins")
        .expect("prepare all_earned");
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
        .expect("query all_earned");
    rows.filter_map(|r| r.ok()).collect()
}

/// Pas het saldo aan. `set`=true → zet op `val`; anders tel `val` erbij. `current`
/// raakt `coins`, `alltime` raakt `total_earned` (naar keuze beide). `max_balance`
/// volgt het (mogelijk nieuwe) saldo. Returnt (vorig coins, vorig total_earned).
pub fn admin_adjust(
    pool: &DbPool,
    user_id: &str,
    username: &str,
    val: i64,
    set: bool,
    current: bool,
    alltime: bool,
) -> (i64, i64) {
    let mut conn = pool.get().expect("db");
    // IMMEDIATE: neem de write-lock meteen zodat de read-modify-write (SELECT → bereken → UPDATE)
    // atomisch is. Zonder deze transactie gaat een gelijktijdige award tussen de SELECT en de
    // UPDATE verloren (lost update): de absolute write zou het net-verdiende bedrag overschrijven.
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .expect("tx adjust");
    let (pc, pe): (i64, i64) = tx
        .query_row(
            "SELECT coins, total_earned FROM coins WHERE user_id = ?1",
            params![user_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .expect("q adjust")
        .unwrap_or((0, 0));
    let coins = if current {
        if set { val } else { pc + val }
    } else {
        pc
    };
    let earned = if alltime {
        if set { val } else { pe + val }
    } else {
        pe
    };
    tx.execute(
        "INSERT INTO coins (user_id, username, coins, total_earned, max_balance) VALUES (?1, ?2, ?3, ?4, ?3)
         ON CONFLICT(user_id) DO UPDATE SET
             coins = ?3, total_earned = ?4,
             username = excluded.username,
             max_balance = MAX(max_balance, ?3)",
        params![user_id, username, coins, earned],
    )
    .expect("admin adjust");
    tx.commit().expect("commit adjust");
    (pc, pe)
}

/// Bewaar de laatste ingreep (enige rij) zodat ze ongedaan gemaakt kan worden.
pub fn admin_record_undo(
    pool: &DbPool,
    user_id: &str,
    username: &str,
    prev_coins: i64,
    prev_earned: i64,
) {
    let conn = pool.get().expect("db");
    conn.execute(
        "INSERT INTO admin_undo (id, user_id, username, prev_coins, prev_earned) VALUES (1, ?1, ?2, ?3, ?4)
         ON CONFLICT(id) DO UPDATE SET
             user_id = excluded.user_id,
             username = excluded.username,
             prev_coins = excluded.prev_coins,
             prev_earned = excluded.prev_earned",
        params![user_id, username, prev_coins, prev_earned],
    )
    .expect("record undo");
}

/// De laatste ongedaan-maakbare ingreep: (user_id, username, prev_coins, prev_earned).
pub fn admin_get_undo(pool: &DbPool) -> Option<(String, String, i64, i64)> {
    let conn = pool.get().expect("db");
    conn.query_row(
        "SELECT user_id, username, prev_coins, prev_earned FROM admin_undo WHERE id = 1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )
    .optional()
    .expect("get undo")
}

/// Maak de laatste ingreep ongedaan: zet coins + total_earned terug en wis de
/// undo-rij. Returnt (username, coins, total_earned) als er iets te herstellen was.
pub fn admin_apply_undo(pool: &DbPool) -> Option<(String, i64, i64)> {
    let (uid, username, pc, pe) = admin_get_undo(pool)?;
    let conn = pool.get().expect("db");
    conn.execute(
        "UPDATE coins SET coins = ?2, total_earned = ?3 WHERE user_id = ?1",
        params![uid, pc, pe],
    )
    .expect("apply undo");
    conn.execute("DELETE FROM admin_undo WHERE id = 1", [])
        .expect("clear undo");
    Some((username, pc, pe))
}

// --- leave/rejoin archief -----------------------------------------------

/// Lid verliet de server: archiveer saldo + all-time en reset beide naar 0
/// (verse start bij terugkeer). Returnt het gearchiveerde saldo, of None als er
/// niets te bewaren viel (0/0 of onbekende user).
pub fn archive_on_leave(pool: &DbPool, user_id: &str, ts: f64) -> Option<i64> {
    let conn = pool.get().expect("db");
    let row: Option<(i64, i64, String)> = conn
        .query_row(
            "SELECT coins, total_earned, username FROM coins WHERE user_id = ?1",
            params![user_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .expect("q leave");
    let (coins, earned, username) = row?;
    if coins == 0 && earned == 0 {
        return None;
    }
    conn.execute(
        "INSERT INTO coin_archive (user_id, coins, total_earned, username, ts) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(user_id) DO UPDATE SET
             coins = excluded.coins, total_earned = excluded.total_earned,
             username = excluded.username, ts = excluded.ts",
        params![user_id, coins, earned, username, ts],
    )
    .expect("archive");
    conn.execute(
        "UPDATE coins SET coins = 0, total_earned = 0, max_balance = 0 WHERE user_id = ?1",
        params![user_id],
    )
    .expect("reset on leave");
    Some(coins)
}

/// Alle archief-rijen: user_id → (coins, total_earned, username).
pub fn all_archives(pool: &DbPool) -> std::collections::HashMap<String, (i64, i64, String)> {
    let conn = pool.get().expect("db");
    let mut stmt = conn
        .prepare("SELECT user_id, coins, total_earned, username FROM coin_archive")
        .expect("prepare archives");
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                (r.get::<_, i64>(1)?, r.get::<_, i64>(2)?, r.get::<_, String>(3)?),
            ))
        })
        .expect("query archives");
    rows.filter_map(|r| r.ok()).collect()
}

/// Geef het gearchiveerde saldo + all-time terug aan het lid en wis het archief.
pub fn restore_archive(pool: &DbPool, user_id: &str) {
    let conn = pool.get().expect("db");
    let row: Option<(i64, i64)> = conn
        .query_row(
            "SELECT coins, total_earned FROM coin_archive WHERE user_id = ?1",
            params![user_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .expect("q restore");
    if let Some((coins, earned)) = row {
        conn.execute(
            "INSERT INTO coins (user_id, username, coins, total_earned, max_balance)
             VALUES (?1, '', ?2, ?3, ?2)
             ON CONFLICT(user_id) DO UPDATE SET
                 coins = ?2, total_earned = ?3, max_balance = MAX(max_balance, ?2)",
            params![user_id, coins, earned],
        )
        .expect("restore");
        conn.execute("DELETE FROM coin_archive WHERE user_id = ?1", params![user_id])
            .expect("del archive");
    }
}

/// Wis het archief van een lid zonder iets terug te geven.
pub fn discard_archive(pool: &DbPool, user_id: &str) {
    let conn = pool.get().expect("db");
    conn.execute("DELETE FROM coin_archive WHERE user_id = ?1", params![user_id])
        .expect("discard archive");
}

// --- coin-kanalen (waar coins verdiend kunnen worden) -------------------

/// Alle kanalen waar coins verdiend mogen worden: (channel_id, name).
pub fn coin_channels(pool: &DbPool) -> Vec<(String, String)> {
    let conn = pool.get().expect("db");
    let mut stmt = conn
        .prepare("SELECT channel_id, name FROM coin_channels ORDER BY name")
        .expect("prepare coin_channels");
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .expect("query coin_channels");
    rows.filter_map(|r| r.ok()).collect()
}

/// Mag er in dit kanaal verdiend worden?
pub fn is_coin_channel(pool: &DbPool, channel_id: u64) -> bool {
    let conn = pool.get().expect("db");
    conn.query_row(
        "SELECT 1 FROM coin_channels WHERE channel_id = ?1",
        params![channel_id.to_string()],
        |_| Ok(()),
    )
    .optional()
    .expect("q coin_channel")
    .is_some()
}

pub fn add_coin_channel(pool: &DbPool, channel_id: &str, name: &str) {
    let conn = pool.get().expect("db");
    conn.execute(
        "INSERT INTO coin_channels (channel_id, name) VALUES (?1, ?2)
         ON CONFLICT(channel_id) DO UPDATE SET name = excluded.name",
        params![channel_id, name],
    )
    .expect("add coin_channel");
}

pub fn remove_coin_channel(pool: &DbPool, channel_id: &str) {
    let conn = pool.get().expect("db");
    conn.execute(
        "DELETE FROM coin_channels WHERE channel_id = ?1",
        params![channel_id],
    )
    .expect("remove coin_channel");
}

// --- treasure-chest cooldowns (overleven een herstart) ------------------

/// Bewaar de chest-cooldown van een kanaal: geen nieuwe chest vóór `until`
/// (epoch-seconden). Idempotent per kanaal.
pub fn set_chest_cooldown(pool: &DbPool, channel_id: u64, until: f64) {
    let conn = pool.get().expect("db");
    conn.execute(
        "INSERT INTO chest_cooldowns (channel_id, until) VALUES (?1, ?2)
         ON CONFLICT(channel_id) DO UPDATE SET until = excluded.until",
        params![channel_id.to_string(), until],
    )
    .expect("set chest cooldown");
}

/// Laad de nog-lopende chest-cooldowns (channel_id → until) en ruim de verlopen
/// rijen meteen op. Wordt bij opstart in de in-memory tracker geladen zodat een
/// herstart de rust per kanaal niet reset.
pub fn load_chest_cooldowns(pool: &DbPool, now: f64) -> std::collections::HashMap<u64, f64> {
    let conn = pool.get().expect("db");
    conn.execute("DELETE FROM chest_cooldowns WHERE until <= ?1", params![now])
        .ok();
    let mut stmt = conn
        .prepare("SELECT channel_id, until FROM chest_cooldowns")
        .expect("prepare load chest cooldowns");
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?)))
        .expect("query chest cooldowns");
    rows.filter_map(Result::ok)
        .filter_map(|(id, until)| id.parse::<u64>().ok().map(|id| (id, until)))
        .collect()
}

// --- weekly leaderboard + Brusselse tijd (EU-DST) -----------------------

/// Weekly leaderboard: (user_id, username, verdiend sinds `since`), aflopend, ≥1.
pub fn leaderboard_week(pool: &DbPool, since: f64, limit: i64) -> Vec<(String, String, i64)> {
    let conn = pool.get().expect("db");
    let mut stmt = conn
        .prepare(
            "SELECT e.user_id, COALESCE(c.username, e.user_id) AS username, SUM(e.amount) AS total
             FROM earn_log e LEFT JOIN coins c ON c.user_id = e.user_id
             WHERE e.ts >= ?1
             GROUP BY e.user_id
             HAVING total >= 1
             ORDER BY total DESC, username ASC
             LIMIT ?2",
        )
        .expect("prepare week");
    let rows = stmt
        .query_map(params![since, limit], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))
        })
        .expect("query week");
    rows.filter_map(|r| r.ok()).collect()
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + (d - 1);
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}
/// 0 = zondag … 6 = zaterdag
fn weekday_from_days(z: i64) -> i64 {
    (z % 7 + 4).rem_euclid(7)
}
fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}
fn last_sunday_days(year: i64, month: i64) -> i64 {
    let z = days_from_civil(year, month, days_in_month(year, month));
    z - weekday_from_days(z)
}
/// Brussel-offset (s) op een UTC-epoch: +2u zomertijd, +1u wintertijd (EU-regels:
/// zomertijd van laatste zondag maart 01:00 UTC tot laatste zondag oktober 01:00 UTC).
fn brussels_offset(utc: i64) -> i64 {
    let (y, _, _) = civil_from_days(utc.div_euclid(86400));
    let mar = last_sunday_days(y, 3) * 86400 + 3600;
    let oct = last_sunday_days(y, 10) * 86400 + 3600;
    if utc >= mar && utc < oct {
        7200
    } else {
        3600
    }
}
/// Epoch (UTC) van de meest recente zaterdag 15:00 Brusselse tijd (≤ now).
pub fn last_saturday_1500_brussels(now: f64) -> f64 {
    let now_i = now as i64;
    let off = brussels_offset(now_i);
    let local = now_i + off;
    let day = local.div_euclid(86400);
    let days_since_sat = (weekday_from_days(day) - 6).rem_euclid(7);
    let mut boundary = (day - days_since_sat) * 86400 + 15 * 3600;
    if boundary > local {
        boundary -= 7 * 86400;
    }
    (boundary - off) as f64
}
/// Eerstvolgende zaterdag 15:00 Brusselse tijd (> now).
pub fn next_saturday_1500_brussels(now: f64) -> f64 {
    let mut b = last_saturday_1500_brussels(now);
    while b <= now {
        b += 7.0 * 86400.0;
    }
    b
}

/// Levelnummer (0-based, oneindig) uit verdiende coins. Zelfde formule als de site
/// (`level_info` in web.rs): 50 × 1.6^level per stap.
pub fn level_of(earned: i64) -> i64 {
    let (base, growth) = (50.0_f64, 1.6_f64);
    let mut level = 0i64;
    let mut floor = 0i64;
    loop {
        let cost = (base * growth.powi(level as i32)).round() as i64;
        if cost <= 0 || floor.checked_add(cost).is_none() || earned < floor + cost {
            return level;
        }
        floor += cost;
        level += 1;
    }
}

// --- level-up-cadeaus ---------------------------------------------------

/// Hoogste level waarvoor dit lid al een cadeau-embed kreeg (0 = nog nooit).
/// **Atomische compare-and-swap** van de level-marker `gifted_level`: zet ze naar `cur` als die
/// hoger is dan de huidige waarde, en returnt de VORIGE marker zodat de aanroeper exact de range
/// `[prev+1, cur]` post. `None` = niets te doen (marker al ≥ `cur`) óf een gelijktijdige aanroep
/// claimde de range net eerder → de aanroeper post dan **niets** → geen dubbele cadeaus/embeds.
///
/// De marker gaat vóór het posten omhoog (self-healing blijft: een gemiste level-up wordt bij de
/// volgende verdienste alsnog opgepikt). Crasht het proces ná de swap maar vóór het cadeau
/// aangemaakt is, dan mist dat ene cadeau — bewust die kant op (geen dubbele uitbetaling), zoals
/// bij de daily-guard. IMMEDIATE-tx: de read en de write kunnen niet met een 2e aanroep verweven.
pub fn advance_gifted_level(pool: &DbPool, uid: &str, cur: i64) -> Option<i64> {
    let mut conn = pool.get().expect("db");
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .expect("tx gifted_level");
    let prev: i64 = tx
        .query_row(
            "SELECT gifted_level FROM coins WHERE user_id = ?1",
            params![uid],
            |r| r.get(0),
        )
        .optional()
        .expect("q gifted_level")
        .unwrap_or(0);
    if cur <= prev {
        return None; // niets te claimen (tx rolt terug bij drop)
    }
    tx.execute(
        "UPDATE coins SET gifted_level = ?2 WHERE user_id = ?1",
        params![uid, cur],
    )
    .expect("advance gifted_level");
    tx.commit().expect("commit gifted_level");
    Some(prev)
}

/// Registreer een openstaand cadeau (claimed = 0) en geef het rij-id terug — dat komt
/// in de custom_id van de claim-knop.
pub fn create_level_gift(pool: &DbPool, uid: &str, amount: i64, level: i64, kind: &str, ts: f64) -> i64 {
    let conn = pool.get().expect("db");
    conn.execute(
        "INSERT INTO level_gifts (uid, amount, level, kind, claimed, ts) VALUES (?1, ?2, ?3, ?4, 0, ?5)",
        params![uid, amount, level, kind, ts],
    )
    .expect("insert level_gift");
    conn.last_insert_rowid()
}

/// Uitkomst van een claim-poging op een cadeau-knop.
pub enum GiftClaim {
    Granted(i64),   // bedrag toegekend
    AlreadyClaimed, // al opgehaald
    NotYours,       // iemand anders klikte
    NotFound,       // cadeau bestaat niet (meer)
}

/// Claim een cadeau: atomisch (claimed 0→1) zodat dubbelklikken nooit dubbel uitbetaalt.
/// Enkel de eigenaar (`uid`) kan claimen. Bij succes komen de coins meteen op het saldo
/// **als verdienste** (verhoogt `total_earned` en logt in `earn_log`, in dezelfde transactie):
/// álle coins tellen mee voor de level-up, ongeacht de bron, en de gift verschijnt in het
/// uurlijkse overzicht. Geen op-hol-slaan: een gift is 1,5 % van het saldo, terwijl een
/// levelgat altijd ~30-40 % is → een cadeau kan nooit zélf een volgend level ontgrendelen.
/// (De caller draait na de claim alsnog `maybe_levelup` voor het randgeval + directe consistentie.)
pub fn claim_level_gift(pool: &DbPool, gift_id: i64, uid: &str, username: &str, ts: f64) -> GiftClaim {
    let mut conn = pool.get().expect("db");
    // IMMEDIATE-tx: de claim-vlag én de uitbetaling zitten in ÉÉN transactie. Zonder dit liep de
    // uitbetaling op een aparte connectie ná de commit van `claimed=1` → een crash daartussen liet
    // het cadeau als geclaimd achter zónder gestorte coins (stille data-loss). Nu: alles of niets.
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .expect("tx claim");
    let row: Option<(String, i64, i64)> = tx
        .query_row(
            "SELECT uid, amount, claimed FROM level_gifts WHERE id = ?1",
            params![gift_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .expect("q level_gift");
    let amount = match row {
        None => return GiftClaim::NotFound, // tx rolt terug bij drop (geen writes gedaan)
        Some((owner, _, _)) if owner != uid => return GiftClaim::NotYours,
        Some((_, _, claimed)) if claimed != 0 => return GiftClaim::AlreadyClaimed,
        Some((_, amount, _)) => {
            let n = tx
                .execute(
                    "UPDATE level_gifts SET claimed = 1 WHERE id = ?1 AND claimed = 0",
                    params![gift_id],
                )
                .expect("claim update");
            if n == 0 {
                return GiftClaim::AlreadyClaimed;
            }
            amount
        }
    };
    // Boek als échte verdienste (coins + total_earned + earn_log), zonder `last_award` aan te
    // raken (dat is enkel de chat-cooldown — een cadeau mag die niet resetten).
    tx.execute(
        "INSERT INTO coins (user_id, username, coins, max_balance, total_earned)
         VALUES (?1, ?2, ?3, ?3, ?3)
         ON CONFLICT(user_id) DO UPDATE SET
             coins        = coins + excluded.coins,
             username     = excluded.username,
             max_balance  = MAX(max_balance, coins + excluded.coins),
             total_earned = total_earned + excluded.coins",
        params![uid, username, amount],
    )
    .expect("credit earned");
    let _ = tx.execute(
        "INSERT INTO earn_log (user_id, amount, ts) VALUES (?1, ?2, ?3)",
        params![uid, amount, ts],
    );
    tx.commit().expect("commit claim");
    GiftClaim::Granted(amount)
}

/// Is dit cadeau (id) al geclaimd? Een niet-bestaand id → true (geen blocker voor "alles op").
pub fn gift_claimed(pool: &DbPool, gid: i64) -> bool {
    let conn = pool.get().expect("db");
    conn.query_row(
        "SELECT claimed FROM level_gifts WHERE id = ?1",
        params![gid],
        |r| r.get::<_, i64>(0),
    )
    .optional()
    .expect("q gift_claimed")
    .map(|c| c != 0)
    .unwrap_or(true)
}

/// Unix-tijdstip van de laatste daily-claim (0.0 als de user er nog geen deed).
pub fn get_last_daily(pool: &DbPool, user_id: &str) -> f64 {
    let conn = pool.get().expect("db");
    conn.query_row(
        "SELECT last_daily FROM coins WHERE user_id = ?1",
        params![user_id],
        |r| r.get(0),
    )
    .optional()
    .expect("query last_daily")
    .unwrap_or(0.0)
}

/// De huidige daily-streak van een lid (0 als er nog geen daily geclaimd is).
pub fn get_daily_streak(pool: &DbPool, user_id: &str) -> i64 {
    let conn = pool.get().expect("db");
    conn.query_row(
        "SELECT daily_streak FROM coins WHERE user_id = ?1",
        params![user_id],
        |r| r.get(0),
    )
    .optional()
    .expect("query daily_streak")
    .unwrap_or(0)
}

/// Daily-beloning: tel `amount` bij, zet last_daily (eigen 24u-cooldown) en de
/// nieuwe `streak`, houd max_balance bij. Returnt het nieuwe totaal.
/// Boekt de daily **atomisch**: de `WHERE last_daily <= ?guard_ts` op de upsert zorgt dat
/// enkel de eerste van twee gelijktijdige claims (dubbelklik-race op de knop) doorkomt — de
/// tweede raakt een no-op (0 rijen) omdat de winnaar `last_daily` al vooruit zette.
/// `guard_ts` = `now - cooldown`. Retourneert `Some(nieuw_saldo)` bij een geboekte claim,
/// `None` als de race verloren is (niets geboekt).
pub fn award_daily(
    pool: &DbPool,
    user_id: &str,
    username: &str,
    amount: i64,
    streak: i64,
    ts: f64,
    guard_ts: f64,
) -> Option<i64> {
    let conn = pool.get().expect("db");
    // De WHERE bindt aan de BESTAANDE rij-waarde (niet `excluded`). Voor een gloednieuw lid
    // vuurt de INSERT (geen conflict, WHERE niet van toepassing) → eerste daily werkt altijd.
    let changed = conn
        .execute(
            "INSERT INTO coins (user_id, username, coins, last_daily, daily_streak, max_balance, total_earned)
         VALUES (?1, ?2, ?3, ?4, ?5, ?3, ?3)
         ON CONFLICT(user_id) DO UPDATE SET
             coins        = coins + excluded.coins,
             username     = excluded.username,
             last_daily   = excluded.last_daily,
             daily_streak = excluded.daily_streak,
             max_balance  = MAX(max_balance, coins + excluded.coins),
             total_earned = total_earned + excluded.coins
         WHERE last_daily <= ?6",
            params![user_id, username, amount, ts, streak, guard_ts],
        )
        .expect("insert daily");
    if changed == 0 {
        return None; // race verloren: gelijktijdige claim was net eerder
    }
    log_earn_event(&conn, user_id, amount, ts);
    let total = conn
        .query_row(
            "SELECT coins FROM coins WHERE user_id = ?1",
            params![user_id],
            |r| r.get(0),
        )
        .expect("query totaal");
    Some(total)
}

/// (saldo, hoogste saldo ooit, publiek?, ooit verdiend) voor de Coins-tab.
/// (chests meegeopend, chests gewonnen) voor dit lid, uit het logboek.
///
/// **Meegeopend** = het aantal `chest/join`-regels: één per lid per chest, enkel bij een
/// échte nieuwe deelname (een tweede klik logt `already_in`, een klik op een verdwenen chest
/// `too_late` — die tellen dus niet mee). Een chest die nadien despawnde omdat er te weinig
/// klikkers waren, telt wél mee: je hebt hem geopend, hij ging enkel niet open.
/// **Gewonnen** = de `chest/win`-regels, inclusief die van een `!chestrescue`.
pub fn chest_counts(pool: &DbPool, user_id: &str) -> (i64, i64) {
    let conn = pool.get().expect("db");
    conn.query_row(
        "SELECT COALESCE(SUM(event = 'join'), 0), COALESCE(SUM(event = 'win'), 0)
           FROM server_log WHERE category = 'chest' AND actor_uid = ?1",
        params![user_id],
        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
    )
    .optional()
    .ok()
    .flatten()
    .unwrap_or((0, 0))
}

pub fn get_stats(pool: &DbPool, user_id: &str) -> (i64, i64, bool, i64) {
    let conn = pool.get().expect("db");
    conn.query_row(
        "SELECT coins, max_balance, is_public, total_earned FROM coins WHERE user_id = ?1",
        params![user_id],
        |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)? != 0,
                r.get::<_, i64>(3)?,
            ))
        },
    )
    .optional()
    .expect("query stats")
    .unwrap_or((0, 0, false, 0))
}

/// Zet de publiek-vlag (of het saldo op het leaderboard mag verschijnen).
pub fn set_public(pool: &DbPool, user_id: &str, username: &str, public: bool) {
    let conn = pool.get().expect("db");
    conn.execute(
        "INSERT INTO coins (user_id, username, is_public)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(user_id) DO UPDATE SET
             is_public = excluded.is_public,
             username  = excluded.username",
        params![user_id, username, public as i64],
    )
    .expect("set public");
}

/// Publiek leaderboard: (user_id, username, saldo, max_balance) aflopend op saldo.
/// Leaderboard 'Now' — (user_id, username, huidig saldo) aflopend.
pub fn leaderboard_now(pool: &DbPool, limit: i64) -> Vec<(String, String, i64)> {
    lb_query(
        pool,
        "SELECT user_id, username, coins FROM coins
         WHERE coins > 0
         ORDER BY coins DESC, username ASC LIMIT ?1",
        limit,
    )
}

/// Leaderboard 'All-time' — (user_id, username, ooit verdiend) aflopend.
pub fn leaderboard_alltime(pool: &DbPool, limit: i64) -> Vec<(String, String, i64)> {
    lb_query(
        pool,
        "SELECT user_id, username, total_earned FROM coins
         WHERE total_earned > 0
         ORDER BY total_earned DESC, username ASC LIMIT ?1",
        limit,
    )
}

fn lb_query(pool: &DbPool, sql: &str, limit: i64) -> Vec<(String, String, i64)> {
    let conn = pool.get().expect("db");
    let mut stmt = conn.prepare(sql).expect("prepare leaderboard");
    let rows = stmt
        .query_map(params![limit], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))
        })
        .expect("query leaderboard");
    rows.filter_map(Result::ok).collect()
}

// --- shop: schappen & items ---------------------------------------------

/// Eén verkoopbaar item (gem/graphic) in de shop.
#[derive(Clone, Debug)]
pub struct Item {
    pub id: i64,
    pub name: String,
    pub price: i64,
    pub image: String,
    pub image2: String, // optionele tweede afbeelding (plain items: kleiner onder de titel)
    pub color: String,
    pub role_id: String,
    pub duration: i64, // 0 = permanent, >0 = seconden
    pub category: String,
    pub description: String,
    pub zone: String,           // 'shelf' | 'lucky'
    pub shelf_id: Option<i64>,  // NULL voor lucky
    /// Uitverkocht: nog zichtbaar, maar niet koopbaar (grijze "Out of Stock"-knop).
    pub sold_out: bool,
    /// Voorraad: **-1 = onbeperkt** (niet gevolgd), anders het aantal dat nog te koop is.
    /// Elke aankoop telt er één af; op 0 is het voor iedereen Out of Stock.
    pub stock: i64,
    /// Relatief lot-gewicht in de dagelijkse shoprotatie: hoger = vaker getrokken.
    /// Zegt niets over de kans op zich — enkel de verhouding tot de andere meedoende items.
    pub shop_weight: f64,
    /// Doet dit item mee in de dagrotatie? Los van het gewicht, zodat uitzetten het
    /// ingestelde gewicht niet wist. De passen staan hier standaard op `false`.
    pub in_rotation: bool,
    /// Testpas: gratis, geen voorraad, geen dagrotatie — en enkel te koop voor de namen
    /// op de lijst van dít item (`item_allow`, het vakje "Naam" in Manage → Shop). Staat
    /// volledig los van de gewone pas, die enkel door `sold_out`/`stock` gestuurd wordt.
    pub test_pass: bool,
}

/// De kolomlijst van `items`, één keer uitgeschreven: hij stond vier keer letterlijk in
/// een query en dan is een nieuwe kolom vergeten op één plek een kwestie van tijd.
const ITEM_COLS: &str = "id, name, price, image, image2, color, role_id, duration, \
                         category, description, zone, shelf_id, sold_out, stock, \
                         shop_weight, in_rotation, test_pass";

fn row_to_item(r: &rusqlite::Row) -> rusqlite::Result<Item> {
    Ok(Item {
        id: r.get("id")?,
        name: r.get("name")?,
        price: r.get("price")?,
        image: r.get("image")?,
        image2: r.get("image2")?,
        color: r.get("color")?,
        role_id: r.get("role_id")?,
        duration: r.get("duration")?,
        category: r.get("category")?,
        description: r.get("description")?,
        zone: r.get("zone")?,
        shelf_id: r.get("shelf_id")?,
        sold_out: r.get::<_, i64>("sold_out")? != 0,
        stock: r.get("stock")?,
        shop_weight: r.get("shop_weight")?,
        in_rotation: r.get::<_, i64>("in_rotation")? != 0,
        test_pass: r.get::<_, i64>("test_pass")? != 0,
    })
}

/// Alle schappen (id, titel) op positie.
pub fn list_shelves(pool: &DbPool) -> Vec<(i64, String)> {
    let conn = pool.get().expect("db");
    let mut stmt = conn
        .prepare("SELECT id, title FROM shelves ORDER BY position, id")
        .expect("prepare shelves");
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
        .expect("query shelves");
    rows.filter_map(Result::ok).collect()
}

/// Items van één schap, op positie.
pub fn shelf_items(pool: &DbPool, shelf_id: i64) -> Vec<Item> {
    let conn = pool.get().expect("db");
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {ITEM_COLS} FROM items
             WHERE zone = 'shelf' AND shelf_id = ?1 ORDER BY position, id"
        ))
        .expect("prepare shelf_items");
    let rows = stmt.query_map(params![shelf_id], row_to_item).expect("query");
    rows.filter_map(Result::ok).collect()
}

/// Alle lucky-items, op positie.
pub fn lucky_items(pool: &DbPool) -> Vec<Item> {
    let conn = pool.get().expect("db");
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {ITEM_COLS} FROM items WHERE zone = 'lucky' ORDER BY position, id"
        ))
        .expect("prepare lucky_items");
    let rows = stmt.query_map([], row_to_item).expect("query lucky");
    rows.filter_map(Result::ok).collect()
}

/// Eén item ophalen (voor image-vervanging e.d.).
pub fn get_item(pool: &DbPool, id: i64) -> Option<Item> {
    let conn = pool.get().expect("db");
    conn.query_row(
        &format!("SELECT {ITEM_COLS} FROM items WHERE id = ?1"),
        params![id],
        row_to_item,
    )
    .optional()
    .expect("query item")
}

/// Nieuw (leeg) schap onderaan. Returnt het id.
pub fn add_shelf(pool: &DbPool, title: &str) -> i64 {
    let conn = pool.get().expect("db");
    let pos: i64 = conn
        .query_row("SELECT COALESCE(MAX(position)+1,0) FROM shelves", [], |r| r.get(0))
        .unwrap_or(0);
    conn.execute(
        "INSERT INTO shelves (title, position) VALUES (?1, ?2)",
        params![title, pos],
    )
    .expect("add shelf");
    conn.last_insert_rowid()
}

pub fn rename_shelf(pool: &DbPool, id: i64, title: &str) {
    let conn = pool.get().expect("db");
    conn.execute("UPDATE shelves SET title = ?2 WHERE id = ?1", params![id, title])
        .expect("rename shelf");
}

/// Schap + al zijn items verwijderen.
pub fn delete_shelf(pool: &DbPool, id: i64) {
    let conn = pool.get().expect("db");
    conn.execute("DELETE FROM items WHERE shelf_id = ?1", params![id])
        .expect("del shelf items");
    conn.execute("DELETE FROM shelves WHERE id = ?1", params![id])
        .expect("del shelf");
}

/// Nieuw leeg item toevoegen aan een schap (zone='shelf') of aan lucky. Returnt id.
pub fn add_item(pool: &DbPool, zone: &str, shelf_id: Option<i64>) -> i64 {
    let conn = pool.get().expect("db");
    let pos: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(position)+1,0) FROM items WHERE zone=?1 AND IFNULL(shelf_id,-1)=IFNULL(?2,-1)",
            params![zone, shelf_id],
            |r| r.get(0),
        )
        .unwrap_or(0);
    // Nieuwe items zijn standaard 'inventory' (verzamelbaar → kaart in de Inventory).
    conn.execute(
        "INSERT INTO items (zone, shelf_id, name, price, category, position)
         VALUES (?1, ?2, '', 0, 'inventory', ?3)",
        params![zone, shelf_id, pos],
    )
    .expect("add item");
    conn.last_insert_rowid()
}

#[allow(clippy::too_many_arguments)]
pub fn update_item(
    pool: &DbPool,
    id: i64,
    name: &str,
    price: i64,
    role_id: &str,
    duration: i64,
    category: &str,
    description: &str,
    sold_out: bool,
    test_pass: bool,
) {
    let conn = pool.get().expect("db");
    conn.execute(
        "UPDATE items SET name = ?2, price = ?3, role_id = ?4, duration = ?5,
             category = ?6, description = ?7, sold_out = ?8, test_pass = ?9 WHERE id = ?1",
        params![
            id,
            name,
            price,
            role_id,
            duration,
            category,
            description,
            sold_out as i64,
            test_pass as i64
        ],
    )
    .expect("update item");
}

/// Vul de voorraad aan met `n` (mag negatief om te corrigeren). Een item dat nog op
/// onbeperkt (-1) staat, begint bij 0 — anders zou "+1" bij -1 op 0 uitkomen en dus
/// meteen uitverkocht zijn. Returnt de nieuwe voorraad.
pub fn add_stock(pool: &DbPool, id: i64, n: i64) -> i64 {
    // IMMEDIATE-tx: de read-modify-write is atomisch, zodat een gelijktijdige verkoop (die
    // `stock = stock - 1` relatief decrementeert) niet overschreven wordt door deze absolute
    // write. Zonder dit las een "+5" de oude voorraad en schreef die + 5 terug, waardoor een
    // net-verkocht exemplaar terug in de voorraad "verscheen" (voorraad-inflatie).
    let mut conn = pool.get().expect("db");
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .expect("tx stock");
    let cur: i64 = tx
        .query_row("SELECT stock FROM items WHERE id = ?1", params![id], |r| r.get(0))
        .unwrap_or(-1);
    let base = if cur < 0 { 0 } else { cur };
    let new = (base + n).max(0);
    tx.execute("UPDATE items SET stock = ?2 WHERE id = ?1", params![id, new])
        .expect("add stock");
    tx.commit().expect("commit stock");
    new
}

/// Zet de voorraad terug op onbeperkt (-1): het item wordt niet meer geteld.
pub fn set_stock_unlimited(pool: &DbPool, id: i64) {
    let conn = pool.get().expect("db");
    conn.execute("UPDATE items SET stock = -1 WHERE id = ?1", params![id])
        .expect("stock unlimited");
}

/// Zet de kleur van elk 'inventory'-item (gem) op de kleur van de gelijknamige Discord-rol.
/// `roles` = (rolnaam, hex-kleur "#rrggbb"). Matcht hoofdletter-ongevoelig op de itemnaam.
/// Retourneert het aantal bijgewerkte items.
pub fn sync_gem_colors(pool: &DbPool, roles: &[(String, String)]) -> usize {
    let conn = pool.get().expect("db");
    let mut n = 0;
    for (name, hex) in roles {
        n += conn
            .execute(
                "UPDATE items SET color = ?2 WHERE category = 'inventory' AND LOWER(name) = LOWER(?1)",
                params![name, hex],
            )
            .unwrap_or(0);
    }
    n
}

pub fn set_item_image(pool: &DbPool, id: i64, image: &str) {
    let conn = pool.get().expect("db");
    conn.execute("UPDATE items SET image = ?2 WHERE id = ?1", params![id, image])
        .expect("set image");
}

/// Afbeelding van een item wissen (terug naar kleur-thumb / bol).
pub fn clear_item_image(pool: &DbPool, id: i64) {
    let conn = pool.get().expect("db");
    conn.execute("UPDATE items SET image = '' WHERE id = ?1", params![id])
        .expect("clear image");
}

/// De tweede afbeelding van een item zetten (plain items: klein onder de titel).
pub fn set_item_image2(pool: &DbPool, id: i64, image: &str) {
    let conn = pool.get().expect("db");
    conn.execute("UPDATE items SET image2 = ?2 WHERE id = ?1", params![id, image])
        .expect("set image2");
}

/// De tweede afbeelding van een item wissen.
pub fn clear_item_image2(pool: &DbPool, id: i64) {
    let conn = pool.get().expect("db");
    conn.execute("UPDATE items SET image2 = '' WHERE id = ?1", params![id])
        .expect("clear image2");
}

/// Verplaats een item één plaats naar links (dir<0) of rechts (dir>0) binnen
/// z'n eigen zone/schap. Herschrijft de posities lineair, dus ook robuust bij
/// gelijke/oude posities.
pub fn move_item(pool: &DbPool, id: i64, dir: i64) {
    if dir == 0 {
        return;
    }
    let conn = pool.get().expect("db");
    let Some((zone, shelf)) = conn
        .query_row(
            "SELECT zone, shelf_id FROM items WHERE id = ?1",
            params![id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<i64>>(1)?)),
        )
        .optional()
        .expect("q item zone")
    else {
        return;
    };
    // Geordende siblings in dezelfde zone/schap.
    let mut ids: Vec<i64> = {
        let mut stmt = conn
            .prepare(
                "SELECT id FROM items WHERE zone = ?1 AND IFNULL(shelf_id,-1) = IFNULL(?2,-1)
                 ORDER BY position, id",
            )
            .expect("prepare siblings");
        stmt.query_map(params![zone, shelf], |r| r.get::<_, i64>(0))
            .expect("query siblings")
            .filter_map(Result::ok)
            .collect()
    };
    let Some(i) = ids.iter().position(|x| *x == id) else {
        return;
    };
    let j = if dir < 0 {
        if i == 0 {
            return;
        }
        i - 1
    } else {
        if i + 1 >= ids.len() {
            return;
        }
        i + 1
    };
    ids.swap(i, j);
    for (pos, iid) in ids.iter().enumerate() {
        conn.execute(
            "UPDATE items SET position = ?2 WHERE id = ?1",
            params![iid, pos as i64],
        )
        .expect("renumber");
    }
}

/// Verplaats een schap-item naar een ander schap (achteraan toegevoegd).
/// Alleen zone='shelf'-items; lucky-items blijven ongemoeid.
pub fn set_item_shelf(pool: &DbPool, id: i64, shelf_id: i64) {
    let conn = pool.get().expect("db");
    let pos: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(position)+1,0) FROM items WHERE zone='shelf' AND shelf_id = ?1",
            params![shelf_id],
            |r| r.get(0),
        )
        .unwrap_or(0);
    conn.execute(
        "UPDATE items SET shelf_id = ?2, position = ?3 WHERE id = ?1 AND zone = 'shelf'",
        params![id, shelf_id, pos],
    )
    .expect("move item shelf");
}

/// Zet de rotatie-instellingen van één item: het lot-gewicht in de dagtrekking en of het
/// überhaupt meedoet. Het gewicht wordt bewaard ook als het item niet meedoet — zo staat de
/// oude instelling er nog als je het later terug aanzet. Negatieve gewichten worden op 0
/// geklemd (0 = doet feitelijk niet mee, ook al staat de vlag aan).
pub fn set_item_rotation(pool: &DbPool, id: i64, weight: f64, in_rotation: bool) {
    let conn = pool.get().expect("db");
    conn.execute(
        "UPDATE items SET shop_weight = ?2, in_rotation = ?3 WHERE id = ?1",
        params![id, weight.max(0.0), i64::from(in_rotation)],
    )
    .expect("set item rotation");
}

pub fn delete_item(pool: &DbPool, id: i64) {
    let conn = pool.get().expect("db");
    conn.execute("DELETE FROM items WHERE id = ?1", params![id])
        .expect("del item");
    // Namenlijst mee opruimen: id's worden hergebruikt, en een achtergebleven lijst zou
    // stil aan een volgend item blijven plakken.
    conn.execute("DELETE FROM item_allow WHERE item_id = ?1", params![id]).ok();
}

// --- kopen & ontgrendelen -----------------------------------------------

/// Koop/ontgrendel `item_id` voor `uid`: elk item kan maar één keer bezeten
/// worden (bingokaart). Controleert saldo, trekt de prijs af en ontgrendelt
/// het item. Atomisch. Ok(nieuw_saldo, item) of Err(reden).
pub fn purchase(pool: &DbPool, uid: &str, item_id: i64, ts: f64) -> Result<(i64, Item), String> {
    let item = get_item(pool, item_id).ok_or("This item no longer exists.")?;
    let mut conn = pool.get().expect("db");
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    // Inventory-items én de booster (Lucky Horseshoe) zijn verzamelkaart-slots: maar
    // één keer te bezitten. De booster is permanent — bezit = altijd dubbele chest-kans.
    if item.category == "inventory" || item.category == "booster" {
        let owned: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM inventory WHERE user_id = ?1 AND item_id = ?2",
                params![uid, item_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        if owned > 0 {
            return Err(format!("You already own {}.", item.name));
        }
    }
    // NB (2026-08-04): hier stonden twee blokkades op de pas — "je hebt al een lopende
    // pas" en "je hebt al permanente toegang". Allebei geschrapt (user-wens). Een pas is
    // sinds die dag een **tegoed aan speeltijd** dat enkel leegloopt terwijl je in-game
    // bent, en elke pas telt zijn tijd bij dat tegoed. Meerdere passen kopen is dus geen
    // fout maar de normale gang van zaken: je koopt uren, geen venster. De oude check keek
    // bovendien naar `expires`, en die waarde zegt niets meer over wie er binnen mag — dat
    // beslist de server op het tegoed. Zie tale/HANDOVER.md, blok "SPEELTIJD-PASSEN".
    // Voorraad: -1 = onbeperkt. Anders atomisch aftellen binnen deze transactie — twee
    // gelijktijdige kopers mogen nooit samen de laatste pas meenemen.
    if item.stock >= 0 {
        let n = tx
            .execute("UPDATE items SET stock = stock - 1 WHERE id = ?1 AND stock > 0", params![item_id])
            .map_err(|e| e.to_string())?;
        if n == 0 {
            return Err(format!("{} is out of stock.", item.name));
        }
    }
    let balance: i64 = tx
        .query_row("SELECT coins FROM coins WHERE user_id = ?1", params![uid], |r| r.get(0))
        .optional()
        .map_err(|e| e.to_string())?
        .unwrap_or(0);
    if balance < item.price {
        return Err(format!(
            "Not enough coins: you have {balance}, {} costs {}.",
            item.name, item.price
        ));
    }
    tx.execute(
        "UPDATE coins SET coins = coins - ?2 WHERE user_id = ?1",
        params![uid, item.price],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT INTO inventory (user_id, item_id, name, image, price, acquired)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![uid, item_id, item.name, item.image, item.price, ts],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok((balance - item.price, item))
}

/// Verbruik één exemplaar van een item uit de inventory (voor boosts).
#[allow(dead_code)]
pub fn consume_item(pool: &DbPool, uid: &str, item_id: i64) {
    let conn = pool.get().expect("db");
    conn.execute(
        "DELETE FROM inventory WHERE id = (SELECT id FROM inventory
             WHERE user_id = ?1 AND item_id = ?2 LIMIT 1)",
        params![uid, item_id],
    )
    .expect("consume item");
}

/// Bewaar een lopende (nog niet gepopte) chest zodat de pop-timer een herstart
/// overleeft. Bij opstart wordt hij via `load_live_chests` hervat.
pub fn save_live_chest(pool: &DbPool, msg_id: u64, channel_id: u64, pop_ts: i64) {
    let conn = pool.get().expect("db");
    conn.execute(
        "INSERT INTO live_chests (message_id, channel_id, pop_ts) VALUES (?1, ?2, ?3)
         ON CONFLICT(message_id) DO UPDATE SET channel_id = excluded.channel_id, pop_ts = excluded.pop_ts",
        params![msg_id.to_string(), channel_id.to_string(), pop_ts],
    )
    .expect("save live chest");
}

/// Verwijder een lopende chest uit de persistentie (bij pop/despawn/rescue).
pub fn delete_live_chest(pool: &DbPool, msg_id: u64) {
    let conn = pool.get().expect("db");
    conn.execute(
        "DELETE FROM live_chests WHERE message_id = ?1",
        params![msg_id.to_string()],
    )
    .ok();
}

/// Alle lopende chests (message_id, channel_id, pop_ts) om bij opstart te hervatten.
pub fn load_live_chests(pool: &DbPool) -> Vec<(u64, u64, i64)> {
    let conn = pool.get().expect("db");
    let mut stmt = conn
        .prepare("SELECT message_id, channel_id, pop_ts FROM live_chests")
        .expect("prepare load_live_chests");
    let rows = stmt
        .query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))
        })
        .expect("query live_chests");
    rows.filter_map(Result::ok)
        .filter_map(|(m, c, ts)| Some((m.parse().ok()?, c.parse().ok()?, ts)))
        .collect()
}

/// Reconstrueer de deelnemers (uid, naam) van een chest uit het logboek, in
/// klik-volgorde en ontdubbeld op uid (nieuwste naam wint). Gebruikt om een
/// verweesde chest (bv. verloren bij een herstart) alsnog handmatig te openen.
pub fn chest_joiners_from_log(pool: &DbPool, msg_id: u64) -> Vec<(String, String)> {
    let conn = pool.get().expect("db");
    let mut stmt = conn
        .prepare(
            "SELECT actor_uid, actor_name FROM server_log
             WHERE category = 'chest' AND event = 'join' AND ref_id = ?1
             ORDER BY id",
        )
        .expect("prepare chest_joiners");
    let rows = stmt
        .query_map(params![msg_id.to_string()], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .expect("query chest_joiners");
    let mut seen = std::collections::HashSet::new();
    let mut out: Vec<(String, String)> = Vec::new();
    for (uid, name) in rows.filter_map(Result::ok) {
        if seen.insert(uid.clone()) {
            out.push((uid, name));
        }
    }
    out
}

/// De meest recente chest die deelnemers kreeg maar nooit werd afgehandeld (geen
/// `win`/`despawn` in het log) — d.w.z. verweesd bij een herstart. None als er
/// geen openstaat. Gebruikt zodat `!chestrescue` zonder message-id werkt.
pub fn last_unresolved_chest(pool: &DbPool) -> Option<u64> {
    let conn = pool.get().expect("db");
    conn.query_row(
        "SELECT ref_id FROM server_log
         WHERE category = 'chest' AND event = 'join'
           AND ref_id NOT IN (
               SELECT ref_id FROM server_log
               WHERE category = 'chest' AND event IN ('win', 'despawn')
           )
         GROUP BY ref_id
         ORDER BY MAX(id) DESC
         LIMIT 1",
        [],
        |r| r.get::<_, String>(0),
    )
    .optional()
    .expect("query last_unresolved_chest")
    .and_then(|s| s.parse::<u64>().ok())
}

/// Het kanaal waarin een (verweesde) chest oorspronkelijk verscheen, opgediept uit
/// het logboek: elk chest-event (spawn/join/…) draagt het `channel_id`. Nodig sinds
/// chests in meerdere kanalen spawnen — `chestrescue` moet de uitslag in het júiste
/// kanaal posten, niet blind in #general. `None` als er geen chest-log met kanaal is.
pub fn chest_channel_from_log(pool: &DbPool, msg_id: u64) -> Option<u64> {
    let conn = pool.get().expect("db");
    conn.query_row(
        "SELECT channel_id FROM server_log
          WHERE category = 'chest' AND ref_id = ?1 AND channel_id <> ''
          ORDER BY id ASC LIMIT 1",
        params![msg_id.to_string()],
        |r| r.get::<_, String>(0),
    )
    .optional()
    .expect("query chest_channel_from_log")
    .and_then(|s| s.parse::<u64>().ok())
}

/// Bezit dit lid een permanente booster (Lucky Horseshoe)? Zo ja → altijd dubbele
/// chest-kans. Eigendom = één rij in `inventory` voor een 'booster'-item (koop 1×).
pub fn owns_horseshoe(pool: &DbPool, uid: &str) -> bool {
    let conn = pool.get().expect("db");
    conn.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM inventory inv JOIN items i ON i.id = inv.item_id
              WHERE inv.user_id = ?1 AND i.category = 'booster')",
        params![uid],
        |r| r.get::<_, i64>(0),
    )
    .optional()
    .ok()
    .flatten()
    .unwrap_or(0)
        > 0
}

/// Het lot-gewicht van dit lid bij een treasure-chest-trekking: **2** wie de Lucky
/// Horseshoe bezit, anders 1. Geldt (voorlopig) voor de enige chest, Fortuna's Favor;
/// een later ander chest-type dat deze weging niet wil, roept dit gewoon niet aan.
pub fn chest_weight(pool: &DbPool, uid: &str) -> u32 {
    if owns_horseshoe(pool, uid) {
        2
    } else {
        1
    }
}

/// Alle booster-items (category 'booster', bv. de Lucky Horseshoe), op positie —
/// ongeacht bezit. Voedt de grey-out-slots op de Boosts-tab.
pub fn all_booster_items(pool: &DbPool) -> Vec<Item> {
    let conn = pool.get().expect("db");
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {ITEM_COLS} FROM items WHERE category = 'booster' ORDER BY position, id"
        ))
        .expect("prepare all_booster_items");
    let rows = stmt.query_map([], row_to_item).expect("query boosters");
    rows.filter_map(Result::ok).collect()
}

/// Zet de permanente-toegangsvlag (na gebruik van de permanente pas).
/// Trek de pas van een **Hytale-naam** volledig in: de grant verdwijnt en permanente
/// toegang gaat eraf. Géén coins terug — dit is een moderatie-actie (de refund op de
/// logpagina is de vriendelijke variant). Returnt de getroffen (user_id, naam)-paren,
/// zodat de aanroeper kan loggen wie het trof.
///
/// Op naam i.p.v. user_id, want de aanroeper is het panel: dat kent enkel de in-game
/// naam. Naam-vergelijking is hoofdletter-ongevoelig — Hytale-namen zijn dat ook.
pub fn revoke_pass_by_name(pool: &DbPool, hytale_name: &str) -> Vec<(String, String)> {
    let conn = pool.get().expect("db");
    let mut stmt = conn
        .prepare(
            "SELECT w.user_id, COALESCE(c.username, w.user_id)
               FROM hytale_whitelist w LEFT JOIN coins c ON c.user_id = w.user_id
              WHERE lower(w.hytale_name) = lower(?1)",
        )
        .expect("prepare revoke lookup");
    let hits: Vec<(String, String)> = stmt
        .query_map(params![hytale_name], |r| Ok((r.get(0)?, r.get(1)?)))
        .expect("query revoke lookup")
        .filter_map(Result::ok)
        .collect();
    for (uid, _) in &hits {
        conn.execute("DELETE FROM hytale_whitelist WHERE user_id = ?1", params![uid]).ok();
        conn.execute("UPDATE coins SET perma_access = 0 WHERE user_id = ?1", params![uid]).ok();
    }
    hits
}

/// Zet de permanente-toegangsvlag. Sinds 2026-08-04 verkoopt de shop geen permanente
/// pas meer (toegang is een tegoed aan speeltijd), dus dit wordt nergens meer aangeroepen —
/// het blijft staan voor een admin-toekenning of als de Twitch-perma-reward ooit aangaat.
#[allow(dead_code)]
pub fn set_perma_access(pool: &DbPool, uid: &str, username: &str) {
    let conn = pool.get().expect("db");
    conn.execute(
        "INSERT INTO coins (user_id, username, perma_access) VALUES (?1, ?2, 1)
         ON CONFLICT(user_id) DO UPDATE SET perma_access = 1, username = excluded.username",
        params![uid, username],
    )
    .expect("set perma_access");
}

/// Heeft dit lid permanente toegang? (behouden voor toekomstig gebruik.)
#[allow(dead_code)]
pub fn has_perma_access(pool: &DbPool, uid: &str) -> bool {
    let conn = pool.get().expect("db");
    conn.query_row(
        "SELECT perma_access FROM coins WHERE user_id = ?1",
        params![uid],
        |r| r.get::<_, i64>(0),
    )
    .optional()
    .expect("query perma_access")
    .unwrap_or(0)
        != 0
}

// --- Hytale-whitelist (passen = echte whitelist, geen Discord-rol) -------

/// De opgeslagen Hytale-naam van een lid (leeg = nog niet ingesteld).
pub fn get_hytale_name(pool: &DbPool, uid: &str) -> String {
    let conn = pool.get().expect("db");
    conn.query_row(
        "SELECT hytale_name FROM coins WHERE user_id = ?1",
        params![uid],
        |r| r.get::<_, String>(0),
    )
    .optional()
    .expect("query hytale_name")
    .unwrap_or_default()
}

/// Bewaar/actualiseer de Hytale-naam van een lid. Verzet meteen de naam op een
/// eventueel lopende whitelist-grant (dezelfde speler, andere in-game naam).
pub fn set_hytale_name(pool: &DbPool, uid: &str, username: &str, hytale_name: &str) {
    let conn = pool.get().expect("db");
    conn.execute(
        "INSERT INTO coins (user_id, username, hytale_name) VALUES (?1, ?2, ?3)
         ON CONFLICT(user_id) DO UPDATE SET hytale_name = excluded.hytale_name,
                                            username = excluded.username",
        params![uid, username, hytale_name],
    )
    .expect("set hytale_name");
    conn.execute(
        "UPDATE hytale_whitelist SET hytale_name = ?2 WHERE user_id = ?1",
        params![uid, hytale_name],
    )
    .ok();
}

/// De actieve whitelist-grant van een lid: (hytale_name, expires).
/// `expires` = None ⇒ permanent. Geeft None als er geen (geldige) grant is.
pub fn get_whitelist(pool: &DbPool, uid: &str, now: f64) -> Option<(String, Option<f64>)> {
    let conn = pool.get().expect("db");
    conn.query_row(
        "SELECT hytale_name, expires FROM hytale_whitelist WHERE user_id = ?1",
        params![uid],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<f64>>(1)?)),
    )
    .optional()
    .expect("query whitelist")
    .filter(|(_, exp)| exp.map_or(true, |e| e > now))
}

// --- Passen: wie is de eigenaar, en wie houdt de klok bij? ---------------
//
// Sinds 2026-08-04 is een pas een **tegoed aan speeltijd**: hij loopt enkel leeg terwijl de
// speler in-game is. Die boekhouding hoort bij de **tale-kant** — alleen de server weet wie
// er online is. Market blijft de winkel: het verkoopt tijd en stapelt ze op `expires`, en de
// tale-bot leidt het toegekende tegoed af uit de **stijging** van dat veld.
//
// Daarom staat hier geen enkele pauze-logica meer. `expires` is voor market een
// aankoop-grootboek geworden, geen wandklok waar je op mag aftellen; de resterende tijd komt
// uit `passes.json` (zie `crate::pass_ledger`).

/// Een pas zoals market hem kent: van wie, en of hij permanent is.
#[derive(Debug, Clone, PartialEq)]
pub struct PassView {
    pub hytale_name: String,
    /// None ⇒ permanent.
    pub expires: Option<f64>,
}

impl PassView {
    pub fn is_permanent(&self) -> bool {
        self.expires.is_none()
    }
}

/// De pas van een lid, inclusief een pas die via **Twitch** werd ingewisseld.
///
/// Een Twitch-redeem landt onder `twitch:<twitch_user_id>` — market weet niet uit zichzelf
/// welk Discord-lid dat is. De brug is `coins.twitch_id`, gevuld bij de login uit de
/// Discord-verbindingen van het lid zelf. Geen koppeling ⇒ enkel de eigen Discord-grant,
/// en dat is de juiste uitkomst: dan is er geen manier om te wéten van wie die pas is.
///
/// Heeft iemand er twee (bv. een gekochte dagpas én een Twitch-pas), dan wint permanent,
/// en anders de verste `expires`.
pub fn get_whitelist_linked(pool: &DbPool, uid: &str, now: f64) -> Option<PassView> {
    let conn = pool.get().expect("db");
    let twitch_id: String = conn
        .query_row("SELECT twitch_id FROM coins WHERE user_id = ?1", params![uid], |r| {
            r.get::<_, String>(0)
        })
        .optional()
        .expect("query twitch_id")
        .unwrap_or_default();

    let twitch_key = if twitch_id.is_empty() {
        String::new()
    } else {
        format!("twitch:{twitch_id}")
    };
    let mut stmt = conn
        .prepare(
            "SELECT hytale_name, expires FROM hytale_whitelist
             WHERE user_id = ?1 OR (?2 <> '' AND user_id = ?2)",
        )
        .expect("prepare linked whitelist");
    let rows = stmt
        .query_map(params![uid, twitch_key], |r| {
            Ok(PassView { hytale_name: r.get(0)?, expires: r.get(1)? })
        })
        .expect("query linked whitelist")
        .filter_map(Result::ok);

    let mut best: Option<PassView> = None;
    for p in rows {
        // Verlopen grants tellen niet mee. NB: `expires` is een aankoop-grootboek — of er
        // nog écht speeltijd over is, weet enkel de tale-kant.
        if p.expires.map_or(false, |e| e <= now) {
            continue;
        }
        let better = match &best {
            None => true,
            Some(b) => match (b.expires, p.expires) {
                (None, _) => false,
                (_, None) => true,
                (Some(be), Some(pe)) => pe > be,
            },
        };
        if better {
            best = Some(p);
        }
    }
    best
}

// --- Eén Hytale-naam per persoon, over beide bronnen heen --------------------
// De tale-kant houdt het speeltijd-tegoed bij **per Hytale-naam**: alle passen van
// dezelfde naam voeden één klok. Twee namen betekent dus twee klokken, en de tijd
// onder de naam waarmee je niet inlogt is onbereikbaar. Vandaar: wie ergens al een
// naam heeft vastgezet, houdt die — ook als de tweede pas via de andere weg binnenkomt.
//
// ⚠️ Dit kan enkel als de accounts gekoppeld zijn (`coins.twitch_id`, uit de
// **geverifieerde** Discord-verbindingen). Zonder koppeling valt niet te wéten dat het
// dezelfde persoon is; dan blijven het twee vreemden voor market, en dat is de juiste
// uitkomst — liever twee losse klokken dan andermans pas op jouw naam.

/// De naam die de **Twitch-pas** van dit Discord-lid al vastzette, als het lid gekoppeld
/// is en die pas een naam draagt. Leeg/onbekend ⇒ None.
pub fn linked_twitch_name(pool: &DbPool, uid: &str) -> Option<String> {
    let conn = pool.get().expect("db");
    let twitch_id: String = conn
        .query_row("SELECT twitch_id FROM coins WHERE user_id = ?1", params![uid], |r| {
            r.get::<_, String>(0)
        })
        .optional()
        .expect("query twitch_id")
        .unwrap_or_default();
    if twitch_id.is_empty() {
        return None;
    }
    conn.query_row(
        "SELECT hytale_name FROM hytale_whitelist WHERE user_id = ?1",
        params![format!("twitch:{twitch_id}")],
        |r| r.get::<_, String>(0),
    )
    .optional()
    .expect("query twitch grant name")
    .map(|n| n.trim().to_string())
    .filter(|n| !n.is_empty())
}

/// Spiegelbeeld: de Hytale-naam van het Discord-lid dat aan dít Twitch-account hangt.
/// Voor de Twitch-kant, zodat een redeem op de naam landt die het lid op de site al
/// gebruikt i.p.v. een tweede klok te openen.
pub fn linked_discord_name(pool: &DbPool, twitch_id: &str) -> Option<String> {
    if twitch_id.trim().is_empty() {
        return None;
    }
    let conn = pool.get().expect("db");
    conn.query_row(
        "SELECT hytale_name FROM coins WHERE twitch_id = ?1 AND hytale_name <> ''",
        params![twitch_id.trim()],
        |r| r.get::<_, String>(0),
    )
    .optional()
    .expect("query linked discord name")
    .map(|n| n.trim().to_string())
    .filter(|n| !n.is_empty())
}

/// Wat een naamcorrectie geraakt heeft — genoeg om er een eerlijke logregel van te maken.
pub struct NameFix {
    /// De accounts waar écht een rij verzet is: het gekozen account, en bij een gekoppeld
    /// paar ook zijn tegenhanger (`twitch:<id>` ⇄ Discord-uid).
    pub uids: Vec<String>,
    /// De namen die er stonden (ontdubbeld, hoofdletter-ongevoelig).
    pub old: Vec<String>,
}

/// Zet de Hytale-naam van een account recht. Dit is de **enige** weg om een typo te
/// herstellen: voor het lid zelf ligt de naam vast zodra er ergens tijd op staat, precies
/// om te verhinderen dat er een tweede naam naast ontstaat.
///
/// Corrigeert alle plekken waar de naam van deze persoon staat — zijn `coins`-rij én elke
/// grant-rij, aan beide kanten van de Twitch↔Discord-koppeling. Bleef er één achter, dan
/// zou de eerstvolgende aankoop of redeem alsnog op de oude naam landen.
///
/// ⚠️ Wat hier **niet** meeverhuist: de speeltijd die aan tale-kant al onder de oude naam
/// staat. Die boekhouding is van de server (per naam in kleine letters); deze correctie
/// stuurt enkel waar nieuwe tijd landt.
pub fn correct_hytale_name(pool: &DbPool, uid: &str, new_name: &str) -> Result<NameFix, String> {
    let uid = uid.trim();
    let new_name = new_name.trim();
    if uid.is_empty() || new_name.is_empty() {
        return Err("leeg account of lege naam".into());
    }
    let mut conn = pool.get().map_err(|e| e.to_string())?;

    // Wie hoort er nog bij? De koppeling loopt beide kanten op.
    let mut family = vec![uid.to_string()];
    if let Some(tid) = uid.strip_prefix("twitch:") {
        let mut stmt = conn
            .prepare("SELECT user_id FROM coins WHERE twitch_id = ?1")
            .map_err(|e| e.to_string())?;
        let found: Vec<String> = stmt
            .query_map(params![tid.trim()], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .collect();
        family.extend(found);
    } else {
        let tid: String = conn
            .query_row("SELECT twitch_id FROM coins WHERE user_id = ?1", params![uid], |r| {
                r.get::<_, String>(0)
            })
            .optional()
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
        if !tid.trim().is_empty() {
            family.push(format!("twitch:{}", tid.trim()));
        }
    }

    // Eerst opschrijven wat er stond: na de UPDATE is dat niet meer te achterhalen.
    let mut old: Vec<String> = Vec::new();
    for u in &family {
        for sql in [
            "SELECT hytale_name FROM coins WHERE user_id = ?1",
            "SELECT hytale_name FROM hytale_whitelist WHERE user_id = ?1",
        ] {
            let n: Option<String> = conn
                .query_row(sql, params![u], |r| r.get::<_, String>(0))
                .optional()
                .map_err(|e| e.to_string())?;
            let n = n.unwrap_or_default().trim().to_string();
            if !n.is_empty() && !old.iter().any(|o| o.eq_ignore_ascii_case(&n)) {
                old.push(n);
            }
        }
    }

    // Alles in één transactie: half gecorrigeerd is erger dan niet gecorrigeerd, want dan
    // staan de twee bronnen uit elkaar en splitst de klok alsnog.
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let mut touched: Vec<String> = Vec::new();
    for u in &family {
        // Enkel UPDATE, nooit INSERT: een `twitch:`-pseudo-account hoort geen `coins`-rij
        // te krijgen (dat zou een spookaccount in het leaderboard zetten).
        let a = tx
            .execute("UPDATE coins SET hytale_name = ?2 WHERE user_id = ?1", params![u, new_name])
            .map_err(|e| e.to_string())?;
        let b = tx
            .execute(
                "UPDATE hytale_whitelist SET hytale_name = ?2 WHERE user_id = ?1",
                params![u, new_name],
            )
            .map_err(|e| e.to_string())?;
        if a + b > 0 {
            touched.push(u.clone());
        }
    }
    if touched.is_empty() {
        // De transactie valt hiermee weg (rollback) — er viel toch niets te wijzigen.
        return Err(format!("onbekend account '{uid}' — niets bijgewerkt"));
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(NameFix { uids: touched, old })
}

/// Bewaar het Twitch-account dat aan dit Discord-lid hangt (leeg = ontkoppeld).
pub fn set_twitch_id(pool: &DbPool, uid: &str, username: &str, twitch_id: &str) {
    let conn = pool.get().expect("db");
    conn.execute(
        "INSERT INTO coins (user_id, username, twitch_id) VALUES (?1, ?2, ?3)
         ON CONFLICT(user_id) DO UPDATE SET twitch_id = excluded.twitch_id",
        params![uid, username, twitch_id],
    )
    .expect("set twitch_id");
}

/// De ooit-vastgezette Hytale-naam van een whitelist-rij, ongeacht of de pas nog geldig is.
/// Gebruikt om de Twitch-naam vast te houden tussen redeems (de 1e redeem zet ze vast).
pub fn get_whitelist_name(pool: &DbPool, uid: &str) -> Option<String> {
    let conn = pool.get().expect("db");
    conn.query_row(
        "SELECT hytale_name FROM hytale_whitelist WHERE user_id = ?1",
        params![uid],
        |r| r.get::<_, String>(0),
    )
    .optional()
    .expect("query whitelist name")
    .filter(|n| !n.is_empty())
}

/// Ken een tijdelijke pas toe: stapelt `add_secs` (de itemduur, normaal 24u) bovenop de
/// resterende tijd (reset niet). Al permanent ⇒ ongemoeid. Retourneert de nieuwe
/// vervaltijd (epoch). `add_secs` volgt de item-duur zodat een admin een testwaarde
/// (bv. 60s) kan zetten om verloop te testen.
pub fn grant_day_whitelist(pool: &DbPool, uid: &str, hytale_name: &str, add_secs: f64, now: f64) -> f64 {
    let conn = pool.get().expect("db");
    let existing: Option<Option<f64>> = conn
        .query_row(
            "SELECT expires FROM hytale_whitelist WHERE user_id = ?1",
            params![uid],
            |r| r.get::<_, Option<f64>>(0),
        )
        .optional()
        .expect("query whitelist expires");
    // Bestaande permanente grant: niets te stapelen.
    if let Some(None) = existing {
        return f64::INFINITY;
    }
    let base = match existing {
        Some(Some(exp)) if exp > now => exp,
        _ => now,
    };
    let new_exp = base + add_secs;
    conn.execute(
        "INSERT INTO hytale_whitelist (user_id, hytale_name, expires) VALUES (?1, ?2, ?3)
         ON CONFLICT(user_id) DO UPDATE SET hytale_name = excluded.hytale_name,
                                            expires = excluded.expires",
        params![uid, hytale_name, new_exp],
    )
    .expect("grant day whitelist");
    new_exp
}

/// Heeft een **ander** account deze Hytale-naam al vastgezet — als vastgelegde naam of via
/// een pas? Gebruikt als rem op de laatste terugval van de speeltijd-weergave: een
/// Discord-naam die toevallig gelijkloopt met een Hytale-naam mag nooit de tijd van iemand
/// anders tonen. Hoofdletter-ongevoelig, want beide kanten sleutelen op kleine letters.
pub fn hytale_name_claimed_by_other(pool: &DbPool, hytale_name: &str, uid: &str) -> bool {
    let n = hytale_name.trim().to_lowercase();
    if n.is_empty() {
        return false;
    }
    let conn = pool.get().expect("db");
    conn.query_row(
        "SELECT 1 FROM coins WHERE lower(hytale_name) = ?1 AND user_id <> ?2
         UNION ALL
         SELECT 1 FROM hytale_whitelist WHERE lower(hytale_name) = ?1 AND user_id <> ?2
         LIMIT 1",
        params![n, uid],
        |_| Ok(()),
    )
    .optional()
    .unwrap_or(None)
    .is_some()
}

/// Ken een permanente whitelist toe (permanente pas).
pub fn grant_perma_whitelist(pool: &DbPool, uid: &str, hytale_name: &str) {
    let conn = pool.get().expect("db");
    conn.execute(
        "INSERT INTO hytale_whitelist (user_id, hytale_name, expires) VALUES (?1, ?2, NULL)
         ON CONFLICT(user_id) DO UPDATE SET hytale_name = excluded.hytale_name,
                                            expires = NULL",
        params![uid, hytale_name],
    )
    .expect("grant perma whitelist");
}

/// Eén rij in het accounts-overzicht (manage → Accounts): een lid dat ooit iets
/// kocht, met zijn pas-status. Bewust minimaal — later uit te breiden met meer info.
pub struct AccountRow {
    pub user_id: String,
    pub username: String,
    pub hytale_name: String,
    /// `Some(secs)` = lopende dagpas met resterende seconden; `None` = geen actieve dagpas.
    pub day_pass_secs_left: Option<i64>,
    /// Permanente toegang (`coins.perma_access`).
    pub perma: bool,
}

/// Alle leden die ooit iets kochten (een inventory-item of een pas), met hun
/// pas-status. Bron = `inventory` ∪ `hytale_whitelist`; naam uit `coins.username`.
/// Gesorteerd alfabetisch op naam (NOCASE).
///
/// De Hytale-naam komt bij voorkeur van de **grant** (daar staat wat er naar de server
/// gaat), en anders uit `coins` — wie zijn naam op de site zette maar nog niets kocht,
/// heeft er nog geen grant-rij bij. Beide tonen is nodig sinds een admin die naam hier
/// kan rechtzetten: een leeg vakje naast een vastgezette naam zou misleiden.
pub fn list_accounts(pool: &DbPool, now: f64) -> Vec<AccountRow> {
    let conn = pool.get().expect("db");
    let mut stmt = conn
        .prepare(
            "SELECT b.user_id,
                    COALESCE(c.username, b.user_id) AS username,
                    COALESCE(NULLIF(w.hytale_name, ''), c.hytale_name, '') AS hytale_name,
                    w.expires                       AS expires,
                    COALESCE(c.perma_access, 0)     AS perma
               FROM (SELECT user_id FROM inventory
                     UNION
                     SELECT user_id FROM hytale_whitelist) b
               LEFT JOIN coins c            ON c.user_id = b.user_id
               LEFT JOIN hytale_whitelist w ON w.user_id = b.user_id
              ORDER BY username COLLATE NOCASE",
        )
        .expect("prepare list_accounts");
    let rows = stmt
        .query_map([], |r| {
            let expires: Option<f64> = r.get(3)?;
            let perma: i64 = r.get(4)?;
            // Dagpas = een verval-datum in de toekomst. Perma (expires NULL) of een
            // verlopen datum telt niet als lopende dagpas.
            let day_pass_secs_left = match expires {
                Some(e) if e > now => Some((e - now) as i64),
                _ => None,
            };
            Ok(AccountRow {
                user_id: r.get(0)?,
                username: r.get(1)?,
                hytale_name: r.get(2)?,
                day_pass_secs_left,
                perma: perma != 0,
            })
        })
        .expect("query list_accounts")
        .filter_map(|r| r.ok())
        .collect();
    rows
}

/// Ontgrendelde item-id's (bingokaart) van een lid.
pub fn owned_item_ids(pool: &DbPool, uid: &str) -> Vec<i64> {
    let conn = pool.get().expect("db");
    let mut stmt = conn
        .prepare("SELECT DISTINCT item_id FROM inventory WHERE user_id = ?1 AND item_id > 0")
        .expect("prepare owned");
    let rows = stmt
        .query_map(params![uid], |r| r.get::<_, i64>(0))
        .expect("query owned");
    rows.filter_map(Result::ok).collect()
}

/// Namen van alle gem-/kleuritems (category 'inventory'). Elk komt overeen met een
/// gelijknamige Discord-kleurrol; gebruikt om bij een gem-Use alle ándere kleurrollen
/// van het lid weg te halen (max één actieve gem-kleur).
pub fn inventory_item_names(pool: &DbPool) -> Vec<String> {
    let conn = pool.get().expect("db");
    let mut stmt = conn
        .prepare("SELECT name FROM items WHERE category = 'inventory'")
        .expect("prep inventory_item_names");
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .expect("query inventory_item_names");
    rows.filter_map(Result::ok).collect()
}

/// De naam van de momenteel "gebruikte" gem (voor de bijhorende Discord-rol). Leeg = geen.
pub fn get_equipped_gem(pool: &DbPool, uid: &str) -> String {
    let conn = pool.get().expect("db");
    conn.query_row(
        "SELECT equipped_gem FROM coins WHERE user_id = ?1",
        params![uid],
        |r| r.get(0),
    )
    .optional()
    .ok()
    .flatten()
    .unwrap_or_default()
}

/// De "gebruikte" gem-naam vastleggen (of leeg om te wissen).
pub fn set_equipped_gem(pool: &DbPool, uid: &str, gem: &str) {
    let conn = pool.get().expect("db");
    conn.execute(
        "UPDATE coins SET equipped_gem = ?2 WHERE user_id = ?1",
        params![uid, gem],
    )
    .ok();
}

/// Admin-testhulp: draai ALLE testaankopen terug. Stort de op elk gekocht item (gems +
/// passen/boosters) uitgegeven coins terug (o.b.v. de bewaarde aankoopprijs), maak de
/// inventory leeg, verwijder de Hytale-whitelist-grant + permanente toegang, en reset de
/// naamkleur/geëquipte gem. Zo kan je gems én passen testen zonder blijvend gevolg.
/// Retourneert het teruggestorte bedrag. Atomisch. (De persistente Hytale-naam blijft.)
pub fn reset_test_collection(pool: &DbPool, uid: &str) -> i64 {
    let mut conn = pool.get().expect("db");
    let tx = conn.transaction().expect("tx");
    let refund: i64 = tx
        .query_row(
            "SELECT COALESCE(SUM(price), 0) FROM inventory WHERE user_id = ?1",
            params![uid],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if refund > 0 {
        tx.execute(
            "UPDATE coins SET coins = coins + ?2 WHERE user_id = ?1",
            params![uid, refund],
        )
        .ok();
    }
    tx.execute("DELETE FROM inventory WHERE user_id = ?1", params![uid]).ok();
    // Pas-test terugdraaien: whitelist-grant weg (tale-bot reconcilet → van whitelist.json/panel).
    tx.execute("DELETE FROM hytale_whitelist WHERE user_id = ?1", params![uid]).ok();
    tx.execute(
        "UPDATE coins SET name_color = '', equipped_gem = '', perma_access = 0, chest_luck = 0 WHERE user_id = ?1",
        params![uid],
    )
    .ok();
    tx.commit().ok();
    refund
}

/// Uitkomst van een refund. `gem_role_removed` is niet-leeg als de web-laag nadien
/// nog een Discord-gem-rol moet intrekken (de db-laag kan niet async met Discord praten).
pub struct RefundOutcome {
    pub buyer_uid: String,
    pub item_name: String,
    pub amount: i64, // teruggestorte coins
    pub gem_role_removed: String,
}

/// Draai één shop-aankoop terug op basis van de logrij-id. Idempotent: een al
/// gerefunde/onbekende rij of een niet-shop-event levert `Err`. Coins gaan terug,
/// het item verlaat de inventory en de neveneffecten (whitelist/perma/gem-kleur) worden
/// mee teruggedraaid. De eventueel in te trekken Discord-gem-rol komt terug in de uitkomst.
pub fn refund_purchase(pool: &DbPool, log_id: i64) -> Result<RefundOutcome, String> {
    let mut conn = pool.get().expect("db");
    let tx = conn.transaction().map_err(|e| e.to_string())?;

    // Logrij ophalen + valideren: enkel een niet-gerefunde shop-aankoop is terugdraaibaar.
    let (category, event, uid, ref_id, amount, refunded, ts): (
        String,
        String,
        String,
        String,
        Option<i64>,
        i64,
        f64,
    ) = tx
        .query_row(
            "SELECT category, event, actor_uid, ref_id, amount, refunded, ts
               FROM server_log WHERE id = ?1",
            params![log_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or("Log entry not found.")?;
    if category != "shop" {
        return Err("Only shop purchases can be refunded.".into());
    }
    if refunded != 0 {
        return Err("This purchase was already refunded.".into());
    }
    let item_id: i64 = ref_id.parse().unwrap_or(0);
    if uid.is_empty() || item_id == 0 {
        return Err("This purchase predates refund support (no item reference).".into());
    }

    // De inventory-rij van DEZE aankoop: van hetzelfde item die met de dichtstbijzijnde
    // aankooptijd (`purchase` en `log_event` schrijven vlak na elkaar). Blind de oudste
    // rij pakken zou bij een tweede aankoop van hetzelfde item de verkeerde treffen.
    // Ontbreekt ze (item al verbruikt/gewist), dan refunden we alsnog en draaien we de
    // neveneffecten terug.
    let inv: Option<(i64, String, i64)> = tx
        .query_row(
            "SELECT id, name, price FROM inventory
               WHERE user_id = ?1 AND item_id = ?2
               ORDER BY ABS(acquired - ?3) LIMIT 1",
            params![uid, item_id, ts],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    // Terug te storten = het bedrag uit DEZE logrij, want dat is exact wat er toen
    // betaald is. `inventory.price` is enkel de terugval voor oude logrijen zonder
    // `amount`: die momentopname kan van een ándere aankoop van hetzelfde item komen
    // (prijzen wijzigen), en dan refund je het verkeerde bedrag.
    let (inv_id, item_name, price) = match inv {
        Some((id, name, p)) => (Some(id), name, amount.unwrap_or(p)),
        None => (None, String::new(), amount.unwrap_or(0)),
    };

    // Coins terug + inventory-rij (indien nog aanwezig) weg.
    if price > 0 {
        tx.execute("UPDATE coins SET coins = coins + ?2 WHERE user_id = ?1", params![uid, price])
            .ok();
    }
    if let Some(id) = inv_id {
        tx.execute("DELETE FROM inventory WHERE id = ?1", params![id]).ok();
    }

    // Neveneffecten terugdraaien op basis van het event-type.
    let mut gem_role_removed = String::new();
    match event.as_str() {
        // Pas-refund: whitelist-grant weg (tale-bot reconcilet). NB: dit trekt de héle
        // whitelist van dit lid in — bij gestapelde passen sneuvelt alles ineens.
        "pass_day" | "pass_perma" => {
            tx.execute("DELETE FROM hytale_whitelist WHERE user_id = ?1", params![uid]).ok();
            if event == "pass_perma" {
                tx.execute("UPDATE coins SET perma_access = 0 WHERE user_id = ?1", params![uid])
                    .ok();
            }
        }
        // Gewone aankoop: gem of booster.
        _ => {
            // Naam (voor de gem-vergelijking): liefst uit de inventory, anders uit items.
            let gem_name = if !item_name.is_empty() {
                item_name.clone()
            } else {
                tx.query_row(
                    "SELECT name FROM items WHERE id = ?1",
                    params![item_id],
                    |r| r.get::<_, String>(0),
                )
                .optional()
                .ok()
                .flatten()
                .unwrap_or_default()
            };
            // Was dit de geëquipte gem? Dan naamkleur + equipped wissen en de rol laten intrekken.
            let equipped: String = tx
                .query_row(
                    "SELECT equipped_gem FROM coins WHERE user_id = ?1",
                    params![uid],
                    |r| r.get(0),
                )
                .optional()
                .ok()
                .flatten()
                .unwrap_or_default();
            if !gem_name.is_empty() && equipped.eq_ignore_ascii_case(&gem_name) {
                tx.execute(
                    "UPDATE coins SET equipped_gem = '', name_color = '' WHERE user_id = ?1",
                    params![uid],
                )
                .ok();
                gem_role_removed = gem_name;
            }
            // Booster-refund: een eventueel nog actieve chest-luck-boost ongedaan maken.
            let cat: Option<String> = tx
                .query_row("SELECT category FROM items WHERE id = ?1", params![item_id], |r| {
                    r.get(0)
                })
                .optional()
                .map_err(|e| e.to_string())?;
            if cat.as_deref() == Some("booster") {
                tx.execute("UPDATE coins SET chest_luck = 0 WHERE user_id = ?1", params![uid]).ok();
            }
        }
    }

    // Logrij als gerefund markeren (knop verdwijnt, geen dubbele refund).
    tx.execute("UPDATE server_log SET refunded = 1 WHERE id = ?1", params![log_id]).ok();
    tx.commit().map_err(|e| e.to_string())?;

    let name = if item_name.is_empty() {
        format!("item #{item_id}")
    } else {
        item_name
    };
    Ok(RefundOutcome { buyer_uid: uid, item_name: name, amount: price, gem_role_removed })
}

/// Alle gems van een categorie ('primary'|'secondary'|'prism'), op positie.
#[allow(dead_code)]
pub fn gems_by_category(pool: &DbPool, category: &str) -> Vec<Item> {
    let conn = pool.get().expect("db");
    let mut stmt = conn
        .prepare(
            &format!(
                "SELECT {ITEM_COLS} FROM items WHERE category = ?1 ORDER BY position, id"
            ),
        )
        .expect("prepare gems_by_category");
    let rows = stmt.query_map(params![category], row_to_item).expect("query gems");
    rows.filter_map(Result::ok).collect()
}

/// Zet de naamkleur (hex) van een lid, of leeg om te resetten.
pub fn set_name_color(pool: &DbPool, uid: &str, username: &str, color: &str) {
    let conn = pool.get().expect("db");
    conn.execute(
        "INSERT INTO coins (user_id, username, name_color) VALUES (?1, ?2, ?3)
         ON CONFLICT(user_id) DO UPDATE SET name_color = excluded.name_color,
             username = excluded.username",
        params![uid, username, color],
    )
    .expect("set name_color");
}

/// Bewaar de Discord-profielkleur (hex) van een lid (uit de OAuth-login).
pub fn set_discord_color(pool: &DbPool, uid: &str, username: &str, hex: &str) {
    let conn = pool.get().expect("db");
    conn.execute(
        "INSERT INTO coins (user_id, username, discord_color) VALUES (?1, ?2, ?3)
         ON CONFLICT(user_id) DO UPDATE SET discord_color = excluded.discord_color,
             username = excluded.username",
        params![uid, username, hex],
    )
    .expect("set discord_color");
}

/// De Discord-profielkleur (hex) van een lid, of leeg.
pub fn get_discord_color(pool: &DbPool, uid: &str) -> String {
    let conn = pool.get().expect("db");
    conn.query_row(
        "SELECT discord_color FROM coins WHERE user_id = ?1",
        params![uid],
        |r| r.get(0),
    )
    .optional()
    .expect("query discord_color")
    .unwrap_or_default()
}

/// De naamkleur (hex) van een lid, of leeg.
pub fn get_name_color(pool: &DbPool, uid: &str) -> String {
    let conn = pool.get().expect("db");
    conn.query_row(
        "SELECT name_color FROM coins WHERE user_id = ?1",
        params![uid],
        |r| r.get(0),
    )
    .optional()
    .expect("query name_color")
    .unwrap_or_default()
}

/// Registreer een tijdelijke rol-toekenning die op `expires_at` weer weg moet.
/// (Passen gebruiken nu whitelist i.p.v. rollen; behouden voor gem-/kleurrollen.)
#[allow(dead_code)]
pub fn add_role_grant(pool: &DbPool, uid: &str, role_id: &str, expires_at: f64, label: &str) {
    let conn = pool.get().expect("db");
    conn.execute(
        "INSERT INTO role_grants (user_id, role_id, expires_at, label) VALUES (?1, ?2, ?3, ?4)",
        params![uid, role_id, expires_at, label],
    )
    .expect("add role_grant");
}

/// Actieve (nog niet verlopen) tijdelijke rollen van een lid: (label, expires_at).
pub fn active_grants(pool: &DbPool, uid: &str, now: f64) -> Vec<(String, f64)> {
    let conn = pool.get().expect("db");
    let mut stmt = conn
        .prepare(
            "SELECT label, expires_at FROM role_grants
             WHERE user_id = ?1 AND expires_at > ?2 ORDER BY expires_at ASC",
        )
        .expect("prepare active_grants");
    let rows = stmt
        .query_map(params![uid, now], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?))
        })
        .expect("query active_grants");
    rows.filter_map(Result::ok).collect()
}

/// Verlopen rol-toekenningen (id, user_id, role_id) op tijdstip `now`.
pub fn due_role_grants(pool: &DbPool, now: f64) -> Vec<(i64, String, String)> {
    let conn = pool.get().expect("db");
    let mut stmt = conn
        .prepare("SELECT id, user_id, role_id FROM role_grants WHERE expires_at <= ?1")
        .expect("prepare due grants");
    let rows = stmt
        .query_map(params![now], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
        })
        .expect("query due grants");
    rows.filter_map(Result::ok).collect()
}

pub fn delete_role_grant(pool: &DbPool, id: i64) {
    let conn = pool.get().expect("db");
    conn.execute("DELETE FROM role_grants WHERE id = ?1", params![id])
        .expect("delete role_grant");
}

// --- server-event-log (generiek, uitbreidbaar) --------------------------
// Eén rij per event. `category` groepeert (nu enkel 'chest', later 'coins',
// 'daily', 'admin', ...). `ref_id` bindt events van hetzelfde object samen
// (bv. alle join/win/despawn van één chest via het chest-bericht-id).

/// Eén te loggen event. Vul enkel de relevante velden; de rest blijft leeg/None.
#[derive(Default)]
pub struct LogEntry {
    pub category: String,
    pub event: String,
    pub actor_uid: String,
    pub actor_name: String,
    pub channel_id: String,
    pub ref_id: String,
    pub amount: Option<i64>,
    pub detail: String,
}

impl LogEntry {
    /// Kortere constructie: `LogEntry::new("chest", "join")` + `.actor(...)` enz.
    pub fn new(category: &str, event: &str) -> Self {
        LogEntry {
            category: category.into(),
            event: event.into(),
            ..Default::default()
        }
    }
    pub fn actor(mut self, uid: &str, name: &str) -> Self {
        self.actor_uid = uid.into();
        self.actor_name = name.into();
        self
    }
    pub fn channel(mut self, channel_id: u64) -> Self {
        self.channel_id = channel_id.to_string();
        self
    }
    pub fn reference(mut self, ref_id: u64) -> Self {
        self.ref_id = ref_id.to_string();
        self
    }
    pub fn amount(mut self, amount: i64) -> Self {
        self.amount = Some(amount);
        self
    }
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = detail.into();
        self
    }
}

/// Schrijf één event weg. Faalt nooit hard (logboek mag de bot niet doen crashen).
pub fn log_event(pool: &DbPool, now: f64, e: &LogEntry) {
    let Ok(conn) = pool.get() else { return };
    let _ = conn.execute(
        "INSERT INTO server_log
           (ts, category, event, actor_uid, actor_name, channel_id, ref_id, amount, detail)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![
            now, e.category, e.event, e.actor_uid, e.actor_name,
            e.channel_id, e.ref_id, e.amount, e.detail
        ],
    );
}

/// Eén rij uit het logboek (voor de adminpagina). Bevat bewust meer velden dan de
/// huidige UI toont (id/ref_id/… voor komende filters en per-chest-groepering).
#[allow(dead_code)]
pub struct LogRow {
    pub id: i64,
    pub ts: f64,
    pub category: String,
    pub event: String,
    pub actor_uid: String,
    pub actor_name: String,
    pub channel_id: String,
    pub ref_id: String,
    pub amount: Option<i64>,
    pub detail: String,
    pub refunded: bool,
}

/// De recentste events, nieuwste eerst. `category = None` = alle categorieën.
pub fn recent_log(pool: &DbPool, category: &[&str], limit: usize) -> Vec<LogRow> {
    let conn = pool.get().expect("db");
    let map = |r: &rusqlite::Row| {
        Ok(LogRow {
            id: r.get(0)?,
            ts: r.get(1)?,
            category: r.get(2)?,
            event: r.get(3)?,
            actor_uid: r.get(4)?,
            actor_name: r.get(5)?,
            channel_id: r.get(6)?,
            ref_id: r.get(7)?,
            amount: r.get(8)?,
            detail: r.get(9)?,
            refunded: r.get::<_, i64>(10)? != 0,
        })
    };
    let cols = "id, ts, category, event, actor_uid, actor_name, channel_id, ref_id, amount, detail, refunded";
    // Leeg = alles. Meerdere categorieën kunnen: één filterknop bundelt er soms een paar
    // (Inventory = gem + booster).
    if category.is_empty() {
        let mut stmt = conn
            .prepare(&format!("SELECT {cols} FROM server_log ORDER BY id DESC LIMIT ?1"))
            .expect("prepare recent_log");
        return stmt
            .query_map(params![limit as i64], map)
            .expect("query recent_log")
            .filter_map(Result::ok)
            .collect();
    }
    let holes = std::iter::repeat_n("?", category.len()).collect::<Vec<_>>().join(",");
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {cols} FROM server_log WHERE category IN ({holes}) ORDER BY id DESC LIMIT ?{}",
            category.len() + 1
        ))
        .expect("prepare recent_log");
    let mut vals: Vec<Box<dyn rusqlite::ToSql>> =
        category.iter().map(|c| Box::new(c.to_string()) as Box<dyn rusqlite::ToSql>).collect();
    vals.push(Box::new(limit as i64));
    stmt.query_map(rusqlite::params_from_iter(vals.iter()), map)
        .expect("query recent_log")
        .filter_map(Result::ok)
        .collect()
}

/// Alle voorkomende categorieën (voor de filterknoppen), alfabetisch.
pub fn log_categories(pool: &DbPool) -> Vec<String> {
    let conn = pool.get().expect("db");
    let mut stmt = conn
        .prepare("SELECT DISTINCT category FROM server_log ORDER BY category")
        .expect("prepare log_categories");
    stmt.query_map([], |r| r.get::<_, String>(0))
        .expect("query log_categories")
        .filter_map(Result::ok)
        .collect()
}

// --- admin-instelbare spelparameters ------------------------------------
// Alles hieronder wordt LIVE gelezen (elke aanroep = één query), zoals
// `is_coin_channel`. Geen cache: een wijziging via Manage → Settings geldt bij
// het eerstvolgende bericht/chest, zonder herstart. De type-kennis en de
// defaults zitten in `settings.rs`, niet hier.

/// Ruwe waarde van één setting; `None` = nooit gezet → `settings.rs` neemt de default.
pub fn setting_get(pool: &DbPool, key: &str) -> Option<String> {
    let conn = pool.get().expect("db");
    conn.query_row("SELECT value FROM settings WHERE key = ?1", params![key], |r| r.get(0))
        .optional()
        .expect("q setting")
}

pub fn setting_set(pool: &DbPool, key: &str, value: &str) {
    let conn = pool.get().expect("db");
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )
    .expect("set setting");
}

/// De gewogen coin-uitkomsten per bericht, oplopend op bedrag.
/// Leeg = de tabel is nooit geseed → `bot::coin_amount` gebruikt zijn vangnet.
pub fn coin_weights_all(pool: &DbPool) -> Vec<(i64, f64)> {
    let conn = pool.get().expect("db");
    let mut stmt = conn
        .prepare("SELECT amount, weight FROM coin_weights ORDER BY amount")
        .expect("prepare coin_weights");
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .expect("query coin_weights")
        .filter_map(Result::ok)
        .collect()
}

/// Voeg een uitkomst toe of wijzig het gewicht ervan (`amount` is de sleutel).
pub fn coin_weight_set(pool: &DbPool, amount: i64, weight: f64) {
    let conn = pool.get().expect("db");
    conn.execute(
        "INSERT INTO coin_weights (amount, weight) VALUES (?1, ?2)
         ON CONFLICT(amount) DO UPDATE SET weight = excluded.weight",
        params![amount, weight],
    )
    .expect("set coin_weight");
}

pub fn coin_weight_delete(pool: &DbPool, amount: i64) {
    let conn = pool.get().expect("db");
    conn.execute("DELETE FROM coin_weights WHERE amount = ?1", params![amount])
        .expect("del coin_weight");
}

/// De chest-prijsverdeling, in weergavevolgorde.
pub fn chest_tiers_all(pool: &DbPool) -> Vec<(i64, f64, i64, i64)> {
    let conn = pool.get().expect("db");
    let mut stmt = conn
        .prepare("SELECT id, weight, lo, hi FROM chest_tiers ORDER BY position, id")
        .expect("prepare chest_tiers");
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .expect("query chest_tiers")
        .filter_map(Result::ok)
        .collect()
}

/// Nieuwe tier onderaan; geeft het nieuwe id terug.
pub fn chest_tier_add(pool: &DbPool, weight: f64, lo: i64, hi: i64) -> i64 {
    let conn = pool.get().expect("db");
    let pos: i64 = conn
        .query_row("SELECT COALESCE(MAX(position), -1) + 1 FROM chest_tiers", [], |r| r.get(0))
        .expect("q tier pos");
    conn.execute(
        "INSERT INTO chest_tiers (weight, lo, hi, position) VALUES (?1, ?2, ?3, ?4)",
        params![weight, lo, hi, pos],
    )
    .expect("add chest_tier");
    conn.last_insert_rowid()
}

pub fn chest_tier_update(pool: &DbPool, id: i64, weight: f64, lo: i64, hi: i64) {
    let conn = pool.get().expect("db");
    conn.execute(
        "UPDATE chest_tiers SET weight = ?2, lo = ?3, hi = ?4 WHERE id = ?1",
        params![id, weight, lo, hi],
    )
    .expect("update chest_tier");
}

pub fn chest_tier_delete(pool: &DbPool, id: i64) {
    let conn = pool.get().expect("db");
    conn.execute("DELETE FROM chest_tiers WHERE id = ?1", params![id]).expect("del chest_tier");
}

// ---------------------------------------------------------------------------
// Retroactieve thread-inhaalslag (thread_backfill)
// ---------------------------------------------------------------------------

/// Leg het gerolde bedrag voor één thread-bericht vast. `INSERT OR IGNORE` op de
/// message_id: een bericht dat al gerold is (of al uitbetaald) blijft ongewijzigd —
/// zo rolt een her-scan nooit opnieuw. Geeft `true` als dit bericht nieuw was.
pub fn backfill_record(pool: &DbPool, message_id: &str, user_id: &str, name: &str, amount: i64, ts: f64) -> bool {
    let conn = pool.get().expect("db");
    let n = conn
        .execute(
            "INSERT OR IGNORE INTO thread_backfill (message_id, user_id, name, amount, applied, ts)
             VALUES (?1, ?2, ?3, ?4, 0, ?5)",
            params![message_id, user_id, name, amount, ts],
        )
        .expect("backfill_record");
    n > 0
}

/// Nog-niet-uitbetaalde inhaalslag, per lid: (user_id, naam, som, aantal berichten).
/// Aflopend op som. Voedt de preview in dev-coins.
pub fn backfill_pending(pool: &DbPool) -> Vec<(String, String, i64, i64)> {
    let conn = pool.get().expect("db");
    let mut stmt = conn
        .prepare(
            "SELECT user_id, MAX(name), SUM(amount), COUNT(*)
             FROM thread_backfill WHERE applied = 0
             GROUP BY user_id ORDER BY SUM(amount) DESC, MAX(name)",
        )
        .expect("prepare backfill_pending");
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .expect("query backfill_pending")
        .filter_map(|r| r.ok())
        .collect()
}

/// Totalen van de openstaande inhaalslag: (coins, berichten, leden).
pub fn backfill_totals(pool: &DbPool) -> (i64, i64, i64) {
    let conn = pool.get().expect("db");
    conn.query_row(
        "SELECT COALESCE(SUM(amount),0), COUNT(*), COUNT(DISTINCT user_id)
         FROM thread_backfill WHERE applied = 0",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .unwrap_or((0, 0, 0))
}

/// Betaal de openstaande inhaalslag uit. Per lid in ÉÉN IMMEDIATE-transactie: de som
/// als échte verdienste boeken (coins + total_earned, net als een level-cadeau) én de
/// betreffende rijen op `applied = 1` zetten — alles of niets. Bewust GÉÉN `earn_log`:
/// retro-coins mogen het weekly-/uur-overzicht niet vervuilen alsof ze deze week verdiend
/// zijn. Idempotent: al-uitbetaalde rijen tellen niet meer mee. Geeft (user_id, naam, som)
/// per uitbetaald lid terug.
pub fn backfill_apply(pool: &DbPool) -> Vec<(String, String, i64)> {
    let pending = backfill_pending(pool);
    let mut done = Vec::new();
    for (uid, name, sum, _cnt) in pending {
        if sum == 0 {
            // Niks te storten, maar wél afvinken zodat de preview leegloopt.
            let conn = pool.get().expect("db");
            conn.execute(
                "UPDATE thread_backfill SET applied = 1 WHERE user_id = ?1 AND applied = 0",
                params![uid],
            )
            .expect("backfill mark-zero");
            done.push((uid, name, 0));
            continue;
        }
        let mut conn = pool.get().expect("db");
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .expect("tx backfill_apply");
        tx.execute(
            "INSERT INTO coins (user_id, username, coins, max_balance, total_earned)
             VALUES (?1, ?2, ?3, ?3, ?3)
             ON CONFLICT(user_id) DO UPDATE SET
                 coins        = coins + excluded.coins,
                 username     = excluded.username,
                 max_balance  = MAX(max_balance, coins + excluded.coins),
                 total_earned = total_earned + excluded.coins",
            params![uid, name, sum],
        )
        .expect("credit backfill");
        tx.execute(
            "UPDATE thread_backfill SET applied = 1 WHERE user_id = ?1 AND applied = 0",
            params![uid],
        )
        .expect("mark applied");
        tx.commit().expect("commit backfill_apply");
        done.push((uid, name, sum));
    }
    done
}

/// Gooi de openstaande (nog niet uitbetaalde) inhaalslag weg — om opnieuw te rollen.
/// Raakt al-uitbetaalde rijen (applied=1) niet aan. Geeft het aantal gewiste rijen.
pub fn backfill_reset_pending(pool: &DbPool) -> usize {
    let conn = pool.get().expect("db");
    conn.execute("DELETE FROM thread_backfill WHERE applied = 0", [])
        .expect("backfill_reset_pending")
}

// ---------------------------------------------------------------------------
// Dry-run: gem → naamkleur (DB-kant). Speelt exact na wat use_gem/unequip_gem
// aan de databank doen (set_name_color + set_equipped_gem), plus de "Equipped"-
// match uit de render (color.eq_ignore_ascii_case(&name_color)). De Discord-rol
// zit hier bewust NIET in — die heeft een echte guild nodig ("echt testen").
// ---------------------------------------------------------------------------
#[cfg(test)]
mod gem_color_dryrun {
    use super::*;

    /// Verse, volledig gemigreerde DB op een uniek temp-bestand.
    fn fresh_db() -> (DbPool, std::path::PathBuf) {
        let p = std::env::temp_dir()
            .join(format!("market-gemtest-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&p);
        let pool = init_pool(p.to_str().unwrap());
        (pool, p)
    }

    /// De equip-badge-regel uit web.rs, hier los getest.
    fn is_equipped(item_color: &str, name_color: &str) -> bool {
        !item_color.is_empty() && item_color.eq_ignore_ascii_case(name_color)
    }

    #[test]
    fn gem_use_sets_and_swaps_and_clears_name_color() {
        let (pool, path) = fresh_db();
        let uid = "111";
        let name = "TestPlayer";

        // 0) Startsituatie: geen kleur, geen gem → swatch valt terug op default.
        assert_eq!(get_name_color(&pool, uid), "", "start: geen naamkleur");
        assert_eq!(get_equipped_gem(&pool, uid), "", "start: geen gem geëquipt");

        // 1) Use Rose Quartz  (simuleert de DB-writes van use_gem).
        let rose = "#E91E63";
        set_name_color(&pool, uid, name, rose);
        set_equipped_gem(&pool, uid, "Rose Quartz");
        assert_eq!(get_name_color(&pool, uid), rose, "kleur gezet");
        assert_eq!(get_equipped_gem(&pool, uid), "Rose Quartz", "gem geëquipt");

        // Render-kant: badge matcht ongeacht hoofdletters van de item-kleur.
        assert!(is_equipped("#e91e63", &get_name_color(&pool, uid)),
                "Rose Quartz toont als Equipped");
        assert!(!is_equipped("#3F51B5", &get_name_color(&pool, uid)),
                "een andere gem toont NIET als Equipped");

        // 2) Wissel naar Sapphire → oude gem laat los, nieuwe kleur staat.
        let sapphire = "#3F51B5";
        set_name_color(&pool, uid, name, sapphire);
        set_equipped_gem(&pool, uid, "Sapphire");
        assert_eq!(get_name_color(&pool, uid), sapphire, "kleur gewisseld");
        assert_eq!(get_equipped_gem(&pool, uid), "Sapphire");
        assert!(!is_equipped("#e91e63", &get_name_color(&pool, uid)),
                "Rose Quartz niet langer Equipped na wissel");
        assert!(is_equipped("#3f51b5", &get_name_color(&pool, uid)),
                "Sapphire nu Equipped");

        // 3) Unequip → beide velden leeg, swatch terug naar default.
        set_name_color(&pool, uid, name, "");
        set_equipped_gem(&pool, uid, "");
        assert_eq!(get_name_color(&pool, uid), "", "kleur gewist na unequip");
        assert_eq!(get_equipped_gem(&pool, uid), "", "gem losgelaten na unequip");
        assert!(!is_equipped("#3f51b5", &get_name_color(&pool, uid)),
                "geen enkele gem meer Equipped");

        // username mag onderweg niet stukgaan (upsert bewaart 'm).
        let uname: String = pool.get().unwrap()
            .query_row("SELECT username FROM coins WHERE user_id=?1",
                       params![uid], |r| r.get(0)).unwrap();
        assert_eq!(uname, name, "username blijft behouden");

        let _ = std::fs::remove_file(path);
    }
}

// ---------------------------------------------------------------------------
// Dry-run: Lucky Horseshoe = permanent verzamel-item. Bezit → altijd dubbele
// chest-kans; koop 1×; zeldzaam in de shop (1/N per dag, hier deterministisch
// getest bij N=1 → altijd, en fill met gems).
// ---------------------------------------------------------------------------
#[cfg(test)]
mod horseshoe_dryrun {
    use super::*;

    fn fresh(tag: &str) -> (DbPool, std::path::PathBuf) {
        let p = std::env::temp_dir()
            .join(format!("market-hstest-{}-{tag}.db", std::process::id()));
        let _ = std::fs::remove_file(&p);
        (init_pool(p.to_str().unwrap()), p)
    }

    fn horseshoe_id(pool: &DbPool) -> i64 {
        pool.get()
            .unwrap()
            .query_row("SELECT id FROM items WHERE category='booster' LIMIT 1", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn ownership_drives_double_odds_and_buy_is_once() {
        let (pool, path) = fresh("own");
        let uid = "77";
        let hid = horseshoe_id(&pool);

        // Geen bezit → gewicht 1.
        assert!(!owns_horseshoe(&pool, uid));
        assert_eq!(chest_weight(&pool, uid), 1);

        // Koop (seed-prijs = 7777, dus geef ruim saldo).
        award(&pool, uid, "Tester", 20_000, 0.0);
        purchase(&pool, uid, hid, 0.0).expect("eerste koop lukt");

        // Bezit → permanent gewicht 2.
        assert!(owns_horseshoe(&pool, uid));
        assert_eq!(chest_weight(&pool, uid), 2);

        // Max 1×: tweede koop faalt.
        assert!(purchase(&pool, uid, hid, 0.0).is_err(), "booster is maar 1× te bezitten");

        let _ = std::fs::remove_file(path);
    }

    /// Voeg `n` gems toe (gewicht = de default) en geef hun ids terug.
    fn gems(pool: &DbPool, namen: &[&str]) {
        let conn = pool.get().unwrap();
        for nm in namen {
            conn.execute(
                "INSERT INTO items (zone, name, price, color, category, description, position)
                 VALUES ('shelf', ?1, 100, '#fff', 'inventory', '', 0)",
                params![nm],
            )
            .unwrap();
        }
    }

    #[test]
    fn rotatie_neemt_alles_mee_dat_meedoet_en_blijft_stabiel() {
        let (pool, path) = fresh("lottery");
        gems(&pool, &["Ruby", "Sapphire"]);
        let hid = horseshoe_id(&pool);

        // 2 gems + horseshoe = 3 items voor 4 slots → alle drie staan er hoe dan ook.
        let offers = shop_offers(&pool, 1, 4);
        let ids: Vec<i64> = offers.iter().map(|it| it.id).collect();
        assert_eq!(ids.len(), 3, "meer slots dan items → gewoon alles");
        assert!(ids.contains(&hid), "de horseshoe doet mee in de rotatie");
        assert!(offers.iter().any(|it| it.category == "inventory"), "gems doen mee");

        // Dezelfde dag opnieuw = stabiel (cache), zelfde set (volgorde is niet gegarandeerd).
        let mut a = ids.clone();
        a.sort();
        let mut again: Vec<i64> = shop_offers(&pool, 1, 4).iter().map(|it| it.id).collect();
        again.sort();
        assert_eq!(a, again, "dagselectie is stabiel");

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn passen_staan_buiten_de_rotatie_en_een_uitgezet_item_wordt_nooit_getrokken() {
        let (pool, path) = fresh("pool");
        gems(&pool, &["Ruby", "Sapphire", "Topaz"]);
        let hid = horseshoe_id(&pool);
        // De pas wordt hier zelf gezet: passen worden manueel in Manage Shop beheerd, een
        // verse DB heeft er geen. Moet vóór `rotation_pool` staan, anders zegt de assert
        // hieronder enkel dat een net aangemaakt id nog niet in een oude lijst zat.
        let conn = pool.get().unwrap();
        conn.execute(
            "INSERT INTO items (zone, name, price, color, duration, category, description,
                                position, in_rotation)
             VALUES ('shelf', 'Testpas', 100, '#fff', 86400, 'boost', '', 0, 0)",
            [],
        )
        .unwrap();
        let pas = conn.last_insert_rowid();
        drop(conn);
        let in_pool: Vec<i64> = rotation_pool(&pool).into_iter().map(|(id, _)| id).collect();

        // Een pas zit niet in de pot, de gems en de horseshoe wel.
        assert!(!in_pool.contains(&pas), "een Hytale-pas hoort niet in de dagtrekking");
        assert!(in_pool.contains(&hid));

        // Uitzetten haalt hem uit de pot, maar bewaart het gewicht.
        set_item_rotation(&pool, hid, 2.0, false);
        assert!(!rotation_pool(&pool).iter().any(|(id, _)| *id == hid));
        assert_eq!(get_item(&pool, hid).unwrap().shop_weight, 2.0, "gewicht blijft bewaard");
        for dag in 0..40 {
            let ids: Vec<i64> = shop_offers(&pool, dag, 2).iter().map(|it| it.id).collect();
            assert!(!ids.contains(&hid), "uitgezet item mag nooit getrokken worden");
        }

        // Gewicht 0 met de vlag aan doet evenmin mee (anders: nooit getrokken, wel getoond).
        set_item_rotation(&pool, hid, 0.0, true);
        assert!(!rotation_pool(&pool).iter().any(|(id, _)| *id == hid));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn alles_uitgezet_geeft_een_lege_shop_en_schrijft_niets() {
        let (pool, path) = fresh("leeg");
        gems(&pool, &["Ruby", "Sapphire"]);
        {
            let conn = pool.get().unwrap();
            conn.execute("UPDATE items SET in_rotation = 0", []).unwrap();
        }
        assert!(shop_offers(&pool, 7, 4).is_empty(), "niets in de rotatie → lege dagshop");
        // En er is niets gepersisteerd, dus morgen (of na het terugzetten) klopt het weer.
        let opgeslagen: i64 = pool
            .get()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM daily_shop", [], |r| r.get(0))
            .unwrap();
        assert_eq!(opgeslagen, 0, "een lege trekking mag niets opslaan");

        let _ = std::fs::remove_file(path);
    }
}

/// De kansberekening achter het gewicht-vakje in Manage → Shop. `rotation_odds` rekent de
/// kans exact uit (integraal), terwijl `draw_weighted` de échte trekking doet — die twee
/// moeten hetzelfde opleveren, anders staat er een getal op het scherm dat niet klopt met
/// wat de shop doet. Daarom hier: formule vs. simulatie van de echte trekking.
#[cfg(test)]
mod rotation_odds_tests {
    use super::{draw_weighted, rotation_odds};

    /// Hoe vaak elk item in de selectie belandt, over `rondes` echte trekkingen.
    fn simuleer(weights: &[f64], n: usize, rondes: usize) -> Vec<f64> {
        let items: Vec<(i64, f64)> =
            weights.iter().enumerate().map(|(i, w)| (i as i64, *w)).collect();
        let mut tel = vec![0usize; weights.len()];
        for _ in 0..rondes {
            for id in draw_weighted(&items, n) {
                tel[id as usize] += 1;
            }
        }
        tel.iter().map(|t| *t as f64 / rondes as f64).collect()
    }

    #[test]
    fn formule_komt_overeen_met_de_echte_trekking() {
        // Realistisch geval: 12 gems van gewicht 10 + een zeldzame booster van 2, 4 slots.
        let mut w = vec![10.0; 12];
        w.push(2.0);
        let exact = rotation_odds(&w, 4);
        let gemeten = simuleer(&w, 4, 60_000);
        for (i, (e, g)) in exact.iter().zip(&gemeten).enumerate() {
            assert!(
                (e - g).abs() < 0.01,
                "item {i}: formule {e:.4} vs. gemeten {g:.4} (mag 1 procentpunt schelen)"
            );
        }
        // De som van alle kansen = het aantal slots (elke trekking vult er precies 4).
        let som: f64 = exact.iter().sum();
        assert!((som - 4.0).abs() < 0.01, "som van de kansen = aantal slots, was {som}");
        // En de booster is duidelijk zeldzamer dan een gem.
        assert!(exact[12] < exact[0] / 3.0, "gewicht 2 vs 10 → veel zeldzamer");
    }

    #[test]
    fn scherpe_verhoudingen_blijven_kloppen() {
        // Eén dominant item tegen vier kleintjes: hier wordt de integrand het steilst.
        let w = vec![100.0, 1.0, 1.0, 1.0, 1.0];
        let exact = rotation_odds(&w, 2);
        let gemeten = simuleer(&w, 2, 60_000);
        for (i, (e, g)) in exact.iter().zip(&gemeten).enumerate() {
            assert!((e - g).abs() < 0.015, "item {i}: formule {e:.4} vs. gemeten {g:.4}");
        }
        assert!(exact[0] > 0.98, "een gewicht van 100 tegen 1 pakt vrijwel altijd een slot");
    }

    #[test]
    fn randgevallen() {
        // Meer slots dan items → iedereen staat er zeker.
        assert_eq!(rotation_odds(&[3.0, 1.0], 5), vec![1.0, 1.0]);
        // Even zware items delen de kans gelijk: 2 van de 4 slots elk.
        let vier = rotation_odds(&[1.0, 1.0, 1.0, 1.0], 2);
        for p in &vier {
            assert!((p - 0.5).abs() < 1e-6, "gelijke gewichten → gelijke kans, kreeg {p}");
        }
        // Gewicht 0 = doet niet mee, en trekt de rest niet scheef.
        let met_nul = rotation_odds(&[1.0, 1.0, 0.0], 1);
        assert_eq!(met_nul[2], 0.0);
        assert!((met_nul[0] - 0.5).abs() < 1e-6);
        // Geen slots = niemand.
        assert_eq!(rotation_odds(&[1.0, 2.0], 0), vec![0.0, 0.0]);
    }
}

#[cfg(test)]
mod daily_atomic_guard {
    use super::*;

    fn fresh(tag: &str) -> (DbPool, std::path::PathBuf) {
        let p = std::env::temp_dir().join(format!("market-dailytest-{}-{tag}.db", std::process::id()));
        let _ = std::fs::remove_file(&p);
        (init_pool(p.to_str().unwrap()), p)
    }

    /// #7 — init_pool zet `busy_timeout` per verbinding (bewust GEEN WAL: zie init_pool-comment,
    /// het read-only panel leest coins.db rechtstreeks uit een niet-schrijfbare map).
    #[test]
    fn pool_sets_busy_timeout_and_no_wal() {
        let (pool, path) = fresh("pragma");
        let conn = pool.get().unwrap();
        let bt: i64 = conn.query_row("PRAGMA busy_timeout", [], |r| r.get(0)).unwrap();
        assert!(bt >= 5000, "busy_timeout moet gezet zijn (>=5000ms), was {bt}");
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_ne!(mode.to_lowercase(), "wal", "WAL bewust NIET aan (panel-read-compat)");
        let _ = std::fs::remove_file(path);
    }

    /// #1 — een tweede daily binnen de cooldown (de dubbelklik-race) wordt geweigerd
    /// door de WHERE-guard: geen dubbele uitbetaling, saldo blijft na één claim staan.
    #[test]
    fn second_daily_within_cooldown_is_refused() {
        let (pool, path) = fresh("guard");
        let uid = "42";
        let now = 1_000_000.0;
        let cooldown = 24.0 * 3600.0;
        let guard_ts = now - cooldown;

        // Eerste claim (gloednieuw lid) → geboekt.
        let t1 = award_daily(&pool, uid, "Tester", 50, 1, now, guard_ts);
        assert_eq!(t1, Some(50), "eerste daily boekt 50");

        // Tweede claim op exact hetzelfde moment (race, cooldown niet verstreken) → geweigerd.
        let t2 = award_daily(&pool, uid, "Tester", 50, 1, now, guard_ts);
        assert_eq!(t2, None, "tweede daily binnen cooldown wordt geweigerd");

        // Saldo staat nog op één claim: geen dubbele uitbetaling.
        let (coins, _m, _p, earned) = get_stats(&pool, uid);
        assert_eq!(coins, 50, "geen dubbele uitbetaling van coins");
        assert_eq!(earned, 50, "geen dubbele total_earned");

        // Ná de cooldown (nieuwe guard_ts) mag het weer.
        let later = now + cooldown + 1.0;
        let t3 = award_daily(&pool, uid, "Tester", 50, 2, later, later - cooldown);
        assert_eq!(t3, Some(100), "ná de cooldown boekt de volgende daily weer");

        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod concurrency_guards {
    use super::*;

    fn fresh(tag: &str) -> (DbPool, std::path::PathBuf) {
        let p = std::env::temp_dir().join(format!("market-guardtest-{}-{tag}.db", std::process::id()));
        let _ = std::fs::remove_file(&p);
        (init_pool(p.to_str().unwrap()), p)
    }

    /// #3 — tweede bericht binnen de cooldown wordt door de WHERE-guard geweigerd (geen dubbele award).
    #[test]
    fn award_if_ready_refuses_second_within_cooldown() {
        let (pool, path) = fresh("award");
        let uid = "9";
        let now = 1_000_000.0;
        let cooldown = 30.0;
        let guard = now - cooldown;

        assert_eq!(award_if_ready(&pool, uid, "T", 3, now, guard), Some(3), "eerste bericht boekt");
        assert_eq!(award_if_ready(&pool, uid, "T", 3, now, guard), None, "tweede binnen cooldown geweigerd");
        let (coins, _m, _p, earned) = get_stats(&pool, uid);
        assert_eq!((coins, earned), (3, 3), "geen dubbele coins/total_earned");

        let later = now + cooldown + 1.0;
        assert_eq!(
            award_if_ready(&pool, uid, "T", 3, later, later - cooldown),
            Some(6),
            "ná de cooldown boekt het volgende bericht weer"
        );
        let _ = std::fs::remove_file(path);
    }

    /// #2 — advance_gifted_level is een idempotente CAS: claimt de range één keer, daarna None.
    #[test]
    fn advance_gifted_level_claims_range_once() {
        let (pool, path) = fresh("levelup");
        let uid = "10";
        award(&pool, uid, "T", 100, 0.0); // maakt de coins-rij aan (marker start op 0)

        assert_eq!(advance_gifted_level(&pool, uid, 3), Some(0), "claimt [1,3], vorige marker 0");
        assert_eq!(advance_gifted_level(&pool, uid, 3), None, "zelfde doel → niets (al geclaimd)");
        assert_eq!(advance_gifted_level(&pool, uid, 2), None, "lager doel → niets");
        assert_eq!(advance_gifted_level(&pool, uid, 5), Some(3), "hoger doel → claimt [4,5], vorige 3");
        let _ = std::fs::remove_file(path);
    }

    /// #4 — admin_adjust: add accumuleert, set overschrijft, current/alltime los, prev correct terug.
    #[test]
    fn admin_adjust_add_set_and_returns_prev() {
        let (pool, path) = fresh("adjust");
        let uid = "11";
        award(&pool, uid, "T", 100, 0.0); // coins 100, total_earned 100

        // +50 op coins (current), earned ongemoeid; prev = (100,100).
        assert_eq!(admin_adjust(&pool, uid, "T", 50, false, true, false), (100, 100));
        let (c, _m, _p, e) = get_stats(&pool, uid);
        assert_eq!((c, e), (150, 100), "add telt bij coins, earned blijft");

        // set coins → 10.
        admin_adjust(&pool, uid, "T", 10, true, true, false);
        let (c, _m, _p, e) = get_stats(&pool, uid);
        assert_eq!((c, e), (10, 100), "set overschrijft coins");

        // +5 op alltime (earned), coins ongemoeid.
        admin_adjust(&pool, uid, "T", 5, false, false, true);
        let (c, _m, _p, e) = get_stats(&pool, uid);
        assert_eq!((c, e), (10, 105), "add op alltime raakt coins niet");
        let _ = std::fs::remove_file(path);
    }

    /// #10 — claim_level_gift betaalt éénmalig uit (claim + credit in één tx), en enkel aan de eigenaar.
    #[test]
    fn claim_level_gift_pays_once_to_owner() {
        let (pool, path) = fresh("gift");
        let uid = "20";
        award(&pool, uid, "T", 100, 0.0); // coins 100, total_earned 100
        let gid = create_level_gift(&pool, uid, 40, 5, "levelup", 0.0);

        // Verkeerde gebruiker → NotYours, niets uitbetaald.
        assert!(matches!(claim_level_gift(&pool, gid, "999", "X", 1.0), GiftClaim::NotYours));
        // Eigenaar claimt → Granted(40), coins+40 en total_earned+40 (verdienste).
        assert!(matches!(claim_level_gift(&pool, gid, uid, "T", 1.0), GiftClaim::Granted(40)));
        let (c, _m, _p, e) = get_stats(&pool, uid);
        assert_eq!((c, e), (140, 140), "cadeau geboekt als verdienste");
        // Tweede claim → AlreadyClaimed, geen dubbele uitbetaling.
        assert!(matches!(claim_level_gift(&pool, gid, uid, "T", 1.0), GiftClaim::AlreadyClaimed));
        let (c, _m, _p, e) = get_stats(&pool, uid);
        assert_eq!((c, e), (140, 140), "geen dubbele uitbetaling");
        // Onbestaand id → NotFound.
        assert!(matches!(claim_level_gift(&pool, 9999, uid, "T", 1.0), GiftClaim::NotFound));
        let _ = std::fs::remove_file(path);
    }

    /// #9a — get_session weigert (en ruimt) een sessie ouder dan max_age.
    #[test]
    fn get_session_expires_after_ttl() {
        let (pool, path) = fresh("session");
        let now = 1_000_000.0;
        let max_age = 100.0;
        create_session(&pool, "tok", "u1", "Name", now);

        // Binnen de TTL → geldig.
        assert_eq!(
            get_session(&pool, "tok", now + 50.0, max_age),
            Some(("u1".to_string(), "Name".to_string())),
            "verse sessie is geldig"
        );
        // Voorbij de TTL → geweigerd + opgeruimd.
        assert_eq!(get_session(&pool, "tok", now + max_age + 1.0, max_age), None, "verlopen sessie geweigerd");
        // Opgeruimd: ook binnen de TTL bevraagd is ze nu weg.
        assert_eq!(get_session(&pool, "tok", now, max_age), None, "verlopen sessie is verwijderd");
        let _ = std::fs::remove_file(path);
    }

    fn insert_gem(pool: &DbPool, name: &str) -> i64 {
        let conn = pool.get().unwrap();
        conn.execute(
            "INSERT INTO items (zone, name, price, color, category, description, position)
             VALUES ('shelf', ?1, 100, '#fff', 'inventory', '', 0)",
            params![name],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    /// #11 — add_stock bouwt voort op de huidige voorraad, behandelt -1 (onbeperkt) als 0 en vloert op 0.
    #[test]
    fn add_stock_accumulates_and_floors() {
        let (pool, path) = fresh("stock");
        let id = insert_gem(&pool, "Widget");
        set_stock_unlimited(&pool, id); // -1
        assert_eq!(add_stock(&pool, id, 5), 5, "onbeperkt(-1) + 5 → base 0 → 5");
        assert_eq!(add_stock(&pool, id, 3), 8, "5 + 3 → 8");
        assert_eq!(add_stock(&pool, id, -100), 0, "vloer op 0, nooit negatief");
        let _ = std::fs::remove_file(path);
    }

    /// #12 — shop_offers persisteert de dagselectie stabiel; getoonde set == opgeslagen set.
    #[test]
    fn shop_offers_is_stable_and_canonical() {
        let (pool, path) = fresh("shop");
        for nm in ["Ruby", "Sapphire", "Topaz"] {
            insert_gem(&pool, nm);
        }
        let day = 100;
        let ids1: Vec<i64> = shop_offers(&pool, day, 2).iter().map(|it| it.id).collect();
        assert_eq!(ids1.len(), 2, "twee gems getrokken (0 = geen booster)");
        // Zelfde dag opnieuw = exact dezelfde set en volgorde (stabiel gebufferd).
        let ids2: Vec<i64> = shop_offers(&pool, day, 2).iter().map(|it| it.id).collect();
        assert_eq!(ids1, ids2, "dagselectie is stabiel");
        // Wat getoond wordt == wat opgeslagen staat (canoniek, geen lokaal-gerolde mengeling).
        let stored = daily_ids(&pool.get().unwrap(), day);
        assert_eq!(stored, ids1, "opgeslagen set == getoonde set");
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod thread_backfill_test {
    use super::*;

    fn fresh(tag: &str) -> (DbPool, std::path::PathBuf) {
        let p = std::env::temp_dir().join(format!("market-tbf-{}-{tag}.db", std::process::id()));
        let _ = std::fs::remove_file(&p);
        (init_pool(p.to_str().unwrap()), p)
    }

    fn balance(pool: &DbPool, uid: &str) -> (i64, i64) {
        let conn = pool.get().unwrap();
        conn.query_row(
            "SELECT coins, total_earned FROM coins WHERE user_id = ?1",
            params![uid],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .unwrap()
        .unwrap_or((0, 0))
    }

    #[test]
    fn record_is_idempotent_pending_sums_and_apply_pays_once() {
        let (pool, path) = fresh("apply");

        // Twee leden, meerdere thread-berichten met bevroren rol-bedragen.
        assert!(backfill_record(&pool, "m1", "u1", "Alice", 3, 0.0));
        assert!(backfill_record(&pool, "m2", "u1", "Alice", 2, 0.0));
        assert!(backfill_record(&pool, "m3", "u2", "Bob", 5, 0.0));
        // Zelfde message_id opnieuw (her-scan) → geen nieuwe rij, geen dubbele coins.
        assert!(!backfill_record(&pool, "m1", "u1", "Alice", 99, 0.0));

        let (coins, msgs, users) = backfill_totals(&pool);
        assert_eq!((coins, msgs, users), (10, 3, 2));

        let pending = backfill_pending(&pool);
        // Aflopend op som: Bob (5) vóór Alice (5)? gelijk → tie-break op naam. Alice=5, Bob=5.
        let alice = pending.iter().find(|(u, ..)| u == "u1").unwrap();
        assert_eq!((alice.2, alice.3), (5, 2));

        // Uitbetalen: saldo + total_earned stijgen met de som, precies één keer.
        let done = backfill_apply(&pool);
        assert_eq!(done.len(), 2);
        assert_eq!(balance(&pool, "u1"), (5, 5));
        assert_eq!(balance(&pool, "u2"), (5, 5));

        // Niets meer openstaand; een tweede commit betaalt niets extra.
        assert_eq!(backfill_totals(&pool), (0, 0, 0));
        assert!(backfill_apply(&pool).is_empty());
        assert_eq!(balance(&pool, "u1"), (5, 5), "geen dubbele uitbetaling");

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn reset_drops_only_unpaid_rows() {
        let (pool, path) = fresh("reset");
        backfill_record(&pool, "a", "u1", "Alice", 4, 0.0);
        backfill_apply(&pool); // 'a' → applied=1
        backfill_record(&pool, "b", "u1", "Alice", 7, 0.0); // nieuw, nog niet betaald

        let removed = backfill_reset_pending(&pool);
        assert_eq!(removed, 1, "enkel de niet-uitbetaalde rij verdwijnt");
        assert_eq!(backfill_totals(&pool), (0, 0, 0));
        assert_eq!(balance(&pool, "u1"), (4, 4), "reeds uitbetaalde coins blijven staan");

        let _ = std::fs::remove_file(path);
    }
}

/// De chest-teller op de Coins-tab. Ze leest het logboek, dus de valkuil is dat er méér
/// chest-regels per lid bestaan dan enkel "meegedaan" en "gewonnen".
#[cfg(test)]
mod chest_counts_test {
    use super::*;

    fn fresh(tag: &str) -> (DbPool, std::path::PathBuf) {
        let p = std::env::temp_dir().join(format!("market-cc-{}-{tag}.db", std::process::id()));
        let _ = std::fs::remove_file(&p);
        (init_pool(p.to_str().unwrap()), p)
    }

    fn log(pool: &DbPool, event: &str, uid: &str, chest: u64) {
        log_event(
            pool,
            1.0,
            &LogEntry::new("chest", event).actor(uid, "Tester").reference(chest),
        );
    }

    #[test]
    fn telt_enkel_echte_deelnames_en_eigen_winsten() {
        let (pool, path) = fresh("telling");
        assert_eq!(chest_counts(&pool, "u1"), (0, 0), "nog niets gedaan");

        // Chest 1: meegedaan en gewonnen.
        log(&pool, "join", "u1", 1);
        log(&pool, "win", "u1", 1);
        // Chest 2: meegedaan, iemand anders won.
        log(&pool, "join", "u1", 2);
        log(&pool, "win", "u2", 2);
        // Chest 3: enkel een tweede klik en een te late klik → geen deelname.
        log(&pool, "already_in", "u1", 3);
        log(&pool, "too_late", "u1", 3);
        // Een spawn staat zonder actor in het log en mag niemand toegerekend worden.
        log_event(&pool, 1.0, &LogEntry::new("chest", "spawn").reference(4));
        // Een aankoop van hetzelfde lid hoort niet in deze telling thuis.
        log_event(&pool, 1.0, &LogEntry::new("shop", "buy").actor("u1", "Tester"));

        assert_eq!(chest_counts(&pool, "u1"), (2, 1), "2 meegeopend, 1 gewonnen");
        assert_eq!(chest_counts(&pool, "u2"), (0, 1), "wie enkel won: 0 deelnames gelogd");
        assert_eq!(chest_counts(&pool, "onbekend"), (0, 0));

        let _ = std::fs::remove_file(path);
    }
}

/// De brug van een Twitch-redeem naar het juiste Discord-lid. (De speeltijd-boekhouding
/// zelf hoort bij de tale-kant — zie `crate::pass_ledger`.)
#[cfg(test)]
mod pass_link_test {
    use super::*;

    fn fresh(tag: &str) -> (DbPool, std::path::PathBuf) {
        let p = std::env::temp_dir().join(format!("market-pl-{}-{tag}.db", std::process::id()));
        let _ = std::fs::remove_file(&p);
        (init_pool(p.to_str().unwrap()), p)
    }

    /// De rem op de laatste terugval van de speeltijd-weergave: een naam die een ánder
    /// account al opeist (vastgezet of via een pas) is niet van jou.
    #[test]
    fn een_naam_van_een_ander_account_is_bezet() {
        let (pool, path) = fresh("claimed");
        set_hytale_name(&pool, "u1", "Waldstein", "Waldstein");

        assert!(hytale_name_claimed_by_other(&pool, "waldstein", "u2"), "hoofdletters doen niet mee");
        assert!(!hytale_name_claimed_by_other(&pool, "Waldstein", "u1"), "van jezelf = niet bezet");
        assert!(!hytale_name_claimed_by_other(&pool, "Faybelle", "u2"), "niemand claimt die");
        assert!(!hytale_name_claimed_by_other(&pool, "  ", "u2"), "lege naam claimt niets");

        // Ook een pas-rij (zonder vastgezette naam in `coins`) legt beslag op de naam.
        grant_day_whitelist(&pool, "twitch:9", "Sigilien", 3600.0, 1000.0);
        assert!(hytale_name_claimed_by_other(&pool, "sigilien", "u2"));
        assert!(!hytale_name_claimed_by_other(&pool, "sigilien", "twitch:9"));

        let _ = std::fs::remove_file(path);
    }

    /// Een Twitch-pas hoort bij een Discord-lid zodra dat lid zijn Twitch in Discord
    /// koppelde — en bij niemand zolang dat niet zo is.
    #[test]
    fn twitch_pas_volgt_de_discord_koppeling() {
        let (pool, path) = fresh("link");
        let t0 = 1_000_000.0;
        grant_day_whitelist(&pool, "twitch:497218221", "Waldstein", 2.0 * 3600.0, t0);

        // Zonder koppeling weet market van niets: geen pas op de pagina. Ook niet via de
        // Hytale-naam — die typt de kijker zelf, dus daar kan iedereen andermans naam zetten.
        set_hytale_name(&pool, "disc1", "Waldstein", "Waldstein");
        assert!(get_whitelist_linked(&pool, "disc1", t0).is_none(), "naam alleen koppelt niet");

        // Iemand anders' Twitch-id mag er evenmin toe leiden.
        set_twitch_id(&pool, "disc2", "Vreemde", "999");
        assert!(get_whitelist_linked(&pool, "disc2", t0).is_none());

        // Met de echte koppeling verschijnt hij wél.
        set_twitch_id(&pool, "disc1", "Waldstein", "497218221");
        let p = get_whitelist_linked(&pool, "disc1", t0).expect("pas via twitch-koppeling");
        assert_eq!(p.hytale_name, "Waldstein");
        assert!(!p.is_permanent());

        let _ = std::fs::remove_file(path);
    }

    /// Twee passen tegelijk (gekochte dagpas + Twitch-redeem): permanent wint, anders de verste.
    #[test]
    fn beste_van_twee_passen_wint() {
        let (pool, path) = fresh("beste");
        let t0 = 1_000_000.0;
        grant_day_whitelist(&pool, "disc1", "Waldstein", 3600.0, t0);
        grant_day_whitelist(&pool, "twitch:1", "Waldstein", 5.0 * 3600.0, t0);
        set_twitch_id(&pool, "disc1", "Waldstein", "1");

        let p = get_whitelist_linked(&pool, "disc1", t0).expect("pas");
        assert!((p.expires.unwrap() - (t0 + 5.0 * 3600.0)).abs() < 1.0, "de verste wint");

        grant_perma_whitelist(&pool, "disc1", "Waldstein");
        let p = get_whitelist_linked(&pool, "disc1", t0).expect("pas");
        assert!(p.is_permanent(), "permanent wint");

        let _ = std::fs::remove_file(path);
    }

    /// Een verlopen grant telt niet mee.
    #[test]
    fn verlopen_grant_telt_niet_mee() {
        let (pool, path) = fresh("op");
        let t0 = 1_000_000.0;
        grant_day_whitelist(&pool, "u1", "Speler", 60.0, t0);
        assert!(get_whitelist_linked(&pool, "u1", t0 + 61.0).is_none());

        let _ = std::fs::remove_file(path);
    }

    /// De naam ligt vast zodra er érgens tijd op staat: haalt iemand eerst een
    /// Twitch-pas en koopt hij daarna op de site, dan moet die aankoop op dezélfde naam
    /// landen. Anders draait tale twee speeltijd-klokken en is de tijd onder de naam
    /// waarmee hij niet inlogt onbereikbaar.
    #[test]
    fn de_naam_ligt_vast_over_beide_bronnen_heen() {
        let (pool, path) = fresh("naamslot");
        let t0 = 1_000_000.0;
        // Kijker wisselt in op Twitch en zet daarmee 'Bob' vast.
        grant_day_whitelist(&pool, "twitch:42", "Bob", 2.0 * 3600.0, t0);

        // Zolang de accounts niet gekoppeld zijn, is er niets te weten: geen slot.
        assert_eq!(linked_twitch_name(&pool, "disc1"), None);
        assert_eq!(linked_discord_name(&pool, "42"), None);

        // Na de koppeling geldt 'Bob' ook op de site — dat is het slot dat de
        // aankoop-route en de login gebruiken.
        set_twitch_id(&pool, "disc1", "Bob", "42");
        assert_eq!(linked_twitch_name(&pool, "disc1"), Some("Bob".into()));

        // Andermans Twitch-id levert nooit een naam op.
        set_twitch_id(&pool, "disc2", "Vreemde", "999");
        assert_eq!(linked_twitch_name(&pool, "disc2"), None);

        let _ = std::fs::remove_file(path);
    }

    /// Andersom: wie op de site al een naam heeft en dán pas op Twitch inwisselt, krijgt
    /// die tijd op zijn bestaande naam — de Twitch-kant leest dit slot.
    #[test]
    fn de_site_naam_geldt_ook_voor_een_latere_redeem() {
        let (pool, path) = fresh("naamslot2");
        set_hytale_name(&pool, "disc1", "Bob", "Bob");

        // Zonder koppeling: niets te weten.
        assert_eq!(linked_discord_name(&pool, "42"), None);

        set_twitch_id(&pool, "disc1", "Bob", "42");
        assert_eq!(linked_discord_name(&pool, "42"), Some("Bob".into()));
        // Een leeg of onbekend Twitch-account geeft nooit een naam.
        assert_eq!(linked_discord_name(&pool, ""), None);
        assert_eq!(linked_discord_name(&pool, "  "), None);
        assert_eq!(linked_discord_name(&pool, "999"), None);

        // Een lid zonder ingevulde naam telt niet als slot.
        set_twitch_id(&pool, "disc3", "Leeg", "77");
        assert_eq!(linked_discord_name(&pool, "77"), None);

        let _ = std::fs::remove_file(path);
    }

    /// De admin-correctie moet **alle** sporen van de oude naam meenemen, aan beide kanten
    /// van de koppeling. Blijft er één staan, dan landt de volgende aankoop of redeem
    /// alsnog op de oude naam en splitst de speeltijd-klok toch nog.
    #[test]
    fn naamcorrectie_verzet_beide_kanten_van_de_koppeling() {
        let (pool, path) = fresh("naamfix");
        let t0 = 1_000_000.0;
        // Kijker typt zich mis bij zijn eerste redeem, koopt daarna ook op de site.
        grant_day_whitelist(&pool, "twitch:42", "Waldstien", 2.0 * 3600.0, t0);
        set_hytale_name(&pool, "disc1", "Waldstein", "Waldstien");
        grant_day_whitelist(&pool, "disc1", "Waldstien", 3600.0, t0);
        set_twitch_id(&pool, "disc1", "Waldstein", "42");

        let fix = correct_hytale_name(&pool, "disc1", " Waldstein ").expect("correctie");
        assert_eq!(fix.old, vec!["Waldstien".to_string()], "de oude naam wordt gerapporteerd");
        assert!(fix.uids.contains(&"twitch:42".to_string()), "de Twitch-grant gaat mee");

        assert_eq!(get_hytale_name(&pool, "disc1"), "Waldstein");
        assert_eq!(get_whitelist_name(&pool, "disc1"), Some("Waldstein".into()));
        assert_eq!(get_whitelist_name(&pool, "twitch:42"), Some("Waldstein".into()));

        // Andersom werkt even goed: corrigeren op de Twitch-rij raakt het Discord-lid.
        correct_hytale_name(&pool, "twitch:42", "Waldstein2").expect("correctie andersom");
        assert_eq!(get_hytale_name(&pool, "disc1"), "Waldstein2");
        assert_eq!(get_whitelist_name(&pool, "disc1"), Some("Waldstein2".into()));

        let _ = std::fs::remove_file(path);
    }

    /// Zonder koppeling blijft de correctie bij het gekozen account — market mag niet
    /// gokken dat twee vreemden dezelfde persoon zijn.
    #[test]
    fn naamcorrectie_blijft_bij_het_gekozen_account() {
        let (pool, path) = fresh("naamfix2");
        let t0 = 1_000_000.0;
        grant_day_whitelist(&pool, "twitch:42", "Bob", 3600.0, t0);
        set_hytale_name(&pool, "disc1", "Bob", "Bob"); // zelfde naam, géén koppeling

        let fix = correct_hytale_name(&pool, "twitch:42", "Bobby").expect("correctie");
        assert_eq!(fix.uids, vec!["twitch:42".to_string()]);
        assert_eq!(get_whitelist_name(&pool, "twitch:42"), Some("Bobby".into()));
        assert_eq!(get_hytale_name(&pool, "disc1"), "Bob", "de vreemde blijft ongemoeid");

        // Een Twitch-account krijgt nooit een eigen coins-rij van deze correctie: dat zou
        // een spookaccount in het leaderboard zetten.
        assert_eq!(get_hytale_name(&pool, "twitch:42"), "");

        // Een account dat nergens bestaat levert een fout op, geen stille no-op.
        assert!(correct_hytale_name(&pool, "spook", "Iets").is_err());
        assert!(correct_hytale_name(&pool, "disc1", "  ").is_err(), "lege naam is geen correctie");

        let _ = std::fs::remove_file(path);
    }

    /// Wie zijn naam op de site zette maar nog niets kocht, hoort ze op de Accounts-pagina
    /// tóch te zien staan — anders corrigeert een admin een leeg vakje naar iets nieuws
    /// terwijl er al een naam vastligt.
    #[test]
    fn accounts_toont_ook_een_naam_zonder_grant() {
        let (pool, path) = fresh("acclijst");
        set_hytale_name(&pool, "disc1", "Bob", "Bob");
        assert!(list_accounts(&pool, 1_000_000.0).is_empty(), "zonder aankoop geen rij");

        // Eén gekocht item volstaat om in de lijst te komen — een pas is er niet, dus er
        // is ook geen grant-rij die de naam kan aanleveren.
        pool.get()
            .unwrap()
            .execute(
                "INSERT INTO inventory (user_id, item_id, name, image, price, acquired)
                 VALUES ('disc1', 1, 'Gem', '', 10, 1.0)",
                [],
            )
            .unwrap();
        let rows = list_accounts(&pool, 1_000_000.0);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].hytale_name, "Bob", "de naam uit coins telt mee");

        let _ = std::fs::remove_file(path);
    }
}

/// De namenlijst per item: wie mag de Test Pass kopen.
#[cfg(test)]
mod item_allow_test {
    use super::*;

    fn fresh(tag: &str) -> (DbPool, std::path::PathBuf) {
        let p = std::env::temp_dir().join(format!("market-pa-{}-{tag}.db", std::process::id()));
        let _ = std::fs::remove_file(&p);
        (init_pool(p.to_str().unwrap()), p)
    }

    /// Toevoegen, dubbel toevoegen, verwijderen — en wie er niet op staat, staat er niet op.
    #[test]
    fn toevoegen_en_verwijderen() {
        let (pool, path) = fresh("basis");
        set_hytale_name(&pool, "u1", "Waldstein", "Waldstein");
        set_hytale_name(&pool, "u2", "FayBelle", "FayBelle");

        assert!(!item_allow_has(&pool, 7, "u1"), "lege lijst = niemand erop");
        assert!(item_allow_add(&pool, 7, "u1", "Waldstein", 100.0));
        assert!(item_allow_has(&pool, 7, "u1"));
        assert!(!item_allow_has(&pool, 7, "u2"), "de rest staat er niet op");

        // De lijst hangt aan het item: hetzelfde lid, een ander item = een andere vraag.
        assert!(!item_allow_has(&pool, 8, "u1"), "lijst van item 7 geldt niet voor item 8");

        // Twee keer dezelfde persoon is géén tweede rij — en zegt dat ook.
        assert!(!item_allow_add(&pool, 7, "u1", "Waldstein", 200.0), "dubbel = niets gedaan");
        assert_eq!(item_allow_list(&pool, 7).len(), 1);

        // Een lege uid is nooit een lid (een niet-ingevulde keuzelijst mag geen spookrij zetten).
        assert!(!item_allow_add(&pool, 7, "  ", "", 100.0));
        assert_eq!(item_allow_list(&pool, 7).len(), 1);

        assert!(item_allow_remove(&pool, 7, "u1"));
        assert!(!item_allow_has(&pool, 7, "u1"));
        assert!(!item_allow_remove(&pool, 7, "u1"), "twee keer verwijderen = niets meer te doen");

        let _ = std::fs::remove_file(path);
    }

    /// De weergegeven naam volgt `coins` (hernoemen op Discord), met de bewaarde naam
    /// als terugval voor een uid die daar niet (meer) in staat. Sortering op naam.
    /// De keuzelijst toont enkel leden mét een Hytale-naam.
    #[test]
    fn naam_volgt_coins_en_valt_anders_terug() {
        let (pool, path) = fresh("naam");
        set_hytale_name(&pool, "u1", "Zoe", "ZoeInGame");
        set_hytale_name(&pool, "u2", "Naamloos", "");
        item_allow_add(&pool, 7, "u1", "OudeNaam", 100.0);
        item_allow_add(&pool, 7, "u9", "Alleen hier gekend", 100.0);

        let l = item_allow_list(&pool, 7);
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].1, "Alleen hier gekend", "alfabetisch eerst");
        assert_eq!(l[1].1, "Zoe", "coins wint van de naam die bij het toevoegen bewaard werd");
        assert_eq!(l[1].2, "ZoeInGame", "de Hytale-naam komt erbij");

        // Zonder Hytale-naam valt er niets te whitelisten ⇒ niet te kiezen.
        let leden = members_with_hytale_name(&pool);
        assert_eq!(
            leden,
            vec![("u1".to_string(), "Zoe".to_string(), "ZoeInGame".to_string())]
        );

        let _ = std::fs::remove_file(path);
    }

    /// De verhuis van de oude server-brede testerslijst (`pass_allow`) naar de lijst per
    /// item: de namen komen op elke testpas terecht, en de oude tabel verdwijnt zodat er
    /// geen tweede lijst blijft staan die niemand meer leest.
    #[test]
    fn oude_testerslijst_verhuist_naar_de_testpas() {
        let (pool, path) = fresh("migratie");
        let id = add_item(&pool, "shelf", None);
        {
            let conn = pool.get().unwrap();
            // Bewust met de oude rommel erin: uitverkocht, een prijs, en in de rotatie.
            conn.execute(
                "UPDATE items SET test_pass = 1, sold_out = 1, price = 500, stock = 0,
                   in_rotation = 1 WHERE id = ?1",
                params![id],
            )
            .unwrap();
            conn.execute(
                "CREATE TABLE pass_allow (user_id TEXT PRIMARY KEY, username TEXT NOT NULL
                   DEFAULT '', added REAL NOT NULL DEFAULT 0)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO pass_allow (user_id, username, added) VALUES ('u1', 'Tester', 5.0)",
                [],
            )
            .unwrap();
        }
        // Tweede opstart = de migratie draait.
        let pool2 = init_pool(path.to_str().unwrap());
        assert!(item_allow_has(&pool2, id, "u1"), "de tester staat nu op de testpas");
        let it = get_item(&pool2, id).unwrap();
        assert_eq!((it.price, it.stock, it.sold_out, it.in_rotation), (0, -1, false, false),
            "gratis, onbeperkt, niet uitverkocht en niet in de dagtrekking");
        let weg: bool = pool2
            .get()
            .unwrap()
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name='pass_allow'",
                [],
                |_| Ok(()),
            )
            .optional()
            .unwrap()
            .is_none();
        assert!(weg, "de oude tabel is opgeruimd");

        let _ = std::fs::remove_file(path);
    }

    /// Een item wissen neemt zijn namenlijst mee: id's worden hergebruikt.
    #[test]
    fn item_wissen_neemt_de_lijst_mee() {
        let (pool, path) = fresh("del");
        let id = add_item(&pool, "shelf", None);
        item_allow_add(&pool, id, "u1", "Waldstein", 100.0);
        delete_item(&pool, id);
        assert!(item_allow_list(&pool, id).is_empty());

        let _ = std::fs::remove_file(path);
    }
}
