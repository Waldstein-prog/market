//! Dunne REST-wrapper (reqwest) voor rol-toekenning vanaf de web-site.
//! Los van de serenity-bot: enkel een Bot Token, geen gateway.
use serde_json::Value;

const API: &str = "https://discord.com/api/v10";

#[derive(Clone)]
pub struct Discord {
    client: reqwest::Client,
    token: String,
    guild: String,
}

impl Discord {
    pub fn new(token: String, guild: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            token,
            guild,
        }
    }

    fn auth(&self) -> String {
        format!("Bot {}", self.token)
    }

    /// Guild-lid ophalen. Ok(None) = geen lid (404).
    pub async fn get_member(&self, user: &str) -> Result<Option<Value>, String> {
        let url = format!("{API}/guilds/{}/members/{}", self.guild, user);
        let resp = self
            .client
            .get(&url)
            .header("Authorization", self.auth())
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = resp.status();
        if status.as_u16() == 404 {
            return Ok(None);
        }
        if !status.is_success() {
            return Err(explain(status.as_u16(), &resp.text().await.unwrap_or_default()));
        }
        resp.json::<Value>().await.map(Some).map_err(|e| e.to_string())
    }

    /// Tekstkanalen van een guild: (channel_id, name). Enkel GUILD_TEXT (type 0).
    pub async fn list_channels(&self, guild: &str) -> Result<Vec<(String, String)>, String> {
        let url = format!("{API}/guilds/{guild}/channels");
        let resp = self
            .client
            .get(&url)
            .header("Authorization", self.auth())
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = resp.status();
        if !status.is_success() {
            return Err(explain(status.as_u16(), &resp.text().await.unwrap_or_default()));
        }
        let arr: Value = resp.json().await.map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        if let Some(chans) = arr.as_array() {
            for c in chans {
                if c["type"].as_i64() != Some(0) {
                    continue; // enkel gewone tekstkanalen
                }
                let id = c["id"].as_str().unwrap_or_default().to_string();
                let name = c["name"].as_str().unwrap_or("?").to_string();
                if !id.is_empty() {
                    out.push((id, name));
                }
            }
        }
        Ok(out)
    }

    /// Rollen van een guild: (naam, kleur-hex "#rrggbb"). Rollen zonder kleur (color 0)
    /// worden overgeslagen.
    pub async fn list_roles(&self, guild: &str) -> Result<Vec<(String, String)>, String> {
        let url = format!("{API}/guilds/{guild}/roles");
        let resp = self
            .client
            .get(&url)
            .header("Authorization", self.auth())
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = resp.status();
        if !status.is_success() {
            return Err(explain(status.as_u16(), &resp.text().await.unwrap_or_default()));
        }
        let arr: Value = resp.json().await.map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        if let Some(roles) = arr.as_array() {
            for r in roles {
                let name = r["name"].as_str().unwrap_or("").to_string();
                let color = r["color"].as_u64().unwrap_or(0);
                if name.is_empty() || color == 0 {
                    continue;
                }
                out.push((name, format!("#{color:06x}")));
            }
        }
        Ok(out)
    }

    /// Alle guild-leden (max 1000): (user_id, weergavenaam). Bots overgeslagen.
    /// Vereist de GUILD_MEMBERS-intent (staat aan voor de gateway-bot).
    pub async fn list_members(&self, guild: &str) -> Result<Vec<(String, String)>, String> {
        let url = format!("{API}/guilds/{guild}/members?limit=1000");
        let resp = self
            .client
            .get(&url)
            .header("Authorization", self.auth())
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = resp.status();
        if !status.is_success() {
            return Err(explain(status.as_u16(), &resp.text().await.unwrap_or_default()));
        }
        let arr: Value = resp.json().await.map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        if let Some(members) = arr.as_array() {
            for m in members {
                let u = &m["user"];
                if u["bot"].as_bool().unwrap_or(false) {
                    continue;
                }
                let id = u["id"].as_str().unwrap_or_default().to_string();
                if id.is_empty() {
                    continue;
                }
                let name = m["nick"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .or_else(|| u["global_name"].as_str().filter(|s| !s.is_empty()))
                    .or_else(|| u["username"].as_str())
                    .unwrap_or("?")
                    .to_string();
                out.push((id, name));
            }
        }
        Ok(out)
    }

    /// None = geen lid; Some(bool) = heeft de rol wel/niet.
    pub async fn has_role(&self, user: &str, role: &str) -> Result<Option<bool>, String> {
        match self.get_member(user).await? {
            None => Ok(None),
            Some(m) => {
                let has = m["roles"]
                    .as_array()
                    .map(|a| a.iter().any(|r| r.as_str() == Some(role)))
                    .unwrap_or(false);
                Ok(Some(has))
            }
        }
    }

    /// Rol toevoegen (enable=true) of verwijderen (false).
    pub async fn set_role(&self, user: &str, role: &str, enable: bool) -> Result<(), String> {
        let url = format!("{API}/guilds/{}/members/{}/roles/{}", self.guild, user, role);
        let req = if enable {
            self.client.put(&url)
        } else {
            self.client.delete(&url)
        };
        let resp = req
            .header("Authorization", self.auth())
            .header("Content-Length", "0")
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = resp.status();
        if !status.is_success() {
            return Err(explain(status.as_u16(), &resp.text().await.unwrap_or_default()));
        }
        Ok(())
    }
}

fn explain(code: u16, body: &str) -> String {
    match code {
        401 => "Bot token invalid (401).".to_string(),
        403 => "No permission (403): the bot lacks 'Manage Roles' or its role is not above the target role."
            .to_string(),
        404 => "Not found (404): guild, user or role does not exist.".to_string(),
        429 => "Rate limited (429): please wait.".to_string(),
        c => format!("Discord error ({c}): {body}"),
    }
}
