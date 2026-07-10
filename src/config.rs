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
    // --- OAuth2 (login op de site) ---
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub client_secret: String,
    /// Basis-URL van de site (voor de OAuth redirect-URI). Lokaal:
    /// http://localhost:8700 ; in prod het HTTPS-domein.
    #[serde(default)]
    pub base_url: String,
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
    env_override(&mut cfg.client_id, "DISCORD_CLIENT_ID");
    env_override(&mut cfg.client_secret, "DISCORD_CLIENT_SECRET");
    env_override(&mut cfg.base_url, "MARKET_BASE_URL");

    if cfg.role_label.is_empty() {
        cfg.role_label = "de rol".to_string();
    }
    if cfg.base_url.is_empty() {
        cfg.base_url = "http://localhost:8700".to_string();
    }
    cfg
}

impl Config {
    /// De redirect-URI die zowel in de Discord-portal als in de flow gebruikt wordt.
    pub fn oauth_redirect(&self) -> String {
        format!("{}/auth/callback", self.base_url.trim_end_matches('/'))
    }

    pub fn oauth_ready(&self) -> bool {
        !self.client_id.is_empty() && !self.client_secret.is_empty()
    }
}
