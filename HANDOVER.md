# Handover — Meadow Market (2026-07-12)

Discord **coin-economy + verzamel-/shop-site** in **Rust** (één self-contained binary:
serenity/poise-bot + Axum-site + gedeelde SQLite). **LIVE** op `https://magicmeadow.org`
(Hetzner-VPS, systemd `market`) en op de **dev-guild** (WaldsteinDevZone).

> Site staat volledig in het **Engels** (merknaam *Meadow Market* blijft). Ik (Claude) praat
> met de user in het Nederlands; de user laat de bouw grotendeels **zelfstandig** afwerken en
> stuurt achteraf bij.

## 📌 Sessie 2026-07-12 (laatste) — ALLES GEBOUWD + GETEST, ENKEL DEPLOY RESTEERT
> **Volgende sessie: start hier.** Beide kanten zijn af, gebouwd en lokaal getest. Niets is
> gedeployed of gecommit. De laatste stap (prod-deploy) werd door de auto-mode-classifier
> geblokkeerd (algemene opdracht, prod-target niet benoemd) — user moet de deploy goedkeuren.

**Market (`cargo build --release` ✓, lokaal e2e getest tegen de live rol-API):**
- **Hytale-passen = echte whitelist.** Use van een pas geeft géén Discord-rol meer maar schrijft
  een grant in nieuwe tabel `hytale_whitelist(user_id PK, hytale_name, expires REAL NULL=perma)`.
  Dagpas **stapelt de itemduur** (`item.duration`, normaal 24u — volgt de admin-testwaarde, bv.
  60s) bovenop de resttijd; permanente pas = `expires NULL` + `perma_access`. Koper zet eerst
  z'n **Hytale-naam** (`coins.hytale_name`, route `POST /hytale/name`, regex `^[A-Za-z0-9_]{1,32}$`);
  zonder geldige naam blokkeert Use server-side. **Shop toont voorlopig enkel de passen**
  (daily-offers + gems verborgen, code bewaard achter `#[allow(dead_code)]`). Boosts-tab =
  whitelist-status + live JS-afteller + naam-invoer.
- **Daily-streaksysteem.** Dag 1 random `[10,100]`; elke opeenvolgende dag ondergrens +1 /
  bovengrens +5 (dag 2 `[11,105]`, dag 200 `[209,1095]`); >48u sinds vorige claim → reset dag 1;
  cap dag 200. Kolom `coins.daily_streak`; `award_daily(...,streak,...)`. Feedback (ephemeral +
  #coins): "🔥 Name checked in for N day(s)! You got N Meadowcoins today! Balance: …".

**tale-bot (`tale/bot/bot.py`, `py_compile` ✓, logica lokaal getest):**
- **`reconcile_market()`** in de bestaande 5-min `pass_maintenance`-lus: leest market's
  `hytale_whitelist` **READ-ONLY** (`market_grants()`, `file:…?mode=ro`, veilige no-op bij
  afwezige tabel), doet FIFO `whitelist add/remove` (spam-arm: add enkel wie nog niet present,
  remove enkel present + niet-beschermd + niet door lokale pas levend). Nieuwe `[market]`-config
  (`enabled`/`coins_db`) in `config.example.toml` (default uit → op prod op `true` zetten).
- **Rol-toegang VERWIJDERD** (user: "geen spelers, test-fase, make it work"): `on_member_update`,
  `sync_whitelist()`/`/sync_whitelist`, en `/link` (+ rol-tak) eruit. Passen (market + eigen
  `hytale_users`/Twitch) zijn nu de enige whitelist-bronnen. `/opt/market/coins.db` is al 644
  (wereld-leesbaar) → geen perms-stap nodig.

**⚠️ Deploy-sequence (nog te doen — user moet goedkeuren):**
```
cd /home/jo/lab/market && ./deploy/deploy.sh          # migreert prod-coins.db (additief)
scp /home/jo/lab/tale/bot/bot.py hytale:/opt/hytale/bot/bot.py
ssh hytale 'grep -q "^\[market\]" /opt/hytale/bot/config.toml || printf "\n[market]\nenabled = true\ncoins_db = \"/opt/market/coins.db\"\n" >> /opt/hytale/bot/config.toml'
ssh hytale 'systemctl restart hytale-bot'
```
Daarna keten verifiëren: pas kopen+Use op de site → `hytale_whitelist`-rij → ≤5 min → `whitelist
add <naam>` in `whitelist.json`. Reconcile forceerbaar i.p.v. 5 min wachten.
**Test-waarden** `DEV_FEEDBACK=true`/`COOLDOWN=10s` in market `bot.rs` blijven bewust staan (dev-
guild test). Vóór ECHTE prod-community terugzetten naar `false`/prod-cooldown. **Nog te committen:
beide repos** (market subtree-push + tale). Los tale-side TODO: in-game welkom+resttijd bij join.

## 📌 Sessie 2026-07-11
- **DEV_FEEDBACK=true + COOLDOWN=10s** (test-waarden) staan in `src/bot.rs` en **draaien LIVE**
  op de dev-guild (gedeployed) — Waldstein test grondig de coins. ⚠️ **Vóór een echte prod-uitrol
  terugzetten** naar `DEV_FEEDBACK=false` + prod-cooldown. (Cooldown = per lid, één award per 10s.)
- **BESLIST — Hytale-passen = echte whitelist i.p.v. Discord-rol.** Volledige spec + designbesluiten
  in **`docs/TODO-hytale-passes.md`**. Kort: rol valt volledig weg (market + tale-bot); koper typt
  z'n **Hytale-naam**; **meerdere dagpassen** in inventory met **Use** (+24u-stapel); shop voorlopig
  **enkel de passen**; market voedt `hytale_users` in de tale-bot-DB, de **tale-bot** whitelistet +
  bewaakt de timer (heeft de 24u-stapel-logica + FIFO al). Nog te bouwen.
- **Nieuw project `lab/ops` (techstuff-console) LIVE**: admin-ops op `magicmeadow.org/techstuff`
  (pwd FluffRules9-) — backups + wereld-manager boven tale+market. Zie `ops/README.md` + memory
  [[ops-techstuff]]. Caddy proxyt nu `/techstuff` → :8091 (apex market ongemoeid).

## Live & deploy
- **Site**: `https://magicmeadow.org` (Caddy → `127.0.0.1:8700`, Let's Encrypt-TLS).
- **Service**: systemd `market` (user `market`, `/opt/market`, `MemoryMax=250M`). Draait de
  bot-gateway + web concurrent. Logs: `ssh hytale 'journalctl -u market -f'`.
- **Deploy**: lokaal `./deploy/deploy.sh` (bouwt release-binary LOKAAL, scp binary + unit,
  `systemctl restart market`). **NOOIT** op de VPS compileren (RAM-krap). Uploads persisteren
  in `/opt/market/uploads`; de DB is `/opt/market/coins.db` (SQLite; migreert idempotent bij
  start via `ensure_column`).
- **secrets.json** op `/opt/market/` (mode 600, niet in git): bot_token, guild_id, role_id,
  client_id, client_secret, base_url. **Env-overrides** in `deploy/market.service`:
  `MARKET_BASE_URL=https://magicmeadow.org` en `DISCORD_ROLE_ID=1525249217897955590`
  (shop-toegangsrol = FlowerBorn).
- **GitHub**: `github.com/Waldstein-prog/market` (privé). Push vanuit de lab-monorepo:
  `git -C /home/jo/lab subtree push --prefix=market market-gh main` (cred-helper=store).
- **Lokaal testen**: `MARKET_WEB_ONLY=1 DISCORD_ROLE_ID=1525249217897955590 cargo run`
  (web-only = geen 2e bot-gateway). Sessie fabriceren + data zetten via python/sqlite3 op
  `coins.db`. **Let op**: een losgekoppelde oude instance kan poort 8700 bezet houden →
  `pkill -f 'release/market'` vóór een test.

## Broncode (`src/`)
- `main.rs` — start bot + web (of web-only via `MARKET_WEB_ONLY`).
- `config.rs` — secrets.json + env-overrides.
- `db.rs` — SQLite (rusqlite/r2d2). Tabellen: `coins`, `sessions`, `shelves`, `items`,
  `inventory` (ontgrendel-ledger, `item_id`), `role_grants` (tijdelijke rollen), `daily_shop`
  (24u shop-rotatie). Seeds (idempotent): `seed_gems` (gem-catalogus), `seed_hytale` (2 tickets),
  `seed_horseshoe` (Lucky Horseshoe). Migratie ruimt oude auto-seed gem-schappen op.
- `discord_rest.rs` — dunne REST-wrapper (rol toekennen/intrekken/checken).
- `bot.rs` — coin-award per bericht (1–3, cooldown 30s), `!coins`-leaderboard, **Daily**-embed-
  knop (interaction), **sweeper** (elke 30s: verlopen tijdelijke rollen intrekken).
- `web.rs` — Axum-site (~1400 regels): alle pagina's, admin, kopen/gebruiken, uploads.

## Site-structuur (ingelogd als FlowerBorn)
Topbar = merknaam; daaronder in de kaart de **naam** + **nav**: `Inventory (home /) · Shop ·
Leaderboard · ⚙ Manage (admin) · Log out`.

- **Inventory (`/`)** — sub-tabs (client-side, `/?tab=coins|gems|boosts`):
  - **Coins**: `coins earned all-time` groot (`coins.total_earned`, stijgt enkel bij verdienen);
    **level 1–10** (exponentieel ×1.6, `level_info`) met fillbar + `n/m`; current balance;
    **active-access** aftel-teller (lopende tijdelijke rollen).
  - **Gems** = **bingokaart**: alle gems (3 primary / 5 secondary / 5 prism), **vergrendeld 🔒
    tot je ze in de Shop koopt** (ontgrendelen). Ontgrendeld → afbeelding + naam + uitleg +
    **Use** → **Use zet je naamkleur** (`coins.name_color`); bovenaan je naam op **swatches**
    (Discord-profielkleur als achtergrond + donker + wit); gebruikte gem = 'Equipped'.
  - **Boosts**: 2 Hytale-tickets (`category='boost'`, **verbruikbaar**). Kopen = ontgrendelen;
    **Use** = activeren: dagpas → rol + 24u-teller, permanent → `perma_access` + permanente rol
    (dagpas daarna niet meer koopbaar).
- **Shop (`/market`)** — 4 **random dagitems** (24u-rotatie, `daily_shop`; pool = gems +
  Lucky Horseshoe) + daaronder vast de 2 tickets. **Grote Purse-box rechts onder de nav**
  (`.shophead`/`.purse-box`) met **slotmachine-afteller** na koop (`?from=` → JS telt af).
  Géén succesbanner meer (fout-banners blijven). **3D Buy/Use-knoppen**. Reeds bezeten gems
  tonen 'Owned'.
- **Leaderboard (`/leaderboard`)** — tabs **All-time** (`total_earned`) / **Now** (`coins`),
  iedereen zichtbaar, medailles 👑🥈🥉, eigen rij gemarkeerd.
- **⚙ Manage (`/admin/market`, enkel Waldstein `391337551543271433` + FayBelle
  `233179495094419456`)** — schappen +/hernoem/verwijder, item-slots (＋), lucky items (＋),
  per item **naam · prijs · omschrijving · categorie · rol-ID · duur (min) · afbeelding
  uploaden** · verwijderen.

## Model & regels
- **Kopen** (`/buy`) = **ontgrendelen** (saldo eraf, in `inventory`). Gems: max 1× (bingo).
  Boosts: verbruikbaar (herkoopbaar). **Geen** rol-effect meer bij kopen — effecten volgen bij
  **Use** (gems: kleur; boosts: rol/toegang).
- **Rollen (dev-guild):** shop-toegang = **FlowerBorn** `1525249217897955590` (env
  `DISCORD_ROLE_ID`). Tickets kennen **Hytaler** `1524867158398730460` toe. **Testwaarden**:
  Day Pass = **1 coin / 1 min** (in prod-DB gezet). Sweeper draait 30s.
- **Discord-kleur**: `accent_color` uit `users/@me` (identify-scope) → `coins.discord_color`
  bij login. Verschijnt pas na **opnieuw inloggen**; enkel als de user een accentkleur heeft.

## Openstaand / mogelijke bijsturing
1. **Permanente waarden** zetten (Manage): Day Pass terug naar 24u (1440 min) + echte prijs;
   whitelist-rol op de **Permanent Pass** invullen (rol-ID staat nu leeg → Use zet enkel
   `perma_access`, kent nog geen rol toe).
2. **Prijzen/economie** balanceren (gems/tickets/horseshoe); Lucky Horseshoe heeft nog **geen
   effect** (enkel koopbaar).
3. **Prod-guild**: alles draait nog op de **dev-guild**. Voor de echte community: guild/rollen
   in secrets/env aanpassen + bot inviten + hiërarchie.
4. **Tale-integratie**: de Hytaler/whitelist-rol → echte Hytale-game-whitelist synct nog niet
   (aparte stap op de tale-server).
5. **Public-profiel**: `coins.is_public` bestaat nog maar wordt niet gebruikt (leaderboard toont
   iedereen); ooit een profielpagina met public-filter.
6. Losse asset `static/MeadowShard.png` (debug) staat nog in de repo, ongebruikt.
7. **TODO (voor later, apart gezet): weekly leaderboard.** Derde leaderboard-tab **"This week"**
   = coins verdiend in de lopende week (per-week teller, reset wekelijks — vergt tracking van
   verdiensten per week, bv. een `weekly_earned` + weekstart, of award-events met timestamp).
   Dit wekelijkse klassement wordt **elke zaterdag 16:00 Brusselse tijd** als een **mooie embed
   in het #general-kanaal** gepost (geplande taak in de bot, tz Europe/Brussels, via Discord REST
   webhook/`POST /channels/{id}/messages` zoals de bestaande embeds).

## Zo pik je het op
1. `cd lab/market`, `MARKET_WEB_ONLY=1 DISCORD_ROLE_ID=1525249217897955590 cargo run`, open
   `http://localhost:8700`, log in met Discord (redirect-URI localhost staat geregistreerd).
2. Wijzig → `cargo build --release` → `./deploy/deploy.sh` → commit → `git subtree push`.
3. Economy-ontwerp/achtergrond: `docs/economy-design.md`. Volledige geschiedenis in de git-log
   en in de projectmemory (`market-project`).
