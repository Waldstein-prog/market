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
            max_balance INTEGER NOT NULL DEFAULT 0,
            is_public   INTEGER NOT NULL DEFAULT 0
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
            position INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS inventory (
            id       INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id  TEXT NOT NULL,
            name     TEXT NOT NULL DEFAULT '',
            image    TEXT NOT NULL DEFAULT '',
            price    INTEGER NOT NULL DEFAULT 0,
            acquired REAL NOT NULL DEFAULT 0
        );",
    )
    .expect("kan tabel niet aanmaken");

    // Migratie voor bestaande DB's: kolommen toevoegen indien nog afwezig,
    // en max_balance backfillen op het huidige saldo (anders start het op 0).
    ensure_column(&conn, "coins", "last_daily", "REAL NOT NULL DEFAULT 0");
    ensure_column(&conn, "coins", "max_balance", "INTEGER NOT NULL DEFAULT 0");
    ensure_column(&conn, "coins", "is_public", "INTEGER NOT NULL DEFAULT 0");
    conn.execute("UPDATE coins SET max_balance = coins WHERE max_balance < coins", [])
        .expect("backfill max_balance");
    drop(conn);
    seed_shop(&pool);
    pool
}

/// Vul de shop één keer met test-gems (4 kleur-schappen van 5) als hij leeg is.
fn seed_shop(pool: &DbPool) {
    let conn = pool.get().expect("db");
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM shelves", [], |r| r.get(0))
        .unwrap_or(0);
    if count > 0 {
        return;
    }
    let colors = [
        ("Yellow", "#e8c34a"),
        ("Red", "#d1543f"),
        ("Blue", "#4a86e8"),
        ("Green", "#57b368"),
    ];
    for (pos, (cname, hex)) in colors.iter().enumerate() {
        conn.execute(
            "INSERT INTO shelves (title, position) VALUES (?1, ?2)",
            params![format!("{cname} Gems"), pos as i64],
        )
        .expect("seed shelf");
        let shelf_id = conn.last_insert_rowid();
        for (i, letter) in ["A", "B", "C", "D", "E"].iter().enumerate() {
            conn.execute(
                "INSERT INTO items (zone, shelf_id, name, price, color, position)
                 VALUES ('shelf', ?1, ?2, ?3, ?4, ?5)",
                params![
                    shelf_id,
                    format!("Gem {cname} {letter}"),
                    (i as i64 + 1) * 5,
                    hex,
                    i as i64
                ],
            )
            .expect("seed item");
        }
    }
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
        "INSERT INTO coins (user_id, username, coins, last_award, max_balance)
         VALUES (?1, ?2, ?3, ?4, ?3)
         ON CONFLICT(user_id) DO UPDATE SET
             coins       = coins + excluded.coins,
             username    = excluded.username,
             last_award  = excluded.last_award,
             max_balance = MAX(max_balance, coins + excluded.coins)",
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

/// Daily-beloning: tel `amount` bij, zet last_daily (eigen 24u-cooldown),
/// houd max_balance bij. Returnt het nieuwe totaal.
pub fn award_daily(pool: &DbPool, user_id: &str, username: &str, amount: i64, ts: f64) -> i64 {
    let conn = pool.get().expect("db");
    conn.execute(
        "INSERT INTO coins (user_id, username, coins, last_daily, max_balance)
         VALUES (?1, ?2, ?3, ?4, ?3)
         ON CONFLICT(user_id) DO UPDATE SET
             coins       = coins + excluded.coins,
             username    = excluded.username,
             last_daily  = excluded.last_daily,
             max_balance = MAX(max_balance, coins + excluded.coins)",
        params![user_id, username, amount, ts],
    )
    .expect("insert daily");
    conn.query_row(
        "SELECT coins FROM coins WHERE user_id = ?1",
        params![user_id],
        |r| r.get(0),
    )
    .expect("query totaal")
}

/// (saldo, hoogste saldo ooit, publiek?) voor de banking-pagina.
pub fn get_stats(pool: &DbPool, user_id: &str) -> (i64, i64, bool) {
    let conn = pool.get().expect("db");
    conn.query_row(
        "SELECT coins, max_balance, is_public FROM coins WHERE user_id = ?1",
        params![user_id],
        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)? != 0)),
    )
    .optional()
    .expect("query stats")
    .unwrap_or((0, 0, false))
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
pub fn public_leaderboard(pool: &DbPool, limit: i64) -> Vec<(String, String, i64, i64)> {
    let conn = pool.get().expect("db");
    let mut stmt = conn
        .prepare(
            "SELECT user_id, username, coins, max_balance FROM coins
             WHERE is_public = 1 ORDER BY coins DESC, username ASC LIMIT ?1",
        )
        .expect("prepare public_leaderboard");
    let rows = stmt
        .query_map(params![limit], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })
        .expect("query public_leaderboard");
    rows.filter_map(Result::ok).collect()
}

/// De publieke recordhouder: (user_id, username, max_balance) met het hoogste
/// max_balance ooit onder de leden die publiek staan. None als niemand publiek staat.
pub fn public_record(pool: &DbPool) -> Option<(String, String, i64)> {
    let conn = pool.get().expect("db");
    conn.query_row(
        "SELECT user_id, username, max_balance FROM coins
         WHERE is_public = 1 ORDER BY max_balance DESC, username ASC LIMIT 1",
        [],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)),
    )
    .optional()
    .expect("query public_record")
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
}

fn row_to_item(r: &rusqlite::Row) -> rusqlite::Result<Item> {
    Ok(Item {
        id: r.get("id")?,
        name: r.get("name")?,
        price: r.get("price")?,
        image: r.get("image")?,
        color: r.get("color")?,
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
            "SELECT id, name, price, image, color FROM items
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
            "SELECT id, name, price, image, color FROM items
             WHERE zone = 'lucky' ORDER BY position, id",
        )
        .expect("prepare lucky_items");
    let rows = stmt.query_map([], row_to_item).expect("query lucky");
    rows.filter_map(Result::ok).collect()
}

/// `n` willekeurige shelf-items voor de Daily picks.
pub fn random_items(pool: &DbPool, n: i64) -> Vec<Item> {
    let conn = pool.get().expect("db");
    let mut stmt = conn
        .prepare(
            "SELECT id, name, price, image, color FROM items
             WHERE zone = 'shelf' ORDER BY RANDOM() LIMIT ?1",
        )
        .expect("prepare random_items");
    let rows = stmt.query_map(params![n], row_to_item).expect("query random");
    rows.filter_map(Result::ok).collect()
}

/// Eén item ophalen (voor image-vervanging e.d.).
pub fn get_item(pool: &DbPool, id: i64) -> Option<Item> {
    let conn = pool.get().expect("db");
    conn.query_row(
        "SELECT id, name, price, image, color FROM items WHERE id = ?1",
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

pub fn update_item(pool: &DbPool, id: i64, name: &str, price: i64) {
    let conn = pool.get().expect("db");
    conn.execute(
        "UPDATE items SET name = ?2, price = ?3 WHERE id = ?1",
        params![id, name, price],
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

// --- kopen & inventory --------------------------------------------------

/// Koop `item_id` voor `uid`: controleer saldo, trek de prijs af en leg het
/// item (snapshot van naam/afbeelding/prijs) in de inventory. Atomisch.
/// Ok(nieuw_saldo) of Err(reden).
pub fn purchase(pool: &DbPool, uid: &str, item_id: i64, ts: f64) -> Result<i64, String> {
    let item = get_item(pool, item_id).ok_or("Dit item bestaat niet meer.")?;
    let mut conn = pool.get().expect("db");
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let balance: i64 = tx
        .query_row("SELECT coins FROM coins WHERE user_id = ?1", params![uid], |r| r.get(0))
        .optional()
        .map_err(|e| e.to_string())?
        .unwrap_or(0);
    if balance < item.price {
        return Err(format!(
            "Niet genoeg coins: je hebt {balance}, {} kost {}.",
            item.name, item.price
        ));
    }
    tx.execute(
        "UPDATE coins SET coins = coins - ?2 WHERE user_id = ?1",
        params![uid, item.price],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT INTO inventory (user_id, name, image, price, acquired)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![uid, item.name, item.image, item.price, ts],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(balance - item.price)
}

/// Inventory van een lid: (naam, afbeelding, betaalde prijs), nieuwste eerst.
pub fn inventory_items(pool: &DbPool, uid: &str) -> Vec<(String, String, i64)> {
    let conn = pool.get().expect("db");
    let mut stmt = conn
        .prepare(
            "SELECT name, image, price FROM inventory
             WHERE user_id = ?1 ORDER BY acquired DESC, id DESC",
        )
        .expect("prepare inventory");
    let rows = stmt
        .query_map(params![uid], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))
        })
        .expect("query inventory");
    rows.filter_map(Result::ok).collect()
}
