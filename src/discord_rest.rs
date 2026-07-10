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
