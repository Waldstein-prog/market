# Handover — market (Meadow Market) — 2026-07-10

> Laatste commit: `e44ffe3`. LIVE op Hetzner, coins-stand net gereset naar nul.

## Wat dit is
Project **`market`** (lab-poort **8700**): Discord **coin-economy + rol-toggle site**,
in **Rust** (één self-contained binary: serenity/poise-bot + Axum-site + gedeelde SQLite).
Draait **LIVE op de Hetzner-VPS** als systemd-service `market`.

GitHub: `github.com/Waldstein-prog/market` (privé). Push vanuit de lab-monorepo via
`git subtree push --prefix=market <auth-url> main`.

## Historiek van vandaag
- Begon als **Python** (Flask + discord.py) op **PythonAnywhere**. Fase I (rol-toggle PoC)
  draaide live op PA.
- Toen naar **Hetzner** verhuisd → PA-account wordt verwijderd, dus **weg van de
  gratis-PA-beperking** (geen always-on proces mogelijk voor een bot).
- Daarna bewust **herschreven naar Rust** (compactheid/veiligheid, matcht cyd/devboard,
  ~3 MB RAM i.p.v. ~140 MB). De Python-code is verwijderd (zit in git-historiek).

## Deploystatus (LIVE op Hetzner)
- Service `market.service` draait als user **market**, `WorkingDirectory=/opt/market`,
  `MemoryMax=250M` (kan Hytale nooit de OOM in duwen). Verbruik in rust: **~3 MB**.
- **Site**: `http://167.235.142.113:8700` (kaal IP:poort, geen TLS — "URL later").
  ufw-regel voor 8700/tcp toegevoegd. Onafhankelijk van de Hytale-services.
- **Bot**: verbonden met de gateway, logt de guild-roster (4 leden dev-guild).
- secrets.json staat op `/opt/market/secrets.json` (mode 600, market-eigenaar), niet in git.
- **Updaten**: lokaal `./deploy/deploy.sh` (build → scp binary → systemctl restart market).
  Bouwen gebeurt LOKAAL, nooit op de RAM-krappe VPS.

## Coin-economy (Fase II PoC — GEBOUWD)
- Elk bericht van een lid → random **1–3 coins**, cooldown **per lid** (nu **10s** voor
  de test; prod-waarde 30s — constante `COOLDOWN` in `src/bot.rs`).
- Persistent in SQLite (`/opt/market/coins.db`), tabel `coins(user_id, username, coins,
  last_award)`. Cooldown overleeft herstart (last_award in DB).
- `!coins` → embed-leaderboard, aflopend op coins.
- **Commando's zijn immuun** (`e44ffe3`): berichten die met de prefix `!` beginnen slaan
  de coin-logica volledig over — geen coins, cooldown onaangeroerd (const `PREFIX`).
- **DEV_FEEDBACK=true**: bot antwoordt op elk bericht met de coins/cooldown. Later op
  `false` zetten (constante in `src/bot.rs`) → dan stil.
- **Coins-stand gereset naar nul** op 2026-07-10 (`coins.db` gewist terwijl de service
  stil lag → app maakte verse lege tabel). Resetten = `systemctl stop market` →
  `rm /opt/market/coins.db` → `systemctl start market`.
- **Nog live te testen door user in Discord**: berichten sturen (coins + cooldown-reply)
  en `!coins` (leaderboard, mag zelf geen coins opleveren). Bot-logica + DB lokaal
  geverifieerd; live coin-award vergt echte berichten.

## Config / dev vs prod
- Huidige guild = **DEV** (`WaldsteinDevZone`, 652452615879262220), doelrol **Hytaler**
  (1524867158398730460), vaste site-user Waldstein (391337551543271433).
- Naar **PROD**: enkel `secrets.json` op de server aanpassen (andere guild/rol) + bot in
  die guild inviten + rol-hiërarchie zetten. Geen codewijziging.
- Bot vereist privileged intents MESSAGE CONTENT + SERVER MEMBERS (staan aan).

## Openstaand / later
- DEV_FEEDBACK uit + cooldown → 30s wanneer de test klaar is.
- Site heeft **geen auth**: iedereen die `IP:8700` kent kan de rol togglen. Voor prod:
  domein + TLS (Caddy) en/of auth. Nu bewust kaal IP:poort (PoC).
- Fase II businesslogic (de bredere economy-specs) wacht nog op toelichting van user.
- `coins.db` valt buiten de bestaande tale-backup (die pullt enkel /opt/hytale).
