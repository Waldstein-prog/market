# Meadow Market

Discord **coin-economy + rol-toggle site**, geschreven in **Rust** (één binary).
Lab-poort **8700**. Draait op de Hetzner-VPS als systemd-service `market`.

## Wat het doet
- **Coin-bot** (serenity + poise): elk bericht van een lid levert random 1–3 coins op,
  met een cooldown per lid (persistent in SQLite). `!coins` toont een embed-leaderboard.
- **Site** (Axum): kale rol-toggle op `http://<host>:8700` — zet een Discord-rol aan/uit
  voor een vaste gebruiker.
- Bot-gateway en web-server draaien **concurrent in één proces**, met een gedeelde
  SQLite-DB (`coins.db`).

## Structuur
```
Cargo.toml
src/
  main.rs         entry: bot + web concurrent (tokio)
  config.rs       secrets.json / env laden
  db.rs           rusqlite + r2d2 (coins-tabel)
  bot.rs          serenity/poise: message->coins, !coins-embed
  web.rs          axum: /, /api/status, /api/toggle, /healthz
  discord_rest.rs reqwest-wrapper voor rol add/remove (web-kant)
templates/index.html   (ingebakken via include_str!)
static/style.css       (ingebakken via include_str!)
deploy/
  market.service  systemd-unit (User=market, MemoryMax=250M)
  deploy.sh       bouwt lokaal + scp't binary naar de VPS
```

## Config
`secrets.json` in de working directory (gitignored), of env-vars
(`DISCORD_BOT_TOKEN`, `DISCORD_GUILD_ID`, `DISCORD_ROLE_ID`, `DISCORD_USER_ID`,
`DISCORD_ROLE_LABEL`):
```json
{
  "bot_token": "…",
  "guild_id": "…",
  "role_id": "…",
  "user_id": "…",
  "role_label": "Hytaler"
}
```
De bot vereist twee **privileged intents** in de Discord Developer Portal:
MESSAGE CONTENT + SERVER MEMBERS.

## Lokaal draaien
```
cargo run          # of: cargo build --release && ./target/release/market
```
→ bot verbindt + site op http://localhost:8700

## Deploy (Hetzner)
De binary is self-contained (templates/CSS ingebakken). Bouwen gebeurt **lokaal**
(niet op de RAM-krappe VPS); enkel de binary wordt gekopieerd:
```
./deploy/deploy.sh          # build -> scp -> systemctl restart market
```
`secrets.json` staat apart op de server (`/opt/market/secrets.json`, mode 600) en gaat
nooit mee in git of het deploy-script.

Logs: `ssh hytale 'journalctl -u market -f'`
