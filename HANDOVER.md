# Handover — market (Meadow Market) — 2026-07-10 (OAuth-sessie, vervolg)

> LIVE op Hetzner. **2026-07-10: browser-login end-to-end GETEST door user = geslaagd**
> (login + rol-check via bot + 🪙 42 op de account-pagina). Bot-prodwaarden én de
> OAuth-flow + topbar zijn nu **GECOMMIT + gepusht** naar `market-gh` (zie §Git-staat).
> Prod-deploy van OAuth blijft geblokkeerd op domein + TLS.

## Wat dit is
Project **`market`** (lab-poort **8700**): Discord **coin-economy + rol-toggle site**,
in **Rust** (één self-contained binary: serenity/poise-bot + Axum-site + gedeelde SQLite).
Draait **LIVE op de Hetzner-VPS** als systemd-service `market`.

GitHub: `github.com/Waldstein-prog/market` (privé). Push vanuit de lab-monorepo via
`git subtree push --prefix=market <auth-url> main`.

## Deploystatus (LIVE op Hetzner)
- Service `market.service`, user **market**, `/opt/market`, `MemoryMax=250M`, ~3–7 MB RSS.
- **Site**: `http://167.235.142.113:8700` (kaal IP:poort, geen TLS — "URL later").
- **Bot**: verbonden, roster 4 leden (FayBelle, Xana, Waldstein, Raevenskye).
- secrets.json op `/opt/market/secrets.json` (mode 600), niet in git.
- **Updaten**: lokaal `./deploy/deploy.sh` (build → scp binary → restart). Bouwen LOKAAL.
- **Vandaag gedeployed** (`e9…`/via deploy.sh): `src/bot.rs` prod-waarden — `COOLDOWN=30.0`,
  `DEV_FEEDBACK=false`. Bot antwoordt dus **niet meer per bericht**; coins gaan stil.
  Coins-test door user = **geslaagd** ("werken prima").

## Git-staat (GECOMMIT + GEPUSHT 2026-07-10)
Twee commits op `master`, daarna `git subtree push --prefix=market … main` naar `market-gh`:
1. `chore(market): prod-waarden — cooldown 30s + feedback uit (live)` — enkel `src/bot.rs`.
2. `feat(market): Discord-OAuth2 login + eigen coins-pagina + topbar` — `config/db/main/web`
   + `docs/economy-design.md` + deze `HANDOVER.md`.

## Wat er vandaag gebouwd is: OAuth2-login op de site
**Doel** (beslist met user): één vaste embed-knop in een statisch Discord-kanaal →
website. Wie **Flowerborn** is ziet z'n eigen coins; wie dat niet is ziet de regels.
**Login = Discord OAuth2**, elk ziet enkel z'n eigen data (sessie-cookie), niets
zichtbaar zonder login. Volledig ontwerp: **`docs/economy-design.md`** (nieuw, in `docs/`).

**Geïmplementeerd (Rust, compileert schoon — `cargo check` OK):**
- `config.rs`: velden `client_id`, `client_secret`, `base_url` (+ env-overrides
  `DISCORD_CLIENT_ID/SECRET`, `MARKET_BASE_URL`), helpers `oauth_redirect()`,
  `oauth_ready()`. `base_url` default `http://localhost:8700`.
- `db.rs`: tabel **`sessions`** (token/user_id/username/created) + `get_coins`,
  `create_session`, `get_session`, `delete_session`.
- `web.rs` (herschreven): routes
  - `GET /` — sessie-cookie? → Flowerborn: account-pagina met saldo; geen rol:
    regels-pagina; niet ingelogd: login-knop.
  - `GET /login` — CSRF-`state`-cookie + 303 naar Discord authorize (`scope=identify`).
  - `GET /auth/callback` — state-check → code→token → `users/@me` → sessie-cookie
    (`HttpOnly; SameSite=Lax`, Max-Age ~90 d) → redirect `/`.
  - `GET /logout` — sessie wissen.
  - **`GET /admin`** — de oude Fase-I rol-toggle (verplaatst, ongewijzigd).
  - `/api/status`, `/api/toggle`, `/healthz` — ongewijzigd.
  - Pagina's inline in `web.rs` (shell + login/account/rules), meadow-groen thema.
  - **Topbar toegevoegd** (deze sessie): volle-breedte balk in de projectkleur
    `MEADOW #6b9b52` bovenaan élke pagina met "🌼 Meadow Market" (zit in `shell()`,
    body → flex-column + `.content` centreert de card eronder).
- `main.rs`: web krijgt nu de DB-pool; **`MARKET_WEB_ONLY=1`** draait enkel de
  web-server (geen bot-gateway) — nodig voor lokaal testen zodat de **live bot niet
  dubbel op de gateway** komt (→ dubbele coins).

**Server-side geverifieerd lokaal** (web-only, `curl`): `/` toont login-knop; `/login`
geeft 303 naar de juiste authorize-URL met correcte client_id + ge-encode redirect_uri
+ CSRF-cookie; `/healthz` = ok. **Browser-flow (echte Discord-login) nog te doen.**

## Config / secrets (lokaal, gitignored)
`secrets.json` lokaal aangevuld met:
- `client_id`: `1524865923771793668` (= de bot-app / publiek).
- `client_secret`: **gezet** (door user aangeleverd — OAuth2-secret van de app).
- `base_url`: `http://localhost:8700`.

**Discord Developer Portal** (door user gedaan): redirect-URI
`http://localhost:8700/auth/callback` toegevoegd onder OAuth2 → Redirects.

**Let op**: de rol-check gebruikt nu `role_id` = **Hytaler** (`1524867158398730460`) als
"Flowerborn". Open vraag 1 (Flowerborn = nieuwe naam of aparte rol?) nog niet beslist;
wordt straks enkel die ID aanpassen.

## Zo pik je het weer op (volgende sessie)
0. **Web-server draait mogelijk al** losgekoppeld op `localhost:8700` (gestart met
   `setsid … cargo run`, log in `/tmp/market-web.log`). Check: `curl -s localhost:8700/healthz`.
   Zo niet, herstart: `cd lab/market && MARKET_WEB_ONLY=1 cargo run` (web-only, geen
   bot-gateway → live bot niet dubbel). **Let op**: in een losgekoppelde/chatter-shell
   staat `cargo` niet op PATH → gebruik `/home/jo/.cargo/bin/cargo`.
1. **De test die de user LATER doet**: open `http://localhost:8700` → groene topbar +
   login-knop → inloggen met Discord. Met Hytaler-rol → account met 🪙 42 (lokaal
   testsaldo voor Waldstein 391337551543271433); zonder → regels. Isolatie: incognito +
   ander account (mag de 42 niet zien).
2. Werkt het → **committen**: eerst `src/bot.rs` apart (prod-waarden, live), dan de
   OAuth-flow (`config/db/main/web` + **topbar** + `docs/`). Dan `git subtree push`
   naar de gh-repo. (User-afspraak: pas committen ná geslaagde test.)
3. **Prod-deploy van OAuth** is geblokkeerd op **domein + TLS** (Caddy): OAuth2 vereist
   een HTTPS redirect-URI; `http://<IP>:8700` kan geen geldig cert. Zie
   `docs/economy-design.md §11`. Pas kopen "als het de moeite waard is" (user).

## Losstaande open vraag: live coins-reset
User vroeg een reset van de coins "voor de test". Live db (`/opt/market/coins.db` op
`ssh hytale`) bevat momenteel **1 rij: Waldstein = 6 coins** (laatst 2026-07-09 22:22 UTC;
sindsdien niets bijgekomen). Reset is dus **NIET** doorgevoerd — user gaf nog geen
expliciete "ja". Wil hij het alsnog: `ssh hytale` → `systemctl stop market` →
`rm /opt/market/coins.db` → `systemctl start market` (app maakt verse lege tabel).
NB: dit staat **los** van het lokale OAuth-testsaldo (Waldstein=42) — twee aparte db's.
NB2: sqlite3 staat niet op de VPS; lees de db door hem lokaal te scp'en.

## Openstaand / roadmap (uit docs/economy-design.md)
- **Beslist**: login=OAuth2, embed=launcher, data-isolatie via sessie-cookie.
- **Nog te beslissen**: Flowerborn vs Hytaler (rol), treasure-chest-mechaniek (A vs B),
  consumables ja/nee, daily-streak-regels, bouwvolgorde.
- **Economy Fase II** verder uitwerken: daily-knop, market (4-slot dagrotatie), inventory
  met schappen, boosters, chest-events, leaderboard/kroontje. Alles in
  `docs/economy-design.md`.
