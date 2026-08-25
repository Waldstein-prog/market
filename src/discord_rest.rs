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
        // Timeout zodat een stallende Discord-call niet eeuwig blokkeert (o.a. de kanaal-backfill
        // doet veel opeenvolgende requests — één hangende request zou anders de hele taak bevriezen).
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            client,
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
    pub async fn list_roles(&self, guild: &str) -> Result<Vec<(String, String, String)>, String> {
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
                let id = r["id"].as_str().unwrap_or("").to_string();
                let name = r["name"].as_str().unwrap_or("").to_string();
                let color = r["color"].as_u64().unwrap_or(0);
                if name.is_empty() || color == 0 {
                    continue;
                }
                out.push((id, name, format!("#{color:06x}")));
            }
        }
        Ok(out)
    }

    /// Het rol-ID van de rol met exact deze naam (hoofdletter-ongevoelig) in de eigen guild
    /// (`self.guild`). Ok(None) = geen rol met die naam.
    pub async fn role_id_by_name(&self, name: &str) -> Result<Option<String>, String> {
        let url = format!("{API}/guilds/{}/roles", self.guild);
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
        if let Some(roles) = arr.as_array() {
            for r in roles {
                if r["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(name)) {
                    return Ok(r["id"].as_str().map(|s| s.to_string()));
                }
            }
        }
        Ok(None)
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

    /// Alle rollen van de eigen guild als (id, naam) — inclusief kleurloze rollen.
    /// (`list_roles` slaat kleurloze rollen over; deze geeft ze allemaal.)
    pub async fn all_roles(&self) -> Result<Vec<(String, String)>, String> {
        let url = format!("{API}/guilds/{}/roles", self.guild);
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
                let id = r["id"].as_str().unwrap_or_default().to_string();
                let name = r["name"].as_str().unwrap_or_default().to_string();
                if !id.is_empty() && !name.is_empty() {
                    out.push((id, name));
                }
            }
        }
        Ok(out)
    }

    /// De rol-ID's die dit lid nu draagt (leeg als het geen lid is).
    pub async fn member_role_ids(&self, user: &str) -> Result<Vec<String>, String> {
        Ok(self
            .get_member(user)
            .await?
            .and_then(|m| {
                m["roles"].as_array().map(|a| {
                    a.iter().filter_map(|r| r.as_str().map(String::from)).collect::<Vec<_>>()
                })
            })
            .unwrap_or_default())
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

    /// Alle guild-leden (max 1000): (user_id, weergavenaam, join-tijd epoch-sec). Bots
    /// overgeslagen. `joined_at` dient als afwezigheids-fallback voor wie nooit postte.
    pub async fn list_members_joined(
        &self,
        guild: &str,
    ) -> Result<Vec<(String, String, f64)>, String> {
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
                let joined = m["joined_at"]
                    .as_str()
                    .and_then(iso8601_to_secs)
                    .unwrap_or(0.0);
                out.push((id, name, joined));
            }
        }
        Ok(out)
    }

    /// Eén pagina kanaalberichten (nieuw→oud), max `limit` (≤100). `before` = message-id om
    /// vanaf terug te bladeren (None = nieuwste). Geeft `(author_id, message_id, author_is_bot)`.
    /// De message-id (snowflake) bevat de aanmaaktijd → geen timestamp-parsing nodig
    /// (zie `snowflake_secs`). 429 wordt intern afgewacht en herprobeerd.
    pub async fn get_messages(
        &self,
        channel_id: &str,
        before: Option<&str>,
        limit: u16,
    ) -> Result<Vec<(String, u64, bool)>, String> {
        let mut url = format!("{API}/channels/{channel_id}/messages?limit={limit}");
        if let Some(b) = before {
            url.push_str(&format!("&before={b}"));
        }
        for _ in 0..6 {
            let resp = self
                .client
                .get(&url)
                .header("Authorization", self.auth())
                .send()
                .await
                .map_err(|e| e.to_string())?;
            let status = resp.status();
            if status.as_u16() == 429 {
                let body: Value = resp.json().await.unwrap_or_default();
                let wait = body["retry_after"].as_f64().unwrap_or(1.0);
                tokio::time::sleep(std::time::Duration::from_secs_f64(wait + 0.1)).await;
                continue;
            }
            if !status.is_success() {
                return Err(explain(status.as_u16(), &resp.text().await.unwrap_or_default()));
            }
            let arr: Value = resp.json().await.map_err(|e| e.to_string())?;
            let mut out = Vec::new();
            if let Some(msgs) = arr.as_array() {
                for m in msgs {
                    let aid = m["author"]["id"].as_str().unwrap_or_default().to_string();
                    let is_bot = m["author"]["bot"].as_bool().unwrap_or(false);
                    let mid = m["id"].as_str().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
                    if !aid.is_empty() && mid != 0 {
                        out.push((aid, mid, is_bot));
                    }
                }
            }
            return Ok(out);
        }
        Err("Rate limited (429) — te vaak achtereen, kanaal opgegeven.".to_string())
    }

    /// Alle ACTIEVE threads in de guild: (thread_id, parent_id). Eén guild-brede call —
    /// Discord geeft enkel de threads terug die de bot kan zien.
    pub async fn active_threads(&self, guild: &str) -> Result<Vec<(String, String)>, String> {
        let url = format!("{API}/guilds/{guild}/threads/active");
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
        let v: Value = resp.json().await.map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        if let Some(threads) = v["threads"].as_array() {
            for t in threads {
                let id = t["id"].as_str().unwrap_or_default().to_string();
                let parent = t["parent_id"].as_str().unwrap_or_default().to_string();
                if !id.is_empty() && !parent.is_empty() {
                    out.push((id, parent));
                }
            }
        }
        Ok(out)
    }

    /// GEARCHIVEERDE threads onder één kanaal: hun thread-ids. `private=false` → publieke,
    /// `true` → private (kan 403 geven zonder Manage Threads/lidmaatschap → de caller vangt
    /// dat op en slaat over). Pagineert via `before` = archive_timestamp van de laatste thread.
    pub async fn archived_threads(&self, channel: &str, private: bool) -> Result<Vec<String>, String> {
        let kind = if private { "private" } else { "public" };
        let mut before: Option<String> = None;
        let mut out = Vec::new();
        loop {
            let mut url = format!("{API}/channels/{channel}/threads/archived/{kind}?limit=100");
            if let Some(b) = &before {
                url.push_str(&format!("&before={b}"));
            }
            let mut got: Option<Value> = None;
            for _ in 0..6 {
                let resp = self
                    .client
                    .get(&url)
                    .header("Authorization", self.auth())
                    .send()
                    .await
                    .map_err(|e| e.to_string())?;
                let status = resp.status();
                if status.as_u16() == 429 {
                    let body: Value = resp.json().await.unwrap_or_default();
                    let wait = body["retry_after"].as_f64().unwrap_or(1.0);
                    tokio::time::sleep(std::time::Duration::from_secs_f64(wait + 0.1)).await;
                    continue;
                }
                if !status.is_success() {
                    return Err(explain(status.as_u16(), &resp.text().await.unwrap_or_default()));
                }
                got = Some(resp.json().await.map_err(|e| e.to_string())?);
                break;
            }
            let v = match got {
                Some(v) => v,
                None => return Err("Rate limited (429) — threads/archived.".to_string()),
            };
            let has_more = v["has_more"].as_bool().unwrap_or(false);
            let mut last_ts: Option<String> = None;
            let mut n = 0usize;
            if let Some(threads) = v["threads"].as_array() {
                for t in threads {
                    if let Some(id) = t["id"].as_str() {
                        out.push(id.to_string());
                        n += 1;
                    }
                    if let Some(ts) = t["thread_metadata"]["archive_timestamp"].as_str() {
                        last_ts = Some(ts.to_string());
                    }
                }
            }
            before = last_ts;
            if !has_more || n == 0 || before.is_none() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        }
        Ok(out)
    }

    /// Als `get_messages`, maar met de bericht-inhoud erbij: (author_id, msg_id, is_bot, content).
    /// De inhoud dient om `!`-commando's over te slaan (die leveren live ook geen coins op).
    pub async fn get_messages_detailed(
        &self,
        channel_id: &str,
        before: Option<&str>,
        limit: u16,
    ) -> Result<Vec<(String, u64, bool, String)>, String> {
        let mut url = format!("{API}/channels/{channel_id}/messages?limit={limit}");
        if let Some(b) = before {
            url.push_str(&format!("&before={b}"));
        }
        for _ in 0..6 {
            let resp = self
                .client
                .get(&url)
                .header("Authorization", self.auth())
                .send()
                .await
                .map_err(|e| e.to_string())?;
            let status = resp.status();
            if status.as_u16() == 429 {
                let body: Value = resp.json().await.unwrap_or_default();
                let wait = body["retry_after"].as_f64().unwrap_or(1.0);
                tokio::time::sleep(std::time::Duration::from_secs_f64(wait + 0.1)).await;
                continue;
            }
            if !status.is_success() {
                return Err(explain(status.as_u16(), &resp.text().await.unwrap_or_default()));
            }
            let arr: Value = resp.json().await.map_err(|e| e.to_string())?;
            let mut out = Vec::new();
            if let Some(msgs) = arr.as_array() {
                for m in msgs {
                    let aid = m["author"]["id"].as_str().unwrap_or_default().to_string();
                    let is_bot = m["author"]["bot"].as_bool().unwrap_or(false);
                    let mid = m["id"].as_str().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
                    let content = m["content"].as_str().unwrap_or_default().to_string();
                    if !aid.is_empty() && mid != 0 {
                        out.push((aid, mid, is_bot, content));
                    }
                }
            }
            return Ok(out);
        }
        Err("Rate limited (429) — te vaak achtereen, kanaal opgegeven.".to_string())
    }

    /// Post een tekstbericht in een kanaal (bv. shop-aankoopmeldingen in #coins).
    /// Los van de gateway-bot: gewone REST-POST met het bot-token.
    pub async fn send_channel_message(&self, channel_id: &str, content: &str) -> Result<(), String> {
        let url = format!("{API}/channels/{channel_id}/messages");
        let resp = self
            .client
            .post(&url)
            .header("Authorization", self.auth())
            .json(&serde_json::json!({ "content": content }))
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

/// Discord-snowflake → epoch-seconden. De bovenste 42 bits zijn ms sinds de Discord-epoch
/// (2015-01-01). Zo lezen we de aanmaaktijd van een bericht zonder de ISO-timestamp te parsen.
pub fn snowflake_secs(id: u64) -> f64 {
    (((id >> 22) + 1_420_070_400_000) as f64) / 1000.0
}

/// ISO-8601 UTC-timestamp (Discord `joined_at`, bv. "2021-06-15T18:30:00.000000+00:00")
/// → epoch-seconden. Discord levert altijd UTC (+00:00), dus geen zone-correctie nodig.
/// Days-from-civil volgens het standaardalgoritme (Howard Hinnant).
fn iso8601_to_secs(s: &str) -> Option<f64> {
    if s.len() < 19 {
        return None;
    }
    let num = |a: usize, z: usize| s.get(a..z)?.parse::<i64>().ok();
    let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (h, mi, se) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = (if y2 >= 0 { y2 } else { y2 - 399 }) / 400;
    let yoe = y2 - era * 400;
    let doy = (153 * (if mo > 2 { mo - 3 } else { mo + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some((days * 86400 + h * 3600 + mi * 60 + se) as f64)
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
