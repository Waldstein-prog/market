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
    /// "dev" of "prod" — bepaalt o.a. de standaard reward-kost (dev 0 / prod 1500).
    #[serde(default)]
    pub environment: String,
    /// Gedeeld geheim voor `/internal/*` — de dienst-tot-dienst-koppeling met het
    /// Hytale-panel (dat kan market's DB niet zelf schrijven en vraagt het ons).
    /// **Hoort in secrets.json, niet in de systemd-unit** (die staat in git).
    /// Leeg ⇒ de interne routes weigeren alles.
    #[serde(default)]
    pub internal_secret: String,
    // --- Twitch-luik: channel-points-redeem → Hytale-whitelist ---
    // Enkel de geheimen staan hier. Alles wat de streamer zelf wil bijstellen
    // (reward-titel, duur van de pas, de whisper-tekst) woont in de `settings`-tabel
    // en staat op Manage → ⚙ Settings — live aanpasbaar, zonder deploy.
    #[serde(default)]
    pub twitch_enabled: bool,
    #[serde(default)]
    pub twitch_app_id: String,
    #[serde(default)]
    pub twitch_app_secret: String,
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
    env_override(&mut cfg.environment, "MARKET_ENV");
    env_override(&mut cfg.internal_secret, "MARKET_INTERNAL_SECRET");
    env_override(&mut cfg.twitch_app_id, "TWITCH_APP_ID");
    env_override(&mut cfg.twitch_app_secret, "TWITCH_APP_SECRET");
    if let Ok(v) = std::env::var("TWITCH_ENABLED") {
        cfg.twitch_enabled = v != "0" && !v.is_empty() && v.to_lowercase() != "false";
    }

    if cfg.role_label.is_empty() {
        cfg.role_label = "de rol".to_string();
    }
    if cfg.base_url.is_empty() {
        cfg.base_url = "http://localhost:8700".to_string();
    }
    if cfg.environment.is_empty() {
        cfg.environment = "prod".to_string();
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

    /// Klaar om het Twitch-luik te starten (aan + app-credentials aanwezig).
    pub fn twitch_ready(&self) -> bool {
        self.twitch_enabled
            && !self.twitch_app_id.is_empty()
            && !self.twitch_app_secret.is_empty()
    }

}
