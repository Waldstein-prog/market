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
    ensure_column(&conn, "items", "role_id", "TEXT NOT NULL DEFAULT ''");
    ensure_column(&conn, "items", "duration", "INTEGER NOT NULL DEFAULT 0");
    ensure_column(&conn, "items", "category", "TEXT NOT NULL DEFAULT ''");
    ensure_column(&conn, "items", "description", "TEXT NOT NULL DEFAULT ''");
    ensure_column(&conn, "inventory", "item_id", "INTEGER NOT NULL DEFAULT 0");
    ensure_column(&conn, "role_grants", "label", "TEXT NOT NULL DEFAULT ''");
    // Hytale-tickets zijn boosts (voor de Boosts-tab).
    conn.execute(
        "UPDATE items SET category='boost' WHERE name IN ('Hytale Day Pass','Hytale Permanent Pass')",
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
    conn.execute("UPDATE coins SET max_balance = coins WHERE max_balance < coins", [])
        .expect("backfill max_balance");
    // total_earned kunnen we niet reconstrueren; als ondergrens het hoogste saldo ooit.
    conn.execute(
        "UPDATE coins SET total_earned = max_balance WHERE total_earned < max_balance",
        [],
    )
    .expect("backfill total_earned");
    drop(conn);
    seed_hytale(&pool);
    seed_gems(&pool);
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
    conn.query_row(
        "SELECT coins FROM coins WHERE user_id = ?1",
        params![user_id],
        |r| r.get(0),
    )
    .expect("query totaal")
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
         ORDER BY coins DESC, username ASC LIMIT ?1",
        limit,
    )
}

/// Leaderboard 'All-time' — (user_id, username, ooit verdiend) aflopend.
pub fn leaderboard_alltime(pool: &DbPool, limit: i64) -> Vec<(String, String, i64)> {
    lb_query(
        pool,
        "SELECT user_id, username, total_earned FROM coins
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

/// (username, coins) aflopend op coins, dan alfabetisch.
pub fn leaderboard(pool: &DbPool, limit: i64) -> Vec<(String, i64)> {
    let conn = pool.get().expect("db");
    let mut stmt = conn
        .prepare(
            "SELECT username, coins FROM coins
             ORDER BY coins DESC, username ASC LIMIT ?1",
        )
        .expect("prepare leaderboard");
    let rows = stmt
        .query_map(params![limit], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
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
    pub color: String,
    pub role_id: String,
    pub duration: i64, // 0 = permanent, >0 = seconden
    pub category: String,
    pub description: String,
}

fn row_to_item(r: &rusqlite::Row) -> rusqlite::Result<Item> {
    Ok(Item {
        id: r.get("id")?,
        name: r.get("name")?,
        price: r.get("price")?,
        image: r.get("image")?,
        color: r.get("color")?,
        role_id: r.get("role_id")?,
        duration: r.get("duration")?,
        category: r.get("category")?,
        description: r.get("description")?,
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
            "SELECT id, name, price, image, color, role_id, duration, category, description FROM items
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
            "SELECT id, name, price, image, color, role_id, duration, category, description FROM items
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
        "SELECT id, name, price, image, color, role_id, duration, category, description FROM items WHERE id = ?1",
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
    conn.execute(
        "INSERT INTO items (zone, shelf_id, name, price, position) VALUES (?1, ?2, '', 0, ?3)",
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

pub fn set_item_image(pool: &DbPool, id: i64, image: &str) {
    let conn = pool.get().expect("db");
    conn.execute("UPDATE items SET image = ?2 WHERE id = ?1", params![id, image])
        .expect("set image");
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
    let is_gem = matches!(item.category.as_str(), "primary" | "secondary" | "prism");
    let mut conn = pool.get().expect("db");
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    // Gems zijn bingokaart-slots: maar één keer te bezitten.
    if is_gem {
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

/// Alle gems van een categorie ('primary'|'secondary'|'prism'), op positie.
pub fn gems_by_category(pool: &DbPool, category: &str) -> Vec<Item> {
    let conn = pool.get().expect("db");
    let mut stmt = conn
        .prepare(
            "SELECT id, name, price, image, color, role_id, duration, category, description FROM items
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
