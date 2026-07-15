//! SQLite-persistentie voor de coin-economy (rusqlite + r2d2, zoals cyd).
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{OptionalExtension, params};

pub type DbPool = Pool<SqliteConnectionManager>;

pub fn init_pool(path: &str) -> DbPool {
    let manager = SqliteConnectionManager::file(path);
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
    // Lucky Horseshoe: 0 = geen boost, 1 = dubbele lot-kans bij de eerstvolgende
    // uitbetalende treasure chest waaraan het lid meedoet (nadien terug op 0).
    ensure_column(&conn, "coins", "chest_luck", "INTEGER NOT NULL DEFAULT 0");
    ensure_column(&conn, "admin_undo", "prev_earned", "INTEGER NOT NULL DEFAULT 0");
    ensure_column(&conn, "items", "role_id", "TEXT NOT NULL DEFAULT ''");
    ensure_column(&conn, "items", "duration", "INTEGER NOT NULL DEFAULT 0");
    ensure_column(&conn, "items", "category", "TEXT NOT NULL DEFAULT ''");
    ensure_column(&conn, "items", "description", "TEXT NOT NULL DEFAULT ''");
    ensure_column(&conn, "items", "image2", "TEXT NOT NULL DEFAULT ''");
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
    // Lucky Horseshoe is ALTIJD een 'booster' (herkoopbaar, geen grey-out, hoort op de
    // Boosters-tab, effect = chest-luck). Fix zowel de oude 'inventory'-migratie ALS een
    // handmatige mis-configuratie naar 'boost' — die laatste is de Hytale-pás-categorie:
    // met duration=0 zou kopen van een hoefijzer anders permanente Hytale-toegang geven!
    // Enkel de categorie wordt gecorrigeerd; prijs/afbeelding uit Manage blijven behouden.
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
    seed_hytale(&pool);
    // seed_gems is bewust NIET meer aangeroepen: items worden nu manueel beheerd in Manage
    // Shop, en de categorie-migratie hierboven zou een re-seed telkens naar 'inventory'
    // omzetten. Bestaande (geseede + eigen) items blijven gewoon staan.
    seed_horseshoe(&pool);
    pool
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
    conn.execute(
        "INSERT INTO items (zone, shelf_id, name, price, color, category, description, position)
         VALUES ('shelf', ?1, 'Lucky Horseshoe', 120, '#c9a227', 'booster',
                 'A lucky charm — boosts your fortune.', 0)",
        params![shelf_id],
    )
    .expect("seed horseshoe");
}

/// De dagelijkse shop-selectie: `n` items voor `day`, stabiel bewaard in
/// daily_shop. Pool = alle koopbare niet-boost items (gems + boosters).
/// (Tijdelijk ongebruikt: shop toont voorlopig enkel de Hytale-passen.)
#[allow(dead_code)]
pub fn shop_offers(pool: &DbPool, day: i64, n: i64) -> Vec<Item> {
    let conn = pool.get().expect("db");
    let mut ids: Vec<i64> = {
        let mut stmt = conn
            .prepare("SELECT item_id FROM daily_shop WHERE day = ?1")
            .expect("prepare daily_shop");
        stmt.query_map(params![day], |r| r.get::<_, i64>(0))
            .expect("query daily_shop")
            .filter_map(Result::ok)
            .collect()
    };
    if ids.is_empty() {
        let mut stmt = conn
            .prepare(
                "SELECT id FROM items
                 WHERE category != 'boost' AND category != '' ORDER BY RANDOM() LIMIT ?1",
            )
            .expect("prepare pick");
        ids = stmt
            .query_map(params![n], |r| r.get::<_, i64>(0))
            .expect("query pick")
            .filter_map(Result::ok)
            .collect();
        for id in &ids {
            conn.execute(
                "INSERT OR IGNORE INTO daily_shop (day, item_id) VALUES (?1, ?2)",
                params![day, id],
            )
            .expect("insert daily_shop");
        }
    }
    ids.iter().filter_map(|id| get_item(pool, *id)).collect()
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

/// Voeg de twee Hytale-tickets één keer toe (idempotent op naam): een dagpas
/// (24u) en een permanent ticket. `role_id` laat de admin invullen in Beheer.
fn seed_hytale(pool: &DbPool) {
    let conn = pool.get().expect("db");
    let exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM items WHERE name IN ('Hytale Day Pass','Hytale Permanent Pass')",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if exists > 0 {
        return;
    }
    // Eigen schap voor de Hytale-toegang, bovenaan (position -1).
    conn.execute(
        "INSERT INTO shelves (title, position) VALUES ('Hytale Access', -1)",
        [],
    )
    .expect("seed hytale shelf");
    let shelf_id = conn.last_insert_rowid();
    // Dagpas: blauw, 24u. Permanent: goud, permanent.
    conn.execute(
        "INSERT INTO items (zone, shelf_id, name, price, color, duration, category, description, position)
         VALUES ('shelf', ?1, 'Hytale Day Pass', 100, '#4a86e8', 86400, 'boost',
                 '24h access to the Hytale server.', 0)",
        params![shelf_id],
    )
    .expect("seed daypass");
    conn.execute(
        "INSERT INTO items (zone, shelf_id, name, price, color, duration, category, description, position)
         VALUES ('shelf', ?1, 'Hytale Permanent Pass', 1000, '#d4af37', 0, 'boost',
                 'Permanent access to the Hytale server.', 1)",
        params![shelf_id],
    )
    .expect("seed permpass");
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

/// (user_id, username) horend bij een sessie-token, indien geldig.
pub fn get_session(pool: &DbPool, token: &str) -> Option<(String, String)> {
    let conn = pool.get().expect("db");
    conn.query_row(
        "SELECT user_id, username FROM sessions WHERE token = ?1",
        params![token],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
    )
    .optional()
    .expect("query session")
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

fn current_coins(conn: &rusqlite::Connection, user_id: &str) -> i64 {
    conn.query_row(
        "SELECT coins FROM coins WHERE user_id = ?1",
        params![user_id],
        |r| r.get(0),
    )
    .optional()
    .expect("q coins")
    .unwrap_or(0)
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
    let conn = pool.get().expect("db");
    let (pc, pe): (i64, i64) = conn
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
    conn.execute(
        "INSERT INTO coins (user_id, username, coins, total_earned, max_balance) VALUES (?1, ?2, ?3, ?4, ?3)
         ON CONFLICT(user_id) DO UPDATE SET
             coins = ?3, total_earned = ?4,
             username = excluded.username,
             max_balance = MAX(max_balance, ?3)",
        params![user_id, username, coins, earned],
    )
    .expect("admin adjust");
    (pc, pe)
}

/// Tel een (mogelijk negatief) bedrag bij het saldo; returnt het vorige saldo.
pub fn admin_add_coins(pool: &DbPool, user_id: &str, username: &str, delta: i64) -> i64 {
    let conn = pool.get().expect("db");
    let prev = current_coins(&conn, user_id);
    let newv = prev + delta;
    conn.execute(
        "INSERT INTO coins (user_id, username, coins, max_balance) VALUES (?1, ?2, ?3, ?3)
         ON CONFLICT(user_id) DO UPDATE SET
             coins = ?3,
             username = excluded.username,
             max_balance = MAX(max_balance, ?3)",
        params![user_id, username, newv],
    )
    .expect("admin add coins");
    prev
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
            "SELECT e.user_id, COALESCE(c.username, e.user_id), SUM(e.amount) AS total
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
pub fn award_daily(
    pool: &DbPool,
    user_id: &str,
    username: &str,
    amount: i64,
    streak: i64,
    ts: f64,
) -> i64 {
    let conn = pool.get().expect("db");
    conn.execute(
        "INSERT INTO coins (user_id, username, coins, last_daily, daily_streak, max_balance, total_earned)
         VALUES (?1, ?2, ?3, ?4, ?5, ?3, ?3)
         ON CONFLICT(user_id) DO UPDATE SET
             coins        = coins + excluded.coins,
             username     = excluded.username,
             last_daily   = excluded.last_daily,
             daily_streak = excluded.daily_streak,
             max_balance  = MAX(max_balance, coins + excluded.coins),
             total_earned = total_earned + excluded.coins",
        params![user_id, username, amount, ts, streak],
    )
    .expect("insert daily");
    log_earn_event(&conn, user_id, amount, ts);
    conn.query_row(
        "SELECT coins FROM coins WHERE user_id = ?1",
        params![user_id],
        |r| r.get(0),
    )
    .expect("query totaal")
}

/// (saldo, hoogste saldo ooit, publiek?, ooit verdiend) voor de Coins-tab.
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
}

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
        .prepare(
            "SELECT id, name, price, image, image2, color, role_id, duration, category, description, zone, shelf_id FROM items
             WHERE zone = 'shelf' AND shelf_id = ?1 ORDER BY position, id",
        )
        .expect("prepare shelf_items");
    let rows = stmt.query_map(params![shelf_id], row_to_item).expect("query");
    rows.filter_map(Result::ok).collect()
}

/// Alle lucky-items, op positie.
pub fn lucky_items(pool: &DbPool) -> Vec<Item> {
    let conn = pool.get().expect("db");
    let mut stmt = conn
        .prepare(
            "SELECT id, name, price, image, image2, color, role_id, duration, category, description, zone, shelf_id FROM items
             WHERE zone = 'lucky' ORDER BY position, id",
        )
        .expect("prepare lucky_items");
    let rows = stmt.query_map([], row_to_item).expect("query lucky");
    rows.filter_map(Result::ok).collect()
}

/// Eén item ophalen (voor image-vervanging e.d.).
pub fn get_item(pool: &DbPool, id: i64) -> Option<Item> {
    let conn = pool.get().expect("db");
    conn.query_row(
        "SELECT id, name, price, image, image2, color, role_id, duration, category, description, zone, shelf_id FROM items WHERE id = ?1",
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
) {
    let conn = pool.get().expect("db");
    conn.execute(
        "UPDATE items SET name = ?2, price = ?3, role_id = ?4, duration = ?5,
             category = ?6, description = ?7 WHERE id = ?1",
        params![id, name, price, role_id, duration, category, description],
    )
    .expect("update item");
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

pub fn delete_item(pool: &DbPool, id: i64) {
    let conn = pool.get().expect("db");
    conn.execute("DELETE FROM items WHERE id = ?1", params![id])
        .expect("del item");
}

// --- kopen & ontgrendelen -----------------------------------------------

/// Koop/ontgrendel `item_id` voor `uid`: elk item kan maar één keer bezeten
/// worden (bingokaart). Controleert saldo, trekt de prijs af en ontgrendelt
/// het item. Atomisch. Ok(nieuw_saldo, item) of Err(reden).
pub fn purchase(pool: &DbPool, uid: &str, item_id: i64, ts: f64) -> Result<(i64, Item), String> {
    let item = get_item(pool, item_id).ok_or("This item no longer exists.")?;
    let mut conn = pool.get().expect("db");
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    // Inventory-items zijn verzamelkaart-slots: maar één keer te bezitten.
    if item.category == "inventory" {
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
    // Dagpas is nutteloos zodra je permanente toegang hebt.
    if item.category == "boost" && item.duration > 0 {
        let perma: i64 = tx
            .query_row(
                "SELECT perma_access FROM coins WHERE user_id = ?1",
                params![uid],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?
            .unwrap_or(0);
        if perma != 0 {
            return Err("You already have permanent access.".to_string());
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

/// Staat er nu een chest-luck-boost (Lucky Horseshoe) klaar voor dit lid?
pub fn has_chest_luck(pool: &DbPool, uid: &str) -> bool {
    let conn = pool.get().expect("db");
    conn.query_row(
        "SELECT chest_luck FROM coins WHERE user_id = ?1",
        params![uid],
        |r| r.get::<_, i64>(0),
    )
    .optional()
    .ok()
    .flatten()
    .unwrap_or(0)
        > 0
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

/// Het lot-gewicht van dit lid bij een treasure-chest-trekking: 2 met een actieve
/// Lucky Horseshoe, anders 1.
pub fn chest_weight(pool: &DbPool, uid: &str) -> u32 {
    if has_chest_luck(pool, uid) {
        2
    } else {
        1
    }
}

/// Verbruik de chest-luck-boost (na een uitbetalende chest). Idempotent.
pub fn clear_chest_luck(pool: &DbPool, uid: &str) {
    let conn = pool.get().expect("db");
    conn.execute(
        "UPDATE coins SET chest_luck = 0 WHERE user_id = ?1",
        params![uid],
    )
    .ok();
}

/// Gebruik een Lucky Horseshoe: verbruik één exemplaar uit de inventory en zet de
/// chest-luck-boost aan. Atomisch. Retourneert:
///   Ok(true)  = geactiveerd,
///   Ok(false) = er stond al een boost klaar (niets verbruikt, geen verspilling),
///   Err(_)    = geen exemplaar in bezit.
pub fn activate_horseshoe(pool: &DbPool, uid: &str, item_id: i64) -> Result<bool, String> {
    let mut conn = pool.get().expect("db");
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let already: i64 = tx
        .query_row(
            "SELECT chest_luck FROM coins WHERE user_id = ?1",
            params![uid],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .unwrap_or(0);
    if already > 0 {
        return Ok(false); // al actief — geen tweede hoefijzer opbranden
    }
    let row_id: Option<i64> = tx
        .query_row(
            "SELECT id FROM inventory WHERE user_id = ?1 AND item_id = ?2 LIMIT 1",
            params![uid, item_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some(row_id) = row_id else {
        return Err("You don't own a Lucky Horseshoe.".to_string());
    };
    tx.execute("DELETE FROM inventory WHERE id = ?1", params![row_id])
        .map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE coins SET chest_luck = 1 WHERE user_id = ?1",
        params![uid],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(true)
}

/// Zet de permanente-toegangsvlag (na gebruik van de permanente pas).
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

/// Bezeten booster-items (categorie 'booster', bv. Lucky Horseshoe) met het aantal.
/// (item_id, naam, afbeelding, kleur, aantal). Enkel wat de user effectief bezit (aantal > 0).
pub fn owned_booster_items(pool: &DbPool, uid: &str) -> Vec<(i64, String, String, String, i64)> {
    let conn = pool.get().expect("db");
    let mut stmt = conn
        .prepare(
            "SELECT i.id, i.name, i.image, i.color, COUNT(inv.id) FROM items i
               JOIN inventory inv ON inv.item_id = i.id AND inv.user_id = ?1
              WHERE i.category = 'booster'
              GROUP BY i.id HAVING COUNT(inv.id) > 0 ORDER BY i.name",
        )
        .expect("prep boosters");
    stmt.query_map(params![uid], |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
    })
    .expect("q boosters")
    .filter_map(Result::ok)
    .collect()
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
            "SELECT id, name, price, image, image2, color, role_id, duration, category, description, zone, shelf_id FROM items
             WHERE category = ?1 ORDER BY position, id",
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
