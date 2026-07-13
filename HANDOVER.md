# Handover — Meadow Market (2026-07-12)

Discord **coin-economy + verzamel-/shop-site** in **Rust** (één self-contained binary:
serenity/poise-bot + Axum-site + gedeelde SQLite). **LIVE** op `https://magicmeadow.org`
(Hetzner-VPS, systemd `market`) en op de **dev-guild** (WaldsteinDevZone).

> Site staat volledig in het **Engels** (merknaam *Meadow Market* blijft). Ik (Claude) praat
> met de user in het Nederlands; de user laat de bouw grotendeels **zelfstandig** afwerken en
> stuurt achteraf bij.

## ⛔ NOOIT DM's
De bot mag **NOOIT** een Direct Message naar een lid sturen — expliciete, absolute user-regel
(2026-07-13, met nadruk). Alle feedback = **publiek kanaalbericht** of een **ephemeral** (enkel
mogelijk als antwoord op een interactie, bv. een knopklik zoals bij de chest). Bij een level-up
(message-event, geen interactie) → publiek bericht in het kanaal + prod #coins, géén DM.

## 📝 Open TODO's
- **Gem-naamkleur**: naam van het lid in het **juiste font** tonen bij de achtergrond-instelling
  via een gem (swatch-preview). Cosmetische verfijning.
- **Admin klik op naam** in /admin/coins → toon de **coin-pagina van díe specifieke user**.
- **(WIP) Birthday-present**: registreer verjaardag → claim een cadeau (staat als "WIP" op /info).
- **Faybelle's oude −270** zit enkel in `coins`, niet in `total_earned` (van vóór de checkbox-fix) —
  evt. gelijktrekken via Set met enkel "all time" aangevinkt.

## 🌐 Discord-guilds & kanalen
- **Dev-guild** (WaldsteinDevZone): `652452615879262220` — nog steeds `cfg.guild_id` (bot-gateway),
  test-omgeving. Dev #coins `1525189157104648343`, dev #fortuna-log `1526159841444237385`.
- **Prod-guild** (Magic Meadow): `1296469405651435592`. Bot = Fortuna `1524865923771793668`.
  - **☀️general** `1296469405651435594` · **🪙coins (meadowcoins)** `1403044480218824794` ·
    **🧺meadowmarket** `1403810528039665745` · **fortuna-log** `1526181603624226938`.
  - Progressieve activering: coin-**verdienen** hangt aan de **coin-kanalenlijst** (DB-tabel
    `coin_channels`, beheerd op **/admin/channels**). **Lege lijst = nergens verdienen.** Voeg een
    prod-kanaal toe → verdienen + shout-outs + weekly + level-ups worden daar vanzelf actief.
  - **Prod-gerichte consts in `bot.rs`**: uurlijkse shout-out + level-up + **daily-melding** → prod
    #coins; weekly leaderboard → prod #general; **fortuna-log → prod** (`1526181603624226938`),
    meadowmarket-log **uit**; coin-emoji = **prod-emoji** `1526188363110023308` (bot zit in beide
    guilds → rendert overal). Natuurlijke **chests spawnen ENKEL in prod #general**
    (`CHEST_SPAWN_CHANNEL_ID`). Coins-beheerpagina + kanalen-picklist lezen van prod (`COINS_GUILD_ID`).
  - **`!chest`/`!chestodds`** = dev-guild-only (test-spawn).

## 📌 Sessie 2026-07-13 (avond) — 🚀 GO-LIVE op Magic Meadow
> **Volgende sessie: start hier.** Alles is **gecommit + gepusht** (`market-gh main`) én live.
> **We zijn LIVE gegaan** met prod-waarden; de community verdient/claimt nu echt.

- **Prod-waarden gezet** (chest pop 10min, cooldown 50min, distinct-chatters 3, hourly shout-out
  op HH:01 ≥100/uur, `COIN_FEEDBACK=false`, chest min-2-joiners). Enige dev-test = `!chest` (spawnt
  op prod-timing) + de ticker-interval.
- **Saldi handmatig gezet** door Faybelle (18 users) — historisch verdiend → via de go-live-fix
  krijgen Add/Set nu `total_earned` mee (backfill gedaan: alle 18 `coins == total_earned`).
- **DB-backup** van de prod-`coins.db` staat in `~/backups/coins.db.prod-20260713-164737` (+ een
  volledige map-tarball `~/backups/market-backup-*.tar.gz`).
- **Streak-preseeds** (7 users, `last_daily`+`daily_streak`, coins ongemoeid) — bv. FayBelle dag 3,
  Yâ-Ôd dag 2. Waldstein-venster verlopen (streak reset bij volgende claim, aanvaard). Server-TZ =
  **Europe/Brussels** gezet (code rekent op absolute epoch, dus TZ is cosmetisch).
- **Daily → prod**: `DAILY_COOLDOWN` 24u→**20u**; aparte **`DAILY_STREAK_WINDOW` 30u** (binnen 30u
  opnieuw klikken = streak behouden). Daily-melding in #coins **tagt** de member + getallen vet.
  **DEBUG-regel** in fortuna-log (admins): `🔧 daily — @user got N · streak S · rolled in [lo–hi]`.
  Geen ephemeral meer bij een geslaagde daily (stille ack). Cooldown-ephemeral = "⏳ Too soon!…".
- **Meadowmarket-embed LIVE** in Magic Meadow **#🧺market** (`1403810528039665745`, hernoemd van
  meadowmarket): titel `# 🧺 Meadow Market 💎`, bulletlijst (bold), knoppen **Check In**
  (`daily_claim`, coin-emoji) · **🧺 Visit Meadow Market** (`site_access` → under-construction) ·
  **ℹ️ Info** (link → `/info`).
- **Publieke `/info`-pagina** (accordion via `<details>`, klik = uit/invouwen) met de earning-uitleg.
- **🔒 Site-gate** (`gate`-middleware in `web.rs`): **niet-admins → redirect naar `/info`**; enkel
  `/info`, `/img/*`, login/oauth, `/healthz` publiek; admins (Waldstein/FayBelle) houden volle
  toegang. Tijdelijk tot de site publiek opengaat.
- **Chest-herwerking**: titel **"🎁 Fortuna's Favor"**; **live M:SS-aftel-timer** (ticker elke 2s,
  embed-edit); chest + coin via **vaste URL** (`/img/chest.png` + emoji-CDN) i.p.v. attachments;
  te-laat-ephemeral = "make sure you click within X minutes".
- **/admin/coins**: **current/all-time-checkboxes** bij Add/Set (beide default aan) + **All-time-
  kolom**; **undo herstelt beide** (`admin_undo.prev_earned` toegevoegd). Coin-emoji op de site =
  prod-emoji-CDN.

### Vroeger deze dag (voormiddag/namiddag) — prod-opzet & economie-uitbreiding

**Discord-serverbeheer (via REST-API, geen code):** in de dev-guild een **Hytale**-, **Archive**-
(open) en **Marketplace**-categorie gemaakt; oude kanalen (geen juli-2026-activiteit) naar Archive
verplaatst, recente naar hun categorie. **LOGS**-kanalen aangemaakt: **fortuna-log** (coin-
verdiensten) + **meadowmarket-log** (saldo-updates). Nieuwe categorieën vereisten *Manage Channels*
op de bot-rol (Fortuna) — user zette dat aan.

**Coin-economie (bot):**
- **Verdienen enkel in kanalen op `coin_channels`** (DB, /admin/channels) i.p.v. één vaste kanaal-ID.
  Guild-gate weg (kanaal-ID is guild-uniek). **Lege lijst = nergens.**
- **Gewogen kans per bericht: 80% → 1 · 19% → 2 · 1% → 3** (`COIN_WEIGHTS`).
- Elke verdienste → **#fortuna-log** (`Naam + **N** 🪙`) + saldo → **#meadowmarket-log** (via `log_earn`,
  ook voor daily + chest). Alle 🪙 vervangen door de **custom `Meadowcoins`-emoji**
  (`<:Meadowcoins:1526149523288883220>`); op de **site** als inline `<img>` (Discord-CDN, klasse `.mc`).
- **Level-up-cadeau**: bij level-wissel **+1% van het saldo** + **publiek** bericht in het kanaal én
  prod #coins (NOOIT DM). ⚠️ Enkel bij **bericht**-verdiensten (daily/chest-level-ups nog niet).
- **Uurlijkse ≥100-shout-out** (`hourly_shoutouts`) → **prod #coins**. **TEST-modus** aan.
- **Weekly leaderboard**: **zaterdag 15:00 Brussel** (EU-DST zelf berekend, geen chrono) → embed in
  **prod #general**; site-tab **"This week"** naast All-time/Now. `earn_log` bewaart **8 dagen**.

**Treasure chest (grondig herwerkt):** artwork ingebakken (`treasure chest.png`, `coin.png`,
`crying.png`, `24hHytale.png` in `artwork/`). Spawn-embed: chest groot + coin-thumbnail, kop-tekst
`### …grand prize! It will **despawn/open** <t:…:R>` (live aftel-timer). **Min. 2 deelnemers**
(`CHEST_MIN_JOINERS`, ook prod) anders **despawn** → *"Fortuna cries…"*-embed met `crying.png`. Bij
genoeg → titel wisselt naar "open"; pop = origineel **verwijderen** + **nieuw** embed onderaan
(*"The Magic Chest opened!"*, winnaar getagd, geen balance). Elke klik werkt de embed live bij
(need-teller). **`!chest`** (spawn) en **`!chestodds`** = **dev-guild-only** commando's.

**Levelsysteem herzien (`web.rs`, `db::level_of`):** **0-based** (beginner = Level 0) en **oneindig**
(formule `50 × 1.6^level`, geen cap). Level-tiers-embed staat in **dev #coins**.

**Admin-tools (site):**
- **/admin/coins** (nav "🪙 Coins"): leest **prod-leden** (`COINS_GUILD_ID`), toont **iedereen** (ook
  0-coin), 4 sorteerknoppen (A–Z, Z–A, Coins ↑/↓), **Add/Set**, **↶ Undo** (altijd zichtbaar, DB-
  backed), **auto-refresh** 20s. `admin_add_coins`/`admin_set_coins` raken enkel `coins` (niet
  total_earned). ⚠️ De **suggestie/seed/Confirm-flow is VERLATEN** (saldo-uitlezen werkte niet;
  seed-knop + toggle verwijderd) — startwaarden worden **manueel** gezet. Handlers/routes bestaan nog
  dormant (`coin_suggest`-tabel ongebruikt).
- **/admin/channels** (nav "📋 Channels"): coin-kanalenlijst, picklist (prod-tekstkanalen), rode ✕.
- **Leave/rejoin-archief**: `GuildMemberRemoval` → `archive_on_leave` (saldo+earned gearchiveerd,
  gereset naar 0). Op /admin/coins: **Restore** / **Discard** per vertrokken lid.
- **Leaderboard**: gefilterd op **≥1 coin** (min 1 om erop te staan).

**24h-pas-icoon** = `24hHytale.png` (ingebakken, `/img/ticket.png`); pas-kaders verruimd (geen afkap).

**Nieuwe DB-tabellen (idempotent gemigreerd):** `earn_log`, `admin_undo`, `coin_archive`,
`coin_channels`, `coin_suggest` (laatste ongebruikt).

> ℹ️ De testwaarden uit deze voormiddag/namiddag zijn bij **go-live (avond) allemaal op de prod-
> waarden gezet** — zie de go-live-sectie bovenaan.

**Nog open / mogelijk vervolg:** daily/chest-level-ups ook cadeau geven (nu enkel bericht-level-ups);
`cfg.guild_id` ooit naar prod als de **bot-gateway** volledig naar Magic Meadow moet (raakt ook de
Hytale-pas-rolgrants + site-rolcheck — geen simpele flip); website publiek openstellen (site-gate weg).

## 📌 Sessie 2026-07-12 (avond) — MARKET-FEATURES + TREASURE CHEST
> **Volgende sessie: start hier.** Alles hieronder is gecommit + gepusht (`tale-gh` +
> `market-gh` subtree) én **gedeployed** op de dev-guild. Reeks kleine iteraties, live getest.

- **Logout naar de topbar** — pill rechtsboven in de groene balk i.p.v. een nav-tab (`39ab399`).
- **Pas-flow herzien — kopen = direct whitelisten** (`c6ba964`): Buy op een Hytale-pas activeert
  meteen (geen inventory-tussenstap, geen aparte Use). De **Hytale-naam wordt één keer** mee-
  getypt in het koopformulier (inline veld zolang er geen naam is), daarna persistent en niet
  meer gevraagd. Boosts-tab toont enkel nog whitelist-status + (corrigeerbare) naam. `/use/boost`
  + `boost_slot` verwijderd. Lokaal e2e geverifieerd (stapelen, guard, perma, blokkering).
- **`bot.py` 1-min reconcile gecommit** (`3b6bc9c`) — draaide al live, git-schuld rechtgezet.
- **🎁 Treasure chest** (market-bot, `src/bot.rs`): bij **≥ CHEST_DISTINCT_USERS verschillende
  chatters** binnen 10 min in het **testkanaal** (`#botstuffs-test-channel`,
  `TEST_CHANNEL_ID`) verschijnt een chest-**embed** met een **Try your luck**-knop. Klikken =
  meedoen (1 inschrijving/lid, ephemeral teller); **3 min** later popt hij, één random klikker
  wint. Anti-spam: per-kanaal `active`-vlag tijdens de 3 min + **30-min cooldown** na een pop.
  In-memory (chest verloren bij bot-herstart; aanvaardbaar).
  - **`CHEST_DISTINCT_USERS = 2` = TESTWAARDE** (weinig testers) — **prod = 3**.
  - **Gewogen prijzen** (`chest_prize`, gewichten in ‰): **70%** 50-100, **20%** 100-300,
    **5%** 300-500, **4%** 500-800, **1%** 800-1000 coins (EV ~148). Dit is de **live** verdeling
    (`CHEST_TIERS`).
  - **`CHEST_TIERS_PROPOSAL`** = fijnkorreliger 10-tier voorstel (EV ~157), **enkel getoond** in
    de `!chest`-embed ter vergelijking — nog niet actief. Omschakelen = de twee arrays wisselen.
- **`!chest`-commando** — embed met **Current (live)** + **Proposal (finer-grained)** verdeling.
  **Enkel op de dev-guild** via poise-`check` `dev_guild_only` (snowflake `DEV_GUILD_ID =
  652452615879262220`); op een prod-guild volledig inert (geen embed, geen actie).
- **Botcommando's wissen hun aanroep-bericht** vóór uitvoering — centraal via een poise
  `pre_command`-hook (`5569d0d`). Staande regel, zie memory [[market-bot-commands-clean]].
  ℹ️ **Vereist "Manage Messages"** in de guild/kanaal — anders `Invalid permissions` en blijft
  het bericht staan (embed verschijnt wél). De bot heeft Manage Roles maar géén Administrator/
  Manage Messages en kan zichzelf dat recht niet geven. **OPGELOST 2026-07-12 avond:** user zette
  Manage Messages AAN voor `MeadowMarketBot` → commando-berichten worden nu correct gewist ✅.
- **`!coins`-commando + Discord-leaderboard VERWIJDERD** (`d94a41f`) — leaderboard leeft enkel
  nog op de site (`/leaderboard`). Dode code opgeruimd (`db::leaderboard`, `LEADERBOARD_SIZE`).
- **Prod-feedback teruggezet** (`9e8272b`): **`DEV_FEEDBACK = false`** (geen per-bericht coin/
  cooldown-reply meer) + **`COOLDOWN = 30s`** (was test 10s). *(Dit vervangt de "nog open"-noot
  van de namiddag-sectie.)* Alle Discord-berichten/feedback in het **Engels**.
- **Coins-tab**: **huidig saldo groot** (`.earned`), all-time klein eronder — minder verwarrend.
- **Live-refresh van het saldo** (`bb8d601`): endpoint **`GET /api/balance`** (enkel sessie,
  géén Discord-call) + een JS-poller (5s) werkt saldo (shop-purse + Coins-tab), all-time en de
  level-balk live bij op ingelogde pagina's. Geen page-reload meer nodig.
- **Nog open / testwaarden:** `CHEST_DISTINCT_USERS = 2` (→ 3 voor prod); overweeg het
  fijnkorrelige chest-voorstel te activeren; `!chest`-`DEV_GUILD_ID` staat op de dev-guild.

## 📌 Sessie 2026-07-12 (namiddag) — GEDEPLOYED + LIVE-FIXES
> **Volgende sessie: start hier.** De whitelist-passen-feature is **gepusht + gedeployed** en
> draait live. Onderweg twee dingen bijgesteld op de prod-host `hytale`:

- **Gepusht:** 4 commits → `tale-gh` (`cdf078b`) + market-subtree → `market-gh` (`b01d468`).
- **Market gedeployed** (`./deploy/deploy.sh`) — `market.service` active, `coins.db` gemigreerd.
- **tale-bot gedeployed** (propere deploy: eerst read-only diff bevestigd dat de live `bot.py`
  == baseline `325bbae`, dus **geen live edits van Faybelle** overschreven; backup
  `bot.py.bak-20260712`). `[market]`-sectie toegevoegd aan `/opt/hytale/bot/config.toml`
  (`enabled=true`, `coins_db="/opt/market/coins.db"`).
- **Reconcile-interval 5 min → 1 min** (`bot.py`, `@tasks.loop(minutes=1)`): een koper wacht nu
  ≤1 min i.p.v. ≤5 min op whitelisting. Load verwaarloosbaar (idle-ronde = paar SQLite-reads;
  console-commando's vuren enkel bij een échte add/remove).
- **⚠️ FIX coins.db-toegang:** map `/opt/market` stond op `750` (`drwxr-x---`) → de bot-user
  `hytale` kon de wereld-leesbare `coins.db` (644) niet bereiken (`market coins.db niet
  gevonden`). Opgelost met **`chmod o+x /opt/market`** (traverse-bit; map krijgt géén leesbit,
  `secrets.json` blijft `600`). Na de fix leest de reconcile schoon. *(De HANDOVER hieronder
  beweerde "644 → geen perms-stap nodig"; dat was fout, de maprechten waren het probleem.)*
- **⚠️ whitelist.json stond `enabled:true`, NIET `false`** (de sectie hieronder zei `false`). De
  nieuwe `enforce_whitelist()` dwingt dus écht af. Gevolg: de purge haalde **Waldstein** van de
  whitelist (hij had geen pass en stond **niet** in `protected_names` — enkel Faybelle stond er).
- **FIX protected_names:** `["Faybelle"]` → **`["Faybelle", "Waldstein"]`** in `config.toml`
  (backup `config.toml.bak-20260712`), bot herstart → Waldstein terug op `whitelist.json`
  (UUID `55f2e0de…`). Beide admins zijn nu beschermd (nooit kickbaar, ook zonder pass).
- **Nog open / let op:** testwaarden `DEV_FEEDBACK=true`/`COOLDOWN=10s` (market `bot.rs`) staan
  nog aan voor de dev-guild — vóór een echte prod-community terug naar `false`/prod-cooldown.
  Tale-side TODO blijft: in-game welkom + resttijd bij join.

## 📌 Sessie 2026-07-12 (voormiddag) — GEBOUWD + GETEST *(zie namiddag hierboven: intussen gedeployed)*
> Beide kanten af, gebouwd en lokaal getest.

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
4. **Tale-integratie**: ✅ LIVE sinds 2026-07-12 (namiddag). Market schrijft grants in
   `hytale_whitelist`; de tale-bot reconcilet elke **1 min** read-only naar `whitelist.json`
   (`whitelist.json` = `enabled:true`, wordt afgedwongen). Zie de bovenste sessie-sectie.
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
