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
            user_id    TEXT PRIMARY KEY,
            username   TEXT NOT NULL,
            coins      INTEGER NOT NULL DEFAULT 0,
            last_award REAL NOT NULL DEFAULT 0
        );",
    )
    .expect("kan tabel niet aanmaken");
    pool
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

/// Tel `amount` coins bij, update username + last_award. Returnt het nieuwe totaal.
pub fn award(pool: &DbPool, user_id: &str, username: &str, amount: i64, ts: f64) -> i64 {
    let conn = pool.get().expect("db");
    conn.execute(
        "INSERT INTO coins (user_id, username, coins, last_award)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(user_id) DO UPDATE SET
             coins      = coins + excluded.coins,
             username   = excluded.username,
             last_award = excluded.last_award",
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
