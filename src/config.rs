//! Config: laadt Discord-instellingen uit secrets.json, met env-var override.
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub bot_token: String,
    #[serde(default)]
    pub guild_id: String,
    #[serde(default)]
    pub role_id: String,
    #[serde(default)]
    pub user_id: String,
    #[serde(default)]
    pub role_label: String,
}

fn env_override(current: &mut String, key: &str) {
    if let Ok(v) = std::env::var(key) {
        if !v.is_empty() {
            *current = v;
        }
    }
}

/// secrets.json in de working directory; env-vars winnen.
pub fn load() -> Config {
    let mut cfg: Config = std::fs::read_to_string("secrets.json")
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    env_override(&mut cfg.bot_token, "DISCORD_BOT_TOKEN");
    env_override(&mut cfg.guild_id, "DISCORD_GUILD_ID");
    env_override(&mut cfg.role_id, "DISCORD_ROLE_ID");
    env_override(&mut cfg.user_id, "DISCORD_USER_ID");
    env_override(&mut cfg.role_label, "DISCORD_ROLE_LABEL");

    if cfg.role_label.is_empty() {
        cfg.role_label = "de rol".to_string();
    }
    cfg
}
