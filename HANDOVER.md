# Handover — Meadow Market (2026-07-15)

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

## ⏭️ Sessie (2026-07-17c) — +0-award wordt gewoon gelogd (stilte was ruis)

**LIVE op prod + gecommit + gepusht** (`0f2cda2`, deploy 11:51:28, subtree `abe3bc9..4b9ad56`).
Eén gerichte fix in `src/bot.rs`, geen nieuwe structuur.

**Wat & waarom.** De **+0**-uitkomst uit `coin_weights` (nieuw sinds 2026-07-17b) werd stil
onderdrukt: `if amount > 0 { log_earn(...) }`. Bedoeld als "geen gebeurtenis = stilte", maar in de
praktijk **las die stilte als ruis/bug** i.p.v. als pech — user-beslissing: een +0 hoort er gewoon
bij te staan. Gate weg → de log toont `Naam + **0** 🪙` in **#fortuna-log** + de balansregel in het
meadowmarket-logkanaal, langs exact dezelfde weg als elk ander bedrag.

**Meegenomen**: dezelfde `&& amount > 0` is ook van de **`COIN_FEEDBACK`**-reply gehaald, zodat die
consistent is als hij ooit aangaat. `COIN_FEEDBACK` staat op `false` → **vandaag verandert dat niets**.

**Ongewijzigd**: de cooldown loopt nog steeds door bij een nul-award (anders blijf je rollen tot er
iets valt). De verdeling zelf is niet aangeraakt — bijsturen kan live via ⚙ Settings.

## ⏭️ Sessie (2026-07-17b) — ⚙ Settings-tab: economie live tunen zonder deploy

**LIVE op prod + gecommit.** De economie-parameters die als `const` in `bot.rs` stonden, zijn nu
admin-instelbaar via **Manage → ⚙ Settings** (tussen Channels en Log). Bot én site lezen ze **LIVE**
uit de DB (zoals `coin_channels`), dus aan een getal draaien werkt **meteen** — geen deploy, geen
herstart. Scope was expliciet **economie-tuning**; feature-toggles, Discord-ID's en de level-curve
zijn er bewust buiten gehouden (user-keuze).

**(A) Nieuwe coin-verdeling per bericht (user-beslissing).** Was 80% → 1 · 19% → 2 · 1% → 3.
Nu **gelijkmatig +0/+1/+2/+3** (gewicht 1 elk), **+4** half zo waarschijnlijk (0,5) en **+5** een
tiende (0,1). Som 4,6 → kansen 21,7% ×4 · 10,9% · 2,2% (1M-trekkingen gesimuleerd, klopt).
⚠️ **ECONOMIE-IMPACT**: gemiddeld **1,85 coins/bericht** i.p.v. 1,21 = **+53% instroom**. De
shopprijzen (gems 1000–11000) zijn nog op de oude instroom geijkt — mogelijk bijsturen.
**+0 is nieuw**: de cooldown loopt wél (anders blijf je rollen). ~~Er gaat geen "+0" naar
#fortuna-log — voor de speler is het stilte.~~ → **Herzien op 2026-07-17c** (user): die stilte las
als ruis/bug. Een +0 gaat nu **gewoon mee in de log**, net als elk ander bedrag.

**(B) Architectuur.** Drie tabellen (stonden al ongecommit in `db.rs` van de vorige sessie):
- `settings (key, value)` — de 14 losse parameters, waarde als TEXT. **De unit zit in de KEY**
  (`_sec`/`_min`/`_hours`/`_coins`/`_days`) zodat een eenheidsfout zichtbaar is op de call-site.
- `coin_weights (amount, weight)` — verdeling per bericht; `weight` is **REAL en RELATIEF**.
- `chest_tiers (id, weight, lo, hi, position)` — chest-prijsverdeling, idem relatief.
- **`src/settings.rs`** (nieuw) = `SPECS`-lijst met key/label/groep/type/default/min/max/help +
  `f64_of`/`i64_of`/`usize_of`/`bool_of`/`set`. **De defaults zijn exact de oude const-waarden**,
  dus een lege `settings`-tabel gedraagt zich als de bot van vóór de refactor (prod heeft 0 rijen).
  Een parameter toevoegen = één `Spec` bijzetten; de GUI tekent hem vanzelf.
- **Seed** (`db::seed_weights`): vult de twee weegtabellen enkel als ze **leeg** zijn — een rij
  wegdoen blijft dus weg. Een tabel volledig leegmaken = "geef me de standaardverdeling terug".

**(C) GUI** (`/admin/settings`): de losse velden per groep (Coins per bericht · Daily · Chest) in
één form, plus **rij-editors** voor beide weegsystemen (toevoegen/wijzigen/✕) met een **berekend
kans-percentage + balkje** — de admin typt relatieve gewichten, het percentage is afgeleid en klopt
dus altijd. **Komma werkt als decimaalteken** (`0,5`). Geweigerd/gecorrigeerd: gewicht ≤ 0, een
omgekeerd tier-bereik (lo/hi wisselen), waarden buiten de spec-grenzen (geklemd).
⚠️ **Checkbox-val**: een uitgevinkt vakje stuurt in HTML géén veld → een partiële POST zou stil elk
vinkje uitzetten. Opgelost met een verborgen **`on_form`**-veld per vinkje; de save-route leest het
formulier als **paren-lijst** (`Form<Vec<(String,String)>>`), niet als map, want `on_form` komt
meermaals voor. Getest: partiële POST laat `chest_enabled` met rust.

**(D) `deploy/deploy.sh` — downtime van scp-duur naar <1s.** Volgorde was: stop → scp → start, dus
de bot lag de hele overdracht plat. Nu: build + **scp + daemon-reload terwijl de oude bot draait**,
daarna **stop → swap → start in één ssh-sessie** (geen round-trips van ~60ms in het dode venster).
`install` op een draaiende binary kan niet (ETXTBSY) → vandaar de /tmp-stage en de swap ná de stop.
Gemeten bij deze deploy: **Stopping en Started binnen dezelfde seconde** (11:04:38).

**Backup vóór de deploy**: `/opt/backups/market/coins-pre-settings-20260717.db` (sqlite online-backup).
**Chest-tiers**: bewust **ongewijzigd** overgenomen als seed (user: "blijven zoals ze zijn voorlopig").

## ⏭️ Sessie (2026-07-17) — gem-teksten/prijzen herzet + Admin shop preview-tab

> **Werkregel bevestigd deze sessie** (memory [[market-session-only-market]]): in een **market**-sessie
> werk je **UITSLUITEND** in `lab/market`. Iemand anders werkt tegelijk in **tale** — daarom botste het.
> Bij "resume" ging ik eerst fout de tale-handover "volgende taak" (memories-crafting) in en bouwde daar
> een grote ingreep → op vraag **volledig teruggedraaid** (`git restore` + untracked weg, niets gedeployed,
> server ongemoeid). Negeer tale-"volgende taak"-noten vanuit een market-sessie.

**(A) Gem-omschrijvingen + prijzen herzet — RECHTSTREEKS IN DE PROD-DB, ⚠️ NIET IN GIT.**
De 12 gems (categorie `inventory`) hadden onnozele teksten ("Get the Amber role" e.d.). Vervangen door
de **letterlijk door de user aangeleverde** omschrijvingen + nieuwe prijzen, in één transactie op
`/opt/market/coins.db` (via `sudo -u market python3`, want **geen `sqlite3` op de VPS**; de shop leest items
live → geen restart nodig). **Backup vooraf:** `/opt/backups/market/coins-pre-gemdesc-20260716-232706.db`
(sqlite online-backup). Eindprijzen: Heliodor 1000, Sapphire 1500, Ruby 2000, Aquamarine 2500, Cinnabar
3500, Iolite 4000, Lapis Lazuli 5000, Topaz 5000, Realgar 6000, Citrine 7500, Amber 10000, Crocoite 11000.
⚠️ Dit is **DATA, geen code** — staat dus nergens in git; de lokale `./coins.db` verschilt van prod. De
"very rare gemstone (11000)"-regel zonder naam is (na bevestiging user) aan **Crocoite** gekoppeld — de
enige overblijvende gem. Werkwijze: eerst analyse-tabel tonen (gem/tekst/prijs), pas schrijven na akkoord.

**(B) "Admin shop" → "Admin shop items" + nieuwe tab "👁 Admin shop preview".** `src/web.rs`:
- `admin_subtabs`: label hernoemd; nieuwe subtab `("/admin/shop/preview", "shop_preview", "👁 Admin shop preview")`.
- Nieuwe route `GET /admin/shop/preview` → **`admin_shop_preview`**: rendert het **beoogde publieke ontwerp**
  (het `else`-tak-beeld van `market()`: **✨ Today's picks** = `SHOP_DAILY_N` willekeurige dagitems +
  **🎟 Hytale access** = passen), **onafhankelijk van `SHOP_TEST_DAY_PASS_ONLY`**. Zo kan de user het shop-
  design goedkeuren zonder het publiek te maken (de echte `/market` blijft in test-modus enkel de dagpas tonen).
- Reroll (`↻`) op de preview stuurt `?next=/admin/shop/preview` mee; **`admin_shop_reroll`** kreeg een
  `RerollQuery{next}` + veilige redirect (default blijft `/market`, dus de publieke knop ongewijzigd).
- **Gedeployed** (`./deploy/deploy.sh` → systemd `market` active) + route geverifieerd (303, geen 404).
  ~~GIT-DEBT: `src/web.rs` nog niet gecommit~~ → **ingehaald** op 2026-07-17 (commit `a0e7041`).

## ⏭️ Sessie (2026-07-16) — Accounts-tab + dagpas als "Bought"

Alles **live op prod** (`magicmeadow.org`, systemd `market`) en gepusht.
Commits: `d70c4a8` (feature) + `6f78435` (header-opschoning). Deploy via
`./deploy/deploy.sh` (bouwt release lokaal, stopt/herstart de service — korte
onderbreking voor spelers). Enkel `src/db.rs` + `src/web.rs` geraakt.

**(A) Manage → 👥 Accounts-tab.** Nieuwe subtab (tussen *Admin shop* en *Coins*),
route `/admin/accounts` → `admin_accounts` → `db::list_accounts(pool, now)`. Tabel
van **iedereen die ooit iets kocht**; kolommen: **Lid** (username + Hytale-naam),
**Dagpas actief** (Nee / Ja + resterende tijd via `fmt_dur`), **Permanente pas**
(Ja/Nee uit `coins.perma_access`). Bron = `inventory ∪ hytale_whitelist`, naam uit
`coins.username`. Elke `<tr>` draagt `data-uid` als haakje voor de latere extra
info / per-account acties (user: "later nog extra info, begin hiermee"). `.yes`/`.no`-
CSS toegevoegd. Header-subtitel ("iedereen die ooit iets kocht (N leden)") op vraag
weer **weggehaald** — de subtab labelt het al, site is intuïtief.
⚠️ "Ooit gekocht" = wie **nú** een rij heeft in inventory/whitelist; een lid dat via
"Reset all test purchases" gewist werd valt uit de lijst. Voor een res-bestendige
historiek is een aparte aankoop-log nodig (niet gebouwd).

**(B) Dagpas toont "Bought" zolang je pas loopt** (vervangt het verwarrende
"Out of Stock" van vorige sessie). Heeft de speler een **actieve** pas
(`db::get_whitelist(..).is_some()`, filtert verlopen al weg), dan is de dagpas-kaart
`bought` → grijs + ✓ + een niet-klikbare **"Bought"**-knop (`.buy.owned`). Herkopen
tijdens de looptijd blijft geblokkeerd in `db::purchase` (`"You already have an
active pass."`, telt `expires IS NULL OR expires > now` → vangt ook perma). Loopt de
timer af → weer koopbaar; `grant_day_whitelist` stapelt de duur bovenop de rest.
NB: dit ging heen en weer deze sessie — user wou eerst herkopen tóélaten, daarna
toch blokkeren maar getoond als "Bought" i.p.v. "Out of Stock". Dít is de eindstand.

**(C) Voorraadtelling verbergen bij admin-sold_out.** In `shop_slot`:
`if it.sold_out || it.stock < 0 → geen telling`. Zette een admin het item handmatig
op *Out of stock*, dan verdwijnt de "N left" — knop en telling spreken elkaar niet
meer tegen. (Een eerder gebouwde per-item "reset day passes"-knop is **verwijderd**:
overbodig zodra de pas-status het gedrag stuurt.)

## ⏭️ Sessie (2026-07-15g) — voorraad + 1 per persoon, embed-knop, poorten dicht

**(A) Voorraadsysteem vervangt "One at a time"** (die leefde één sessie; user: "dit werkt niet
zo"). Nu een echte teller: **`items.stock`** — **-1 = onbeperkt** (default, want gems mogen niet
plots uitverkocht zijn), anders het aantal dat nog te koop is. Manage-kaart heeft een eigen
**"+ Add stock"**-formuliertje dat **optelt** ("er komen er 3 bij" — zo denkt een admin) plus
een **∞**-knop om weer onbeperkt te zetten. De shop toont de voorraad (`3 left` / rood
`out of stock`), want dan weet een speler of wachten zin heeft.
**Twee onafhankelijke grendels:** voorraad = globaal (op 0 dicht voor iedereen tot je bijvult);
**1 per persoon** = wie een lopende pas heeft ziet Out of Stock, ook al ligt er nog voorraad.
Beide zitten in **`db::purchase`**, atomisch mét het afboeken van de coins — de voorraad telt af
met `WHERE stock > 0`, dus twee gelijktijdige klikkers nemen nooit samen de laatste mee.
`auto_sold_out` is weg, kolom incl. (`DROP COLUMN`), anders staan er twee mechanismen naast elkaar.
Getest met drie echte leden: 3 → koop → koper ziet dicht maar anderen `2 left` → … → 0 → dicht
voor iedereen → admin vult bij → open. ⚠️ Begrenst het **aantal aankopen**, niet de opgespaarde
tijd; het echte plafond blijft de TODO hieronder.

**(B) Shop ververst zichzelf.** `AUTO_REFRESH_JS` bestond al (Log/Coins/Channels) maar stond
níét op de shop → voorraad-aanvullingen verschenen pas na een handmatige F5. Nu een
`auto_refresh_js(ms)`-helper: **shop 5s** (user-wens: voorraad meteen zien landen), Log/Coins/
Channels blijven **20s** (daar zit je te lezen). Slaat over zolang je in een veld staat.
NB: een tabblad dat vóór de deploy geladen is heeft het script nog niet — één keer F5.

**(C) Embed-knop stuurde mensen weg.** Klacht "de redirect staat nog op, mensen geraken niet in
de shop" was **niet** de (verwijderde) gate: de knop `site_access` in de #🧺market-embed was een
**interactie**-knop die een ephemeral terugstuurde met `…/info` voor niet-admins. Nu een
**link-knop** → `https://magicmeadow.org/login?next=/market`: geen interactie dus **geen
ephemeral**, en iedereen landt na login op de shop. Bericht `1526273201456414894` in kanaal
`1403810528039665745` ge-PATCHt; Check In + Info bleven. De `site_access`-handler in `bot.rs` is
daarmee dood — **nog op te ruimen**.

**(D) 🔒 Poorten 8700 + 8090 dicht** (v4+v6). Aanleiding: user wou het server-IP aan spelers
geven. Dat is veilig — het IP staat sowieso in DNS en de whitelist gate't de game — maar de
doorlichting toonde dat **market op 8700 en het panel op 8090 rechtstreeks van het internet
bereikbaar waren, buiten Caddy om**. Daardoor was mijn "Caddy blokkeert `/internal/*`"-laag in de
praktijk géén laag: wie 8700 kende stond recht voor het revoke-endpoint (enkel het geheim hield
hem tegen). Nu: alles via het domein (Caddy → 127.0.0.1). Van buitenaf geverifieerd: IP:8700 en
IP:8090 onbereikbaar, `magicmeadow.org` + `/panel` + `/market` werken, `/internal` geeft 404.
⚠️ Rechtstreeks `https://IP:8090` werkt dus niet meer → gebruik **Manage → 🖥 Server**.

**(E) 🅿️ TODO — fail2ban + SSH.** SSH staat open voor **wachtwoord-login** en er draait **geen
fail2ban**: niets vertraagt iemand die eindeloos wachtwoorden probeert. Root mag enkel met
sleutel (goed). User: "todo voor later, we willen nu asap testen". Fail2ban is risicoloos;
SSH op keys-only vraagt eerst een bevestigde werkende sleutel (anders sluit je jezelf buiten).

**(F) Hytale-server draait nu onder Faybelle's profiel.** `discovery link` gaf 403 *"session
token needs to be from same profile as server"*: de server was ooit met `/auth login device`
onder **Waldstein** ingelogd, terwijl Faybelle de discovery-token uit háár account haalde.
Opgelost via console: `/auth logout` → `/auth login device` → zij bevestigde de device-code →
`Profile: Faybelle (963f818d-…)`, link geslaagd om 17:19, **sindsdien geen enkele 403 meer**.
⚠️ Vangnet `Server/auth.enc.bak-waldstein` staat er nog — weg zodra dit stabiel blijkt.
NB: `/auth select` = kiezen tussen game-profielen bínnen één account; voor een ánder account is
`logout` + `login device` nodig. De server is tussen die twee even niet geauthenticeerd.

## ⏭️ Sessie (2026-07-15f) — "One at a time" + chat-brug naar prod

**(A) Toelaten met de druppelaar.** User wou tijdens de testfase geen limietsysteem maar wel
beletten dat één iemand meerdere passen koopt (de pas kost 1 coin). Oplossing: nieuw vinkje
**"One at a time"** per item (`items.auto_sold_out`) — élke aankoop zet `sold_out` meteen
weer aan. De admin vinkt *Out of stock* af om precies één koper binnen te laten, waarna het
vanzelf sluit. **Staat AAN voor de dagpas** (id 21). De rem zit server-side in `buy()` (op
béíde koop-paden, pas én gewoon item), dus twee gelijktijdige klikkers glippen er niet door.
Elke automatische sluiting logt als `admin/auto_sold_out` met wie het slot opgebruikte.
Getest met twee echte leden: koop → dicht → 2e geweigerd → admin geeft vrij → koop → dicht.
⚠️ Dit begrenst het **aantal aankopen**, niet de opgespaarde tijd: dezelfde persoon kan bij
een volgende vrijgave nog eens kopen en stapelt dan naar 48u. Zie de TODO hieronder.

**(B) 🅿️ TODO — echte pas-limieten** (user: "todo voor later"). Twee grendels, apart te
bouwen: **(1) per speler** een plafond op *opgespaarde tijd* (niet op aantal aankopen — wat
telt is hoeveel toegang je tegelijk in handen hebt; `grant_day_whitelist` stapelt nu
ongelimiteerd). Veld op het item, naast `Access (minutes)`. **(2) community-cap**: tel geldige
passen (`expires > now` of NULL) en weiger bij vol ("server is full (5/5)"). Dat is
server-breed, dus hoort in een `settings`-tabel + admin-UI — meteen het eerste stuk van
[[params-to-ui]]. Open beslissingen: tellen permanente passen mee (voorstel: ja, en ze
schaars houden), en Faybelle valt buiten de telling (staat via `protected_names` op de
whitelist, niet via een pas) — dat lijkt juist.

**(C) Chat-brug staat nu op PROD.** `channel_id` in `/opt/hytale/bot/config.toml` ging van
`1523242084440608838` (#hytale-chat, dev) → **`1520079113002422302` (#🌼meadowland, Magic
Meadow)**. ⚠️ **Die config staat NIET in git** (bevat de bot-token) — enkel op de VPS;
backup: `config.toml.bak-chatchannel`. De brug is **tweerichtings** en dat is bewust
(user-keuze): wie in dat kanaal typt, praat mee in de game-chat.
**Argus zat niet in de prod-guild** — user heeft hem uitgenodigd
(`client_id=1522930621402316861`, permissions `536939520` = view/send/history/manage-webhooks;
webhooks zijn nodig voor de per-speler Hytale-kopjes). `#🌼meadowland` is besloten
(@everyone deny view; enkel Flowerborn+Betty), dus Argus kreeg een eigen kanaal-override voor
*View Channel*. Alle vier de rechten geverifieerd. NB: de tale-bot draait verder nog op de
**dev**-config (`environment=dev`, `guild_id=652452615879262220`) — enkel het chat-kanaal wijst
naar prod.

## 💾 BACKUP van coins.db — LIVE sinds 2026-07-15
Er was er **geen**: saldo's, aankopen, passen, shop-instellingen en het logboek leefden in
één bestand op één VPS. Nu: systemd-timer **`market-backup.timer`** (dagelijks, `Persistent=true`
→ haalt een gemiste run in, `RandomizedDelaySec=15m`) draait `/opt/market/backup-coins.py`
als user `market` → **`/opt/backups/market/coins-YYYY-MM-DD.db.gz`**, 30 dagen bewaard (~18 KB
per stuk).
Gebruikt SQLite's **online-backup-API**, geen `cp`: market schrijft door, een kale kopie kan
een half geschreven transactie vangen. Doet daarna `PRAGMA integrity_check` en gooit de
snapshot weg als die niet 'ok' zegt. **Herstelproef gedaan** (uitpakken → integrity ok → 20
leden, 15 items, 55 logregels leesbaar). Bron: `deploy/backup-coins.py` + de twee units.
**Nog open:** dit staat op **dezelfde schijf als de DB** — een off-site kopie (bv. naar de PC)
is er nog niet. Zie ook [[ops-techstuff]] (de oude backup-plannen daar zijn niet uitgevoerd).

## ⏭️ Sessie (2026-07-15e) — eerlijke verwijderknop, Out of Stock, 3D-knoppen

**(A) Panel-verwijderknop loog — nu eerlijk.** User meldde: pas gekocht, zichzelf verwijderd,
opnieuw gekocht → **47u i.p.v. 24u**. Diagnose: `grant_day_whitelist` **stapelt** bewust
(+24u bovenop resterende tijd — dat is een feature), maar de panel-knop deed enkel
`whitelist remove` op de Hytale-server en raakte **market's `hytale_whitelist` niet aan**.
Dus: de betaalde tijd bleef staan, de volgende aankoop stapelde erop, én de reconcile-lus
zette hem binnen 15s gewoon terug. De verwijdering was zinloos.
Nu doet `whitelist_remove` (panel): **kick → alle pas-bronnen wissen → van de whitelist**,
in die volgorde (eerst kicken, want de whitelist blokkeert enkel nieuwe joins; en
andersom zou de reconcile hem tussenin terugzetten). Mislukt de revoke → knop weigert en
zegt het eerlijk. **Geen coins terug** (user-keuze: moderatie-actie; de refund op de
logpagina blijft de vriendelijke variant).
**Koppeling:** het panel draait als `hytale` en kan `coins.db` (user `market`) enkel lézen
→ het **vraagt** market om te revoken via **`POST /internal/pass/revoke`**
(`db::revoke_pass_by_name`, hoofdletter-ongevoelig, wist ook `perma_access`, logt
`admin/pass_revoke`). Market blijft zo de enige schrijver van z'n DB.
**Beveiliging in 2 lagen:** Caddy geeft `/internal/*` van buitenaf een **404**, en market
eist een gedeeld geheim (constante-tijd-vergelijking; leeg geheim = alles weigeren).
⚠️ **Het geheim staat NIET in git**: market leest het uit `secrets.json` (mode 600), het
panel uit `EnvironmentFile=/opt/hytale/panel/market.env` (mode 600) — de unit-bestanden
zitten wél in git. Live geverifieerd: extern 404, fout/geen geheim 403, en de echte knop
wiste Waldsteins 47,6u-pas waarna de reconcile hem niet terugzette.

**(B) Out of Stock** — vinkje per item in Manage (`items.sold_out`, idempotente migratie).
Item blijft zichtbaar met een grijze dode "Out of Stock"-knop; de **échte** rem zit in
`buy()` (een grijze knop houdt niemand tegen die zelf POST — getest: geweigerd, geen coins
of pas bewogen). Wijziging komt in de log als `→ out of stock`.
⚠️ **Valkuil die me bijna prod sloopte:** `row_to_item` leest per naam, maar vier queries
hadden een **expliciete kolomlijst** zonder `sold_out` → `InvalidColumnName` bij élk
item-verzoek. Nieuwe Item-velden: **alle vier de `... FROM items`-SELECTs mee aanpassen.**

**(C) Cosmetisch:** Manage-kaarten 168 → **240px** (+ `max-width:100%`), en **alle** `.btn`-
knoppen kregen de 3D-druk van de Buy-knop (rand eronder die wegvalt terwijl de knop 3px
zakt; per variant een eigen donkerdere onderrand). **Klik-geluid volgt** — user bezorgt het
sample.

## 🅿️ GEPARKEERD — pauzesysteem met bevroren pas-timers (idee 2026-07-15)
Spelers **individueel of allemaal samen kicken** voor bv. dringende maintenance, waarbij
hun **dagpas-timers bevriezen** zolang de pauze duurt. Nu verliest een betalende tester
speeltijd bij elke onderhouds-restart — precies waarom `[[tale-geen-restart-tijdens-test]]`
een harde regel werd. Raakt market (`hytale_whitelist.expires` = absolute epoch, dus
bevriezen vraagt een pauze-offset of het omrekenen naar resterende seconden), het panel
(knop) en tale/Argus (kick + de PassTimer-HUD). **Nog niet gebouwd, niet beginnen zonder
overleg over het datamodel.**

## 🚦 GO-LIVE-SCHAKELAAR (klaarzetten op user-commando "we zijn go")
De **`gate`-middleware** (`web.rs`, ~r242) stuurt op dit moment **elke niet-admin** naar
`/info` — enkel `/info`, `/img/*`, `/login`, `/auth/callback`, `/logout`, `/healthz` en
`/favicon.ico` zijn publiek. Daardoor loopt de embed-knop naar de shop voor gewone leden
dood op de info-pagina. **Bij go-live:** die admin-check eruit zodat Flowerborns bij
`/market` raken en een dagpas kunnen kopen (`require_flowerborn` op `market()` blijft de
échte gate). User-instructie 2026-07-15: **"doe dit seffens als we klaar zijn voor de go"**
— dus **nog NIET doen**, wachten op zijn woord.

## 🔴 GAME-TEST — nog één tijdelijke instelling LIVE (2026-07-15)
> **Terugzetten vóór echte spelers erop komen:**
> - **`protected_names = ["Faybelle"]`** (`/opt/hytale/bot/config.toml`) — Waldstein is er
>   op eigen vraag uitgehaald voor de koop-test; zonder dat kan zijn eigen toegang verlopen.
>
> ✅ **`DAY_PASS_SECS` staat weer op `24 * 3600`** (stond even op 2 min voor de verval-test).
>
> Aan, en mag blijven tot de go: `SHOP_TEST_DAY_PASS_ONLY = true` (shop toont enkel de
> dagpas; prijs staat op **1 coin** in de DB — vóór de go op een echte prijs zetten) en de
> reconcile-lus op **15s** (was 1 min).

## ⏭️ Sessie (2026-07-15d) — shop-herwerking + prod-config-blocker + pas-keten live getest

**(A) Shop herwerkt.** Publieke shop = **4 dagitems** (`shop_offers`, willekeurig uit de 13
niet-boost items, voor iedereen dezelfde, stabiel tot middernacht UTC) + de **passen** los
eronder, altijd te koop. De vroegere volledige catalogus leeft voort als **Manage → 🛍 Admin
shop** (`/admin/shop`, alles koopbaar om te testen). Admins krijgen een **↻**-knopje naast de
dagitems (`/admin/shop/reroll`, gelogd). `shop_offers` bestond al maar stond achter
`#[allow(dead_code)]` — hergebruikt i.p.v. herbouwd. Verder: **Unequip**-knop op de geëquipte
gem (naamkleur terug + rol eraf, gem blijft; guard: enkel de gem die je écht draagt) en het
**gem-raster wrapt** nu (stond in een zijwaartse schuifstrip → je zag niet alles).

**(B) ⚠️ PROD-BLOCKER GEVONDEN EN GEFIXT — niemand kon in de shop.** `deploy/market.service`
zette `DISCORD_ROLE_ID=1525249217897955590`: de Flowerborn van de **dev**-guild. Die rol
bestaat niet in Magic Meadow → `has_role` gaf voor élk prod-lid false → alle **32** Flowerborns
zagen de regels-pagina i.p.v. de shop. Nu staan **guild én rol expliciet** in de unit
(`1296469405651435592` + `1399336425069219881`). Rolcheck nagespeeld: Waldstein, FayBelle,
TechHeadFred en ねこ krijgen toegang. **NB:** de kleurrollen (Amber, Ruby, …) bestaan in béide
guilds met andere ID's, maar worden op **naam** opgezocht (`role_id_by_name`) → die volgen de
guild vanzelf; enkel deze twee ID's moesten kloppen.

**(C) Pas-keten end-to-end live getest** (Waldstein, pas van 2 min):
`15:27:44 ✅ market-whitelist add Waldstein` → `15:29:43 🚫 Geen geldige pass meer — van de
whitelist gehaald (ok)`. Kopen → naam → whitelist → verval werkt volledig.
**Bevinding: een verlopen pas kickt niet** — de whitelist blokkeert enkel nieuwe joins, wie al
ingelogd is speelt door tot hij uitlogt. `kick <naam>` via de console wérkt (land-claim
`deploy.sh` gebruikt het), maar de fix hoort in **tale/Argus**, niet hier (user-beslissing).
Zie `[[tale-whitelist-passes]]`.

**(D) Naam-font van TechHeadFred — feature bewust geschrapt.** Zijn naam is platte ASCII; de
styling zit in `display_name_styles` (`{font_id: 8, effect_id: 1, colors: [747943]}`), een
Discord-Shop-cosmetic. 4 van de 38 leden hebben er een, twee met gradient. Het font zelf zit in
Discord's client — wij krijgen enkel een nummer, geen font-bestand → niet reproduceerbaar.
Admin bevestigde dat op hun server **de rollen** de kleur bepalen, nitro of niet.

## ⏭️ Sessie (2026-07-15c) — item-/prijs-logging + Shop/Inventory-filters op de logpagina

**Aanleiding:** de "Faybelle kreeg 1017 voor een heliodor van 1000"-vraag kostte een uur
forensiek, puur omdat **prijswijzigingen nergens werden vastgelegd**. User: "kunnen we niet
beter meer in detail loggen?" → ja, maar enkel de zeldzame admin-mutaties; niet elke
paginaweergave of coin-per-bericht (dat zit al in `earn_log`).

**Wat er al gelogd bleek:** gem-equip, booster-gebruik, admin-saldo-ingrepen, aankopen,
passen, refunds, test-reset. Die knoppen ontbraken enkel omdat er nog geen zulke rijen wáren
(de chips werden uit de aanwezige categorieën afgeleid).

**Toegevoegd (`web.rs`):** `admin/item_update` (leest de oude waarde vóór de schrijf → detail
"Ruby · price 40 → 60 · by Waldstein"; niets gewijzigd = géén regel), `admin/item_add`,
`admin/item_delete` (naam+prijs vastgelegd vóór het wissen). Nieuwe badges voor die drie +
`admin/correction` + `gem/unequip` (badge staat klaar; de **unequip-knop zelf bestaat nog
niet** — zie shop-herwerking).
⚠️ `amount` op `item_update` = de **nieuwe prijs**, geen coin-bedrag zoals bij de andere
regels. User weet dit; eruit halen als het verwart in de kolom.

**Filterknoppen = groepen (`LOG_GROUPS` in web.rs).** Eén knop mag meerdere categorieën
bundelen: **🎒 Inventory** = `gem` + `booster` (zat verspreid, is voor een admin één ding),
**🛒 Shop** = `shop`, **🪙 Coins** = `daily` + `level`. `db::recent_log` neemt nu `&[&str]`
i.p.v. `Option<&str>` (leeg = alles). Categorieën die in géén groep zitten krijgen alsnog
automatisch een eigen knop → een nieuw event-type kan nooit stil uit beeld vallen.

**Getest** (lokaal, echte routes tegen een kopie van prod-`coins.db`): Shop toont enkel
shop-events, Inventory bundelt equip+unequip+booster, Admin toont de prijswijziging; drie
POSTs op `/admin/item/update` (prijs, no-op, naam) → exact 2 logregels. **Gedeployed.**

## ⏭️ Sessie (2026-07-15b) — Hytale-panel onder Manage + wereldbeheer uitgezet

**(A) Panel → market Manage → 🖥 Server — LIVE + e2e geverifieerd.** User koos expliciet
**"gewoon een link + zelfde look"** boven een Rust-port of een iframe ("het mooist en het
efficiëntst voor nu"). Zie [[market-project]]-memory voor het volledige recept. Kort:
- `admin_subtabs` (web.rs) → 5e tab **🖥 Server** naar `/panel`; panel linkt terug naar Manage.
- Caddy `handle_path /panel*` → `reverse_proxy https://127.0.0.1:8090` + `tls_insecure_skip_verify`
  → **geldig cert, geen poort, geen waarschuwing**. (Subdomein kon niet: geen wildcard op Porkbun.)
- `panel.py`: nieuwe env **`PANEL_BASE=/panel`** (in `hytale-panel.service`), JS bouwt `B+pad`,
  `do_GET` aanvaardt ook `""`. CSS = market's thema + market-topbar.
- ⚠️ **Cookie-botsing gefixt:** panel zette `session` op `Path=/` — market's cookienaam. Nu
  **`panel_session`** op `Path=/panel`. Oude panel-sessies eenmalig ongeldig.
- Geverifieerd via Caddy: login → cookie `Path=/panel` → `/api/stats` (echte uptime 23u27m),
  `/api/whitelist`, `/api/console`. Apex + market ongemoeid.

**(B) Wereldbeheerpagina (`/techstuff`, ops :8091) UITGEZET** op vraag van de user: moet **van de
grond af herdacht** en **in de admin-sectie geïntegreerd** worden, "maar dat is niet voor nu".
`systemctl disable --now techstuff` + `handle_path /techstuff*` uit beide Caddyfiles. `/techstuff`
valt nu door naar market (303). Code blijft in `lab/ops`; niets hing eraan vast (geen
backup-cron/timer verwijst ernaar — geverifieerd).

**(C) Twee stille DB-bugs gevonden + gefixt** (`src/db.rs`, meegedeployd in dezelfde binary):
- **`total_earned`-backfill lekte via refunds.** De regel `UPDATE coins SET total_earned =
  max_balance WHERE total_earned < max_balance` was een **eenmalige** migratie van toen die kolom
  bijkwam, maar draaide bij **élke opstart**. Een refund verhoogt `coins` zonder verdiensten →
  `max_balance` volgt dat saldo → de eerstvolgende herstart promoveerde die refund stil tot
  "all-time verdiend", waar het **levelsysteem** op draait. Nu gegate op **`PRAGMA user_version`**
  (`< 1` → backfill + zet 1), dus enkel op een DB die de migratie nog nooit zag. Prod geverifieerd:
  `user_version = 1`, 0 rijen met `total_earned < max_balance`.
- **Refund kon het verkeerde bedrag/rij pakken.** `refund_purchase` zocht de inventory-rij met
  `ORDER BY id LIMIT 1` (= de **oudste** rij van dat item) en betaalde `inventory.price` terug. Bij
  een tweede aankoop van hetzelfde item trof dat de verkeerde rij, en bij een prijswijziging tussen
  beide aankopen het verkeerde bedrag. Nu: rij via **`ORDER BY ABS(acquired - ts)`** (dichtst bij de
  logtijd — `purchase` en `log_event` schrijven vlak na elkaar) en terugbetaling = **`amount` uit
  díe logrij** (wat er toen écht betaald is); `inventory.price` blijft enkel de terugval voor oude
  logrijen zonder `amount`.

**Status:** panel + Caddy + market-binary **gedeployed en geverifieerd** (prod-binary = deze build,
identieke sha256; geen errors in de logs sinds de herstart van 13:24).

**Openstaand / overwegen:** poort **8090** staat nog open in ufw en het panel bindt nog op
`0.0.0.0` — nu alles via Caddy loopt, kan dat dicht (`ufw delete allow 8090/tcp` + bind op
127.0.0.1). Niet gedaan: zou het rechtstreekse `https://IP:8090` breken, en dat is niet gevraagd.

## ⏭️ Sessie (2026-07-15) — uurlijkse shout-out → top 10-embed zonder tags, alfabetisch
> **AF — layout beslist door de user, gebouwd, gedeployed en in dev #general goedgekeurd
> ("de embed is prima").** De varianten A-D hieronder zijn achterhaald. Eindvorm:
> - **titel** = `⏳ Earners of the last hour` (user schreef "Eaners"/"…" — typo + puntjes
>   weg op zijn bevestiging; de ⏳ bleef uit de bestaande conventie).
> - **10** grootste verdieners i.p.v. 5 (`HOURLY_SHOUTOUT_TOP = 10`).
> - **alfabetisch op naam** i.p.v. op coins → de DB selecteert nog steeds de top 10 op coins
>   (`ORDER BY total DESC LIMIT 10`), daarna `sort_by_key(name.to_lowercase())`.
> - **medailles 👑🥈🥉 eruit** (iedereen 🌼) — bij een alfabetische lijst suggereren die een
>   rangschikking die er niet is. Voorgesteld door Claude, door de user bevestigd.
> - drempel **≥1 coin** (`HOURLY_SHOUTOUT_MIN = 1`).
>
> **Visuele check gedaan** (dev #general, bericht `1526902795129716883`): omdat het laatste uur
> maar 1 verdiener had (Waldstein, 1 coin) is een **als demo gelabelde** post over de laatste
> **7 dagen** gestuurd — 9 namen, dus de sortering is écht zichtbaar. Script:
> `scratchpad/demo_post.py` (repliceert de opmaak 1-op-1; leest prod-`coins.db` read-only +
> post via bot-token & Discord REST). Meteen bevestigd: sortering is hoofdletter-ongevoelig
> (`easycomes` tussen CookiesOfOreo en FayBelle), `escape_md` ontsnapt de blokhaken in
> `Yâ-Ôd [Kalia Lune de Demain]`, `ねこ` sorteert achteraan.
> **NB:** posten vereist de bot-token uit `secrets.json`; de auto-mode-classifier blokkeert dat
> uitlezen tenzij de user het expliciet vraagt/toestaat (dat was hier het geval).

**Aanleiding (user-vraag):** de uurlijkse shout-out postte een **apart bericht per lid** dat ≥100
coins verdiende, **met een mention** (`<@uid>, wow you've earned…`). Dat pingt leden elk uur.
Nieuwe wens: **één embed, top 5, géén tags, iedereen met minstens 1 coin komt in aanmerking**.

**Gewijzigd (`src/bot.rs` + `src/db.rs`, lokaal):**
- `HOURLY_SHOUTOUT_MIN` **100 → 1**; nieuwe const **`HOURLY_SHOUTOUT_TOP = 10`**.
  `HOURLY_TEST_MIN` 3 → 1 (test-modus volgt dezelfde drempel).
- **`db::hourly_earners`** kreeg een **`limit`-param** → `LIMIT ?4` op de bestaande
  `ORDER BY total DESC`. Verder ongewijzigd (venster + `HAVING total >= min` bleven).
- **`hourly_shoutouts`** postte een `say()` per lid; doet nu **één `CreateEmbed`** in
  `PROD_COINS_CHANNEL_ID`, in dezelfde stijl/kleur (`0x6B_9B_52`) als het weekly leaderboard,
  maar **alfabetisch en zonder medailles** (iedereen 🌼 — zie de eindvorm bovenaan). Namen als
  **platte tekst** i.p.v. `<@uid>`.
- Nieuwe helper **`escape_md()`** — namen staan nu als platte tekst in het embed, dus een `_` of
  `*` in een Discord-naam zou de opmaak van de regel breken. (Het weekly leaderboard gebruikt nog
  mentions en heeft dit dus niet nodig.)

**Getest:** `cargo build` + `--release` groen (enkel de bekende `role_id`-warning). De query
gevalideerd op een **kopie van `coins.db`** met gezaaide rijen: exact 5 rijen, aflopend gesorteerd,
venster gerespecteerd (rijen vóór/na het uur vallen weg), #6 en #7 correct afgekapt door de LIMIT.
Live testen kan niet zonder in het **echte prod #coins** te posten (kanaal-ID is hardcoded), dus
niet gedaan.

**Geverifieerd onderweg:** de `COALESCE(c.username, e.user_id)`-fallback in `hourly_earners` kan
in de praktijk niet vuren — `log_earn_event` wordt **binnen** `db::award` (en de daily-claim)
aangeroepen, dezelfde call die de username upsert. Een `earn_log`-rij impliceert dus een naam →
geen kale snowflake in het embed.

**Varianten voorgelegd aan Faybelle** (inhoud identiek, enkel vorm):
- **A** = zoals gebouwd: titel "⏳ Top earners of the past hour" + medaille-lijst.
- **B** = "🪙 Hourly top 5" + introzin + genummerd 1-5 *(Claude's voorkeur: rijmt op het weekly
  leaderboard en de introzin duidt meteen het venster).*
- **C** = winnaar uitgelicht met een regel eronder, rest compact.
- **D** = uitgelijnde kolommen in een codeblok. **Let op:** in een codeblok rendert de
  Meadowcoins-emoji **niet** (wordt letterlijke `<:Meadowcoins:...>`-tekst) en kan het kader op
  smalle gsm-schermen afbreken.

**Volgende stap:** variant verwerken → `cargo build --release` → `./deploy/deploy.sh` → commit.

## ⏭️ Sessie (2026-07-14 nacht) — chest-state volledig herstart-persistent + rescue-commando + odds
> **GEBOUWD + GEDEPLOYD + LIVE** (systemd `market` active) + **GECOMMIT** (`f2f716e`)
> + **GEPUSHT** op 2026-07-15 (`tale-gh/master` `c4225c8..0ea1b57`, `market-gh/main`
> `bdee3a0..ca7aa38`). Geen git-schuld meer.

**Aanleiding:** er spawnde telkens te snel een nieuwe treasure chest. Oorzaak: de hele
`ChestTracker` (cooldowns, `active`, lopende chests, pop-timers) leefde **enkel in geheugen** →
elke botherstart/redeploy wiste die staat. Twee zichtbare gevolgen: (1) dubbele chest vlak na
een deploy omdat de per-kanaal-cooldown verdween; (2) een chest die openstond tijdens een
herstart werd **wees** (dode knop, spelers wachtten op winst die nooit kwam).

**User-regel (nieuw, hard):** *alle timers en cooldowns moeten persistent zijn — geen enkel
state-verlies meer bij een botherstart.*

**Opgelost (`src/bot.rs` + `src/db.rs`):**
- **Chest-cooldown persistent** — nieuwe tabel `chest_cooldowns(channel_id, until)`.
  `pop_chest` + de rescue schrijven de 50→**60 min** rust weg; bij opstart in
  `cooldown_until` geladen (`db::set_chest_cooldown` / `db::load_chest_cooldowns`).
- **Lopende chest + pop-timer persistent** — nieuwe tabel `live_chests(message_id,
  channel_id, pop_ts)`. `do_spawn_chest` schrijft de chest weg; `pop_chest`/rescue wissen
  hem. Bij opstart (`setup`-closure) laadt de bot elke lopende chest terug, herstelt de
  **deelnemers uit het logboek** (`db::chest_joiners_from_log`), en herplant pop-taak + ticker
  voor de **resterende** tijd via de nieuwe helper `schedule_chest_tasks` (popt meteen als het
  pop-moment al voorbij is). `do_spawn_chest` gebruikt nu diezelfde helper.
- **Rescue-commando** `!chestrescue [message_id]` — **admin-only** (`admin_only`-check via
  `web::is_admin`, werkt óók op de prod-guild). Zonder id zoekt het de laatste **verweesde**
  chest op (`db::last_unresolved_chest`: join-events zonder `win`/`despawn`). Het trekt een
  winnaar (gewogen op Lucky Horseshoe), betaalt uit, **wist het dode chest-bericht** en post de
  **identieke** "The Magic Chest opened!"-embed in #general → niet te onderscheiden van een
  echte opening. Tip: draai het vanuit de **dev-guild** zodat de `✅`-bevestiging niet in prod
  verschijnt. Werd deze sessie succesvol gebruikt om de 23:27-wees-chest alsnog uit te betalen.
- **Chest-odds** — de fijnkorrelige 10-tier-verdeling (50–1000 coins) is nu de **live** trekking
  (verving de grovere 5-tier); `CHEST_TIERS_PROPOSAL` verwijderd, `!chestodds` toont één tabel.

**Al herstart-veilig (geverifieerd, niet aangeraakt):** coins-cooldown (`last_award`),
daily/streak (`last_daily`), 24u-ticketrollen (`role_grants` via sweeper). Achtergrondtaken
(hourly shout-out, weekly leaderboard) putten uit `earn_log` en herberekenen hun timing uit de
klok → geen dataverlies bij herstart.

**Open/afgesproken:** user wilde **geen** live test. Randgeval blijft: een chest die tijdens
langdurige downtime z'n pop-moment passeert, popt bij de eerstvolgende opstart (bedoeld gedrag).

## ⏭️ Sessie (2026-07-14 avond) — embed-"site"-knop: admins → /market, rest → /info, LIVE
> **GEBOUWD + GEDEPLOYD + LIVE + door user bevestigd + GECOMMIT/GEPUSHT** (`28285d1` op `master`,
> subtree-gepusht `market-gh main` `e20b0b5..2259c50`).

**Aanleiding:** de "site"-knop in de Discord-embed (`site_access`) gaf iedereen enkel een
under-construction-melding. De user wil dat **admins** vanuit die knop in de **echte market**
raken, terwijl **gewone leden** op de publieke **`/info`**-pagina blijven.

**Kernprobleem:** de web-`gate` stuurt niet-admins naar `/info`, maar kan een **niet-ingelogde
admin niet herkennen** (geen sessie-cookie) → die belandde óók op `/info`. De enige plek waar de
identiteit al bekend is vóór login is de **Discord-knop** (bot kent `mc.user.id`).

**Oplossing (`src/bot.rs` + `src/web.rs`):**
- `site_access`-handler reageert nu per persoon: **admin** → ephemeral link `…/login?next=/market`;
  **niet-admin** → `…/info`. (`crate::web::is_admin` — daarvoor `is_admin` `pub(crate)` gemaakt.)
- `/login` accepteert `?next=<pad>` met **open-redirect-guard** (`safe_next`: enkel paden die met
  één `/` beginnen, niet `//`); bewaard over de OAuth-roundtrip via `oauth_next`-cookie; `callback`
  keert daarheen terug i.p.v. altijd `/`. Non-admins met `next=/market` worden door de gate alsnog
  naar `/info` geleid — enkel admins raken echt in de market.
- **Bugfix onderweg** (gaf "Something went wrong" bij inloggen): twee `Set-Cookie`-headers werden
  met `insert` gezet → de tweede (`oauth_next`) overschreef de eerste (`oauth_state`) → CSRF-check
  faalde. Nieuwe helper **`set_cookies()`** gebruikt `append`. Idem bij de `callback`.

**Open follow-ups:** geen dwingende. Admin moet één keer via Discord-login (zit in de link); zolang
de sessie-cookie leeft gaat het daarna direct door.

---

## ⏭️ Sessie (2026-07-14 nacht) — volledige market-event-logging + refunds op de logpagina, LIVE
> **GEBOUWD + GEDEPLOYD + LIVE + geverifieerd + GECOMMIT/GEPUSHT** (`deploy.sh` → systemd `market`
> `active (running)`, geen errors/panics in de opstartlogs, migratie schoon; `refunded`-kolom bevestigd
> aanwezig op de prod-`coins.db`; nadien gecommit op `master` + subtree-gepusht `market-gh main`).

**Aanleiding:** chest-debugging leerde dat er bij een drukke market dingen kunnen mislopen; we
wilden op een **audittrail** kunnen terugvallen om alles recht te zetten. Het bestaande server-log
(enkel chest-events) is nu uitgebreid naar **alle relevante market-events** + admins kunnen
aankopen **terugdraaien** vanaf diezelfde logpagina.

**1) Uitgebreide event-logging** (hergebruikt de bestaande generieke `db::log_event`/`LogEntry`;
elke call faalt zacht → kan bot/site nooit crashen). Nieuwe categorieën + badges op `/admin/log`
(filterknoppen verschijnen automatisch):
- **`shop`** — `buy` (gewone aankoop/gem/collectible), `pass_day`, `pass_perma` (met Hytale-naam in detail).
  Dragen nu **`ref_id = item_id`** zodat een refund weet wát terug te draaien. *(`web.rs` `buy`)*
- **`gem` / `equip`** — gem geëquipt *(web.rs `use_gem`)*; **`booster` / `use`** — hoefijzer verbruikt *(`use_booster`)*.
- **`daily` / `checkin`** (bedrag + streak + saldo, `bot.rs handle_daily`) en **`level` / `levelup`**
  (1%-bonus + bereikt level, `bot.rs`). **Per-bericht-coins bewust NIET gelogd** (te veel ruis;
  daar dient `earn_log` al voor).
- **`twitch` / `whitelist`** (channel-points-pas toegekend) + **`twitch` / `rejected`** (ongeldige naam,
  refund van de punten) *(`twitch.rs on_redeem`)*.
- **`admin`** — `coins_add`/`coins_set` (mét welke admin: `by <naam>`), `coins_undo`, `coins_restore`,
  `coins_discard`, `reset_collection` *(`web.rs`, `apply_coin_op` kreeg een `admin`-param)*.

**2) Refunds op de logpagina** (user koos: op het log, niet als aparte tab; **volledige** terugdraai):
- Elke **shop-aankoop-rij** krijgt een **`↩ Refund`**-knop (met confirm); al gerefund → grijze
  `↩ refunded`-tag; niet-shop-rijen geen knop. Extra tabel-kolom + CSS in `admin_log`.
- **`db::refund_purchase(pool, log_id)`** (één transactie, spiegelt `reset_test_collection` maar per-item):
  coins terug (prijs uit de **inventory-rij** — klopt ook als het shop-item nadien gewijzigd/verwijderd is),
  inventory-rij weg, en neveneffecten per **event-type**: pas → whitelist-grant weg (+perma → `perma_access=0`);
  geëquipte gem → `equipped_gem`/`name_color` leeg + **Discord-rol intrekken bij de koper** (async in de
  handler, via `RefundOutcome.gem_role_removed` + `buyer_uid`); booster → `chest_luck=0`. Rij markeert
  `refunded=1`, en de refund zelf wordt gelogd als **`admin/refund`** (mét welke admin).
- **`/admin/refund`**-route + `admin_refund`-handler (admin-only); mislukte refund → **foutbanner** op het
  log (`LogQuery.err`). Nieuwe kolom **`server_log.refunded`** via `ensure_column` (additief/idempotent).

**Getest:** `cargo build` groen (enkel bestaande `role_id`-warning); alle refund-SQL gevalideerd tegen
een **kopie van de echte `coins.db`** + een **volledige refund-simulatie** (380→500 coins, inventory
leeg, gem unequipped, `refunded=1`). Prod na deploy: geen errors, `refunded`-kolom bevestigd, `/admin/log`
→ 303 oningelogd.

**⚠️ Belangrijk / grenzen:**
- Logt + refundt **vanaf deze deploy vooruit** — oudere aankopen staan niet in het log en zijn niet via
  de knop terug te draaien (daarvoor blijft de handmatige **Coins-management**-tab).
- Een **pas-refund trekt de héle whitelist** van dat lid in — bij gestapelde dagpassen sneuvelt alles
  ineens (staat als comment in de code).
- **Git:** gecommit + subtree-gepusht (`market-gh main`). De 2026-07-13-server-log-sessie zat al in
  HEAD (commit `515cb9c`) — die "nog niet gecommit"-noot verderop was stale, geen echte schuld meer.

## ⏭️ Sessie (2026-07-14 late) — Lucky Horseshoe-effect LIVE (chest-luck) + pas-check
> Gebouwd, **op dev getest** (web-only, `MARKET_WEB_ONLY=1` op een DB-kopie met gesmede sessie),
> **GEDEPLOYD + LIVE** (`deploy.sh` → systemd `market` actief, migratie schoon gelopen op prod-DB)
> en **gecommit**. Nog te doen: **de gewogen chest-trekking live valideren op de dev-guild** met
> `!chest` (kansmatig, dus lokaal niet te bewijzen — user test later).

**Lucky Horseshoe = chest-luck-booster (eindelijk werkend).** Use verbruikt één hoefijzer en geeft
**dubbele lot-kans** (2 loten i.p.v. 1) bij de eerstvolgende **uitbetalende** treasure chest waaraan
het lid meedoet. Eenmalig; nadien op.
- **`db.rs`**: nieuwe kolom `coins.chest_luck` (0/1). `activate_horseshoe` (atomisch: verbruik 1 exemplaar
  + zet vlag; **guard**: al actief → `Ok(false)`, geen tweede hoefijzer opbranden; geen bezit → `Err`),
  `chest_weight` (2 bij actief, anders 1), `has_chest_luck`, `clear_chest_luck`. `owned_booster_items`
  geeft nu ook `item_id` terug. Reset ruimt `chest_luck` mee op.
- **`bot.rs`** `pop_chest`: winnaar-trekking is nu **GEWOGEN** i.p.v. uniform (hoefijzer-houder = gewicht 2).
  Boost wordt verbruikt bij **álle** deelnemers die er een hadden — **enkel bij een echt uitbetalende chest**
  (niet bij een despawn). Win-embed toont *"🍀 Their Lucky Horseshoe doubled the odds!"* als de winnaar er
  een had.
- **`web.rs`**: route `/use/booster` + handler; Use-knop bekabeld (was "coming soon"). Actieve boost →
  **banner "🍀 A Lucky Horseshoe is active…"** (LOS van bezit — anders verdween met je laatste hoefijzer
  ook élk teken van de boost; **op dev gevonden UX-gat, meteen gefixt**) + kaart-badge "Active" + knop grijst uit.
- **⚠️ Latente prod-bug gefixt (nooit getriggerd):** de Lucky Horseshoe stond in mijn dev-DB als categorie
  **`boost`** (= Hytale-pás) met `duration=0` → kopen zou via de pas-flow **permanente Hytale-toegang** geven!
  Migratie corrigeert nu robuust `Lucky Horseshoe` → `booster` bij élke andere categorie (prijs/afbeelding
  blijven). Geverifieerd: niemand had hem gekocht, niemand kreeg onterecht toegang.

**Pas-vraag beantwoord (geen codewijziging nodig):** dagpas kopen, 5 min later permapas → **werkt vlot**.
`grant_perma_whitelist` doet `ON CONFLICT(user_id) DO UPDATE SET expires=NULL` → je whitelist-rij wordt
**ter plaatse opgewaardeerd** naar permanent, geen dubbele rij, geen toegangsonderbreking (enkel resterende
dagpas-uren vervallen, geen verrekening). Omgekeerd (dagpas ná perma) wordt correct geblokkeerd in `purchase`.

## ⏭️ Sessie (2026-07-14 avond) — website: gems-collectie + passen + Discord-rollen + UX
> **Parallelle "website-sessie"** (naast de Twitch-sessie hieronder). Alles GEBOUWD + lokaal e2e
> getest (poort 8710, `MARKET_ENV=dev`) + **GEDEPLOYD + LIVE** + gecommit/subtree-gepusht
> (`market-gh main`). Tip: test lokaal met `MARKET_PORT=8710` náást een andere instance (nieuw:
> bind-poort configureerbaar via `MARKET_PORT`, default 8700).

**Categorie-model omgegooid** — items zijn nu `inventory` / `noninv` / `booster` / `boost` (=Hytale-pas):
- Idempotente migratie: oude gem-categorieën (primary/secondary/prism) + plain `''` → `inventory`;
  Lucky Horseshoe → `booster`. Nieuwe items default `inventory`. `seed_gems` niet meer aangeroepen.
- Manage Type-dropdown: Inventory / Non-inventory / Booster (lucky item) / Hytale pass. Role-name-veld
  + kleurkiezer verwijderd (gem-naam = rolnaam; kleur komt uit Discord — zie onder).

**Inventory = verzamelkaart die de shop volgt** (💎 Gems-tab):
- Elk `inventory`-item krijgt een kaart: **grijs met "?"** tot aankoop, daarna onthuld (afbeelding +
  naam + uitleg). Shop: reeds gekochte `inventory`-items → **grijs + groene ✓** (geen Buy). De
  **permanente Hytale-pas** grijst óók zodra `perma_access`. `purchase`: `inventory`-items éénmalig.

**Gem-kleur uit Discord-rollen** (gem-naam = rolnaam):
- `db::sync_gem_colors` zet `items.color` op de kleur van de gelijknamige Discord-rol. Sync bij opstart
  + admin-knop **🎨 Sync gem colors** op Manage. Guild = dev in dev-env, prod (`COINS_GUILD_ID`) in prod
  (`color_guild()`). `discord_rest::list_roles`.
- **12 gem-rollen op de dev-server aangemaakt** (met de prod-kleuren, via Fortuna's token). Gem "Lolite"
  was een typo → hernoemd naar **Iolite** (+ Iolite-rol op dev). ⚠️ **Rol-HIËRARCHIE**: gem-rollen in dev
  boven Hytaler getild (zodat de gem-kleur wint), maar bot-rol **Fortuna staat op pos 3** → de bot kan de
  gems niet boven hoger-gekleurde test/admin-rollen (SuperUser/Red/…) tillen. Sleep Fortuna's rol hoger
  (dev én prod) voor waterdichte gem-kleuren.

**Fase 2 — gem-preview + Use → Discord-rol:**
- Preview: klik een bezeten gem → je naam live in díe gem-kleur op **zwart/wit-swatches**, Discord-achtig
  font (`GEM_PREVIEW_JS`). De swatches zijn **sticky**.
- **Use** kent de gelijknamige Discord-rol toe in `cfg.guild_id` (= de **dev-guild**, óók op prod → rol
  landt op dev). Vorige gem-rol wordt eerst ingetrokken (max. één kleur tegelijk). `coins.equipped_gem`
  + `discord_rest::role_id_by_name`. **E2e getest** (Ruby toegekend+verwijderd in dev). De kleur toont pas
  als de gem-rol boven de andere gekleurde rollen staat (zie hiërarchie).

**Fonts/achtergrond uit Discord (onderzoek):** een user-**font** zit in `display_name_styles.font_id`
(ophaalbaar; TechHeadFred=8) — haalbaar, maar Discord's fonts zijn proprietary → enkel benaderbaar met
een webfont (**nog niet gebouwd**). Een **achtergrond/thema-kleur** is NIET haalbaar: persoonlijk
client-thema = privé; profiel-thema-kleuren (`/users/{id}/profile`) = **403 voor bots** (getest).

**Hytale-passen vereenvoudigd:** geen instelbare duur — dagpas = vast **24h** (`DAY_PASS_SECS`), permanent
= eeuwig. Manage toont read-only "Access"; het minuten-veld is overal weg.

**Boosters** (🍀 op de Boosts-tab): bezeten `booster`-items (Lucky Horseshoe) met aantal ×N + een
**werkende Use-knop** (chest-luck-effect — zie sessie *2026-07-14 late* bovenaan). Geen verzamel-/
grey-out-logica; herkoopbaar.

**🧪 Reset all test purchases** (Gems-tab, admin-only): refundt ALLE aankopen (gems + passen + boosters),
maakt inventory leeg, verwijdert de **`hytale_whitelist`-grant + `perma_access`**, reset naamkleur/equipped-
gem + trekt de gem-rol op Discord in. **All-time saldo blijft ongemoeid.** Zo test je gems én passen: koop
→ op de whitelist in het panel (tale-bot reconcilet ~1 min) → reset → eraf. (Waldstein blijft op
`whitelist.json` via `protected_names`; de pas-**timer** in het panel is wat je test.) Bevestigingsdialoog weg.

**UX-polish:** zwevende (sticky) **Purse** (shop) + preview-swatches; **scrollpositie behouden** na élke
actie (`KEEP_SCROLL_JS` op shop + inventory + Manage); **image-caching** (`/uploads` Cache-Control immutable
+ `?v=<mtime>`-cachebuster) → geen herlaad-flits van graphics bij een Buy; **drag-&-drop** upload op de
Manage-afbeeldingskaders; **2e afbeelding** per item + **auto-save** van de Manage-velden (sendBeacon);
item-omschrijving **cursief** in de shop; **bredere shop-kaders** (210px); Amber-prijs-fix.

**Open / volgend:** (1) **font-in-preview** bouwen (`font_id`→webfont-mapping); (2) **Fortuna's rol hoger
slepen** (dev+prod) voor waterdichte gem-kleuren; (3) ~~Booster-Use-logica (Lucky Horseshoe-effect)~~ ✅
**GEDAAN** (sessie *2026-07-14 late*) — enkel nog live valideren op dev-guild; (4) losse **Ruby-testrol** bij
Waldstein in de **prod**-guild opruimen; (5) shop members-zichtbaar maken (site-gate weg) zodra de graphics af zijn.

## ⏭️ Laatste sessie (2026-07-14 avond) — Twitch-pas → Rust + tale fd-lek + panel-timer
> Parallelle sessie naast de website-sessie (die aan `web.rs`/templates werkte) en een
> tale-sessie. **Monorepo `lab`** — market + tale delen één git-repo. Alles hieronder
> **gecommit**; de tale-kant is **gedeployed + live op de VPS**, `src/twitch.rs` nog niet.

**Twitch-pas-luik geport van tale (Python) → market (Rust)** — `src/twitch.rs` (`2835e5f`):
- Port van het oude `tale/bot/twitch_bridge.py`. Een channel-points-redeem "Hytale-ticket (24u)"
  → token-refresh + Helix (reward beheren, redemption fulfill/cancel, chat) + EventSub-WebSocket
  (tokio-tungstenite) met reconnect/backoff.
- **Model schoner dan de Python-versie:** raakt de Hytale-FIFO NIET meer aan. `on_redeem` leest de
  ingetypte naam uit `event.user_input`, valideert via `web::valid_hytale_name`, zet 'm vast op de
  1e redeem, en schrijft een grant in `coins.db.hytale_whitelist` onder pseudo-id `twitch:<id>`
  (`db::grant_day_whitelist`, +24u stapelend). De tale-bot reconcilet die al → whitelist.
- Config: `[twitch]`-velden in `Config` (secrets.json) + `twitch_ready()`; kost dev 0 / prod 1500.
  Start in `main.rs` los van de Discord-gateway.
- **E2e getest via de Twitch CLI EventSub-mock** (`~/.local/bin/twitch`, geen Affiliate nodig):
  `TWITCH_EVENTSUB_URL=ws://127.0.0.1:8080/ws` → mock-modus (skip Helix/token). Bewezen: grant,
  naam-vastzetten, stapelen 24→48u, refund bij ongeldige naam. Gids: `docs/twitch-setup.md`.
- ⚠️ **Nog NIET op de VPS gedeployd** — enkel lokaal gecommit. Deploy nodig zodra echte
  Twitch-redeems live gaan (met de Affiliate-**prod-streamer**; OAuth-flow ligt klaar).

**tale opgeruimd** (`cab33de`): `twitch_bridge.py` + `[twitch]`-bedrading (import/start/stop/
`/twitchsetname`) uit `bot.py`, `[twitch]` uit config.example.toml, sectie 9 uit SETUP.md,
`twitchAPI` uit requirements. `reconcile_market`/`enforce_whitelist` BLIJVEN. **Gedeployed + live.**

**FD-LEK in de tale-bot gevonden + gefixt** (`0537f41`) — dit was de échte reden dat market-grants
niet whitelistten (niet het Twitch-luik, niet UUID's): `bot.py` `db()` deed `with sqlite3.connect()`
zonder `close()` → lekte fd's naar `links.db` tot de 1024-limiet in ~2 dagen → dan vielen reconcile
(sqlite-open faalt → lege grants), `ensure_protected` én de Discord-gateway stil uit ("Too many open
files"). db() is nu een `@contextmanager`. **Gedeployed + herstart + geverifieerd** (fd's vlak op 0).

**Panel-resttijd-timer voor ALLE passen** (`cf003d7`) — `tale/panel/panel.py` `pass_expiries()` las
enkel de bot-DB; leest nu óók market's `coins.db.hytale_whitelist` → resttijd zichtbaar voor shop-
én Twitch-passen (dagpas = live afteller, `expires NULL` = "permanent"). **Gedeployed + live.**
Basis voor de gewenste latere **in-game resttijd-berichten** bij join.

**Whitelist-keten LIVE bewezen:** test-grant in prod `/opt/market/coins.db` (`Waldstein`, 24u) →
tale-bot → `whitelist add` → op `whitelist.json` + panel toont de timer. Test-grant
`twitch:waldstein-vpstest` **blijft bewust staan** (houdt Waldstein 24u erop; opruimen wanneer hij
zich permanent terugzet). NB whitelist = UUID's; `protected_names=["Faybelle","Waldstein"]`.

**Open:** (1) echt Twitch-redeem-mondstuk met de Affiliate-prod-streamer; (2) `src/twitch.rs` naar
de VPS deployen als dat live gaat; (3) in-game resttijd-bericht bij join; (4) evt. Client Secret roteren.

## ⏭️ Sessie (2026-07-14 mid) — 2e afbeelding (plain items) + auto-save Manage
> **Gebouwd + lokaal e2e getest + GEDEPLOYED + LIVE** (12:59) én **gecommit** (`ffc7f83`).
> Nog te subtree-pushen naar `market-gh` (gebeurt onderaan deze sessie).

- **Tweede afbeelding voor plain items** (categorie `''`): nieuwe kolom `items.image2`
  (idempotente migratie, prod bevestigd), `set/clear_item_image2`, alle 4 item-SELECTs dragen
  `image2`. De upload-handler leest nu een `slot`-veld (`2` → tweede afbeelding, bestand
  `item_<id>_2.<ext>`); nieuwe route `/admin/item/image2/clear`. **Shop** toont de 2e afbeelding
  **kleiner, gecentreerd, onder de titel** (`.thumb2`, max 62%/64px). **Manage** krijgt per plain
  item een image2-blok (preview + "Upload 2nd" + "Remove 2nd"). Enkel plain items (gems/passen niet).
- **Prijs-footgun opgelost** (`AUTOSAVE_JS`): elk item-update-form persisteert zich nu **automatisch
  bij een veldwijziging** (op `change`, via `navigator.sendBeacon` zodat het een navigatie overleeft)
  met een korte **"✓ Saved"**-flits (`.autoflash`, onderaan de kaart). Oorzaak van de Amber-bug: de
  kaart heeft meerdere losse `<form>`'s; het prijsveld zat enkel in het 💾 Save-form, dus wie 1000
  typte en dan **Upload** klikte, verloor de prijs. Nu blijft elke edit bewaard.
- **Amber-fix**: prod-`coins.db` item 38 `price 0 → 1000` (directe DB-write met user-akkoord).

## ⏭️ Sessie (2026-07-14 vroeg) — deftige per-item CRUD op Manage Shop
> **Gebouwd + gecommit + subtree-gepusht** (`market-gh main`, `a8767ac..5dbeda1`, lokaal commit
> `d2f673b`) én **GEDEPLOYED + LIVE** (2026-07-14 12:02). Prod draait de nieuwe CRUD.

De Manage Shop-pagina (`/admin/market`) had gebrekkige item-CRUD; elk item is nu een volwaardige
beheerkaart (render + alle acties lokaal e2e geverifieerd tegen een web-only instance):

- **Duidelijke 💾 Save-knop onderaan** het update-formulier (i.p.v. de verwarrende ✓ midden naast
  de prijs die alle velden bewaarde maar prijs-only leek).
- **Bevestiging na een actie** — Save/shelf-move/image-clear redirecten met `?saved=<id>` → een
  groene **"✓ Saved"**-flits op díe kaart (`.savedflash`, fade na 2,5s via `SAVED_FLASH_JS`, dat
  ook `?saved` uit de URL strippt zodat een refresh niet herflitst). Werkt samen met `KEEP_SCROLL_JS`
  (keyt op `location.pathname`, dus scroll blijft behouden).
- **Categorie-select bevat nu ook "Hytale pass (boost)"** → passen aanmaakbaar via de UI (voorheen
  enkel gem-categorieën primary/secondary/prism + "geen gem"). Labels verduidelijkt ("gem · primary"
  enz., "— plain item —").
- **Volgorde**: **◀ ▶** verschuiven binnen zone/schap. `db::move_item(id, dir)` herschrijft de
  posities lineair (robuust ook bij oude/gelijke posities).
- **Item naar ander schap**: dropdown + **Move** (enkel getoond bij >1 schap; `db::set_item_shelf`
  hangt het achteraan het doelschap). Lucky-zone-items krijgen geen schap-dropdown.
- **Afbeelding wissen**: **"Remove image"** (enkel zichtbaar bij een geüpload beeld;
  `db::clear_item_image` zet `image=''` → terug naar kleur-thumb/bol).
- **Security**: alle nieuwe handlers checken `require_admin`; niet-admin POST → redirect `/info`,
  geen mutatie (getest).
- **Code**: `db.rs` — `Item` draagt nu `zone`+`shelf_id` (alle `row_to_item`-SELECTs bijgewerkt,
  incl. `gems_by_category`); nieuwe `move_item`/`set_item_shelf`/`clear_item_image`. `web.rs` — 3
  nieuwe routes (`/admin/item/move`, `/admin/item/shelf`, `/admin/item/image/clear`) + handlers +
  structs (`ItemMove`/`ItemShelf`/`SavedQuery`), `admin_item` herschreven (neemt nu `shelves`+`saved`),
  `admin_item_update` redirect met `?saved`, CSS voor `.savedflash`/`.save`/`.arow`/`.mvshelf` (kaart
  152px→168px). `.prow`-CSS blijft ongebruikt achter (onschadelijk).

## ✅ Laatste sessie (2026-07-13 nacht) — site-UI-overhaul, LIVE
Puur front-end/UX-werk in `web.rs` (self-contained binary; templates/CSS zitten via
`include_str!` erin). Alles **gebouwd + gedeployed** (`./deploy/deploy.sh`, service `active`) en
**gecommit + subtree-gepusht** naar `market-gh`.

- **Twee-kader-layout** — de nav zit nu in een **eigen afgeronde kaart** bóven de content-kaart,
  met ruimte ertussen (`shell()`: aparte `.navcard` + `.content{row-gap}`). Login/info/rules
  (geen nav) blijven één kaart.
- **Inventory = landingpagina, herschikt** — content-kaart begint met de **naam groot +
  gecentreerd** (`.bigname`), daaronder de **Coins/Gems/Boosts**-subtabs als groep gecentreerd
  (`.subtabs.center`), dan de panels.
- **Naam weg uit de navbar** — `chrome()` rendert de `.uname` niet meer; de naam staat enkel nog
  groot op de Inventory-pagina. (`.uname`-CSS blijft ongebruikt achter, onschadelijk.)
- **Admin-nav geconsolideerd** — de vier losse admin-knoppen → één **⚙ Manage**-knop. De
  Manage-pagina's krijgen een **sub-tabbalk** (helper `admin_subtabs`): 🛒 Shop · 🪙 Coins ·
  📋 Channels · 📜 Log (link-tabs naar de bestaande routes; alle admin-pagina's dragen nav-key
  `"admin"`).
- **Uitleg-teksten opgeschoond** (site moet intuïtief zijn) — weg: shop "Hytale server passes…",
  boosts-blurbs (→ korte lege-staat "No Hytale pass yet."), leaderboard "Ranked by…"-hint (+ JS),
  login-uitleg, rules-2e-zin, en de blurbs boven Manage Shop/Log/Channels. **Behouden**:
  veldlabels, foutmeldingen, lege-staten en de /info-gids.
- **Bugfix Manage Shop-thumb** — de 24u-pas (`/img/ticket.png`) had geen begrenzende CSS in
  `.aitem .thumb` (bestond enkel voor `.slot .thumb img`) → stond te groot. Toegevoegd:
  `.aitem .thumb img{max-width/height:100%;object-fit:contain}` + `overflow:hidden`.
- **Scroll behouden bij CRUD** — nieuwe const `KEEP_SCROLL_JS` op de Manage Shop: bewaart
  `scrollY` in `sessionStorage` (per pad) vóór elke form-submit en herstelt na de POST→redirect,
  zodat delete/update/upload/add niet naar de top springen.
- **Shop rendert nu dynamisch alle schappen** (i.p.v. hardcoded "🎟 Hytale passes" + enkel
  `boost`-items): `market()` loopt over `db::list_shelves()` → schap-titel + `shop_slot` per item,
  lege schappen overgeslagen. Toont nu **Hytale Access / Primary / Secondary / Prism Gems /
  Boosters** precies zoals Manage. ⚠️ Gems zonder afbeelding renderen als gekleurde bolletjes —
  graphics zijn de reden dat ze eerder verborgen waren; shop is nog **niet zichtbaar voor members**
  (site-gate), dus veilig om verder te polijsten.
- **Shop-itemkaders verbreed** (`66ce175`): shop-shelves dragen nu een `.shelf.shop`-modifier —
  kaarten **136px→180px** en naam `white-space:nowrap`+ellipsis, zodat "Hytale Permanent Pass" op
  **één regel** past i.p.v. te wrappen. De gems-shelf op de Inventory-pagina blijft 136px.

## ✅ Sessie (2026-07-13 avond) — server-log + chest-fix, LIVE
- **Pro server-log op de website (admin-only)** — GEBOUWD + **GEDEPLOYED** (draait op prod,
  PID 1321146, `/admin/log` → 303 oningelogd, healthz 200). Generieke tabel `server_log`
  (`category`/`event`/`actor`/`channel_id`/`ref_id`/`amount`/`detail`, idempotent aangemaakt in
  `init_pool`) + `/admin/log`-pagina (nav-tab **📜 Log**) met categorie-filterknoppen (nu enkel
  `chest`; raamwerk uitbreidbaar), gekleurde event-badges, lokale tijd (JS), laatste 500 nieuwste-eerst.
  - **Chest-events gepersisteerd**, gegroepeerd per chest via `ref_id` = bericht-id: `spawn`
    (detail = wie hem uitlokte), `join` (met volgnummer), `already_in`, **`too_late`** (klik nadat
    de chest al gepopt was), `win` (winnaar + prijs + deelnemerslijst), `despawn` (met lijst).
  - **Diagnose "3 deden mee, maar 2 bij opening":** klikken werden vroeger nergens gelogd → niet te
    bewijzen achteraf (enkel winnaar+aantal stonden in `journalctl`). Meest waarschijnlijke oorzaak:
    de knop bleef **live tot het chest-bericht gewist was** → een klik op/na het pop-moment viel in
    het gaatje tussen "chest uit de map" en "bericht gewist" en werd stil als `too_late` gedropt.
  - **Harde fix meegeleverd:** `pop_chest` verwijdert nu **éérst de knop** (message-edit met lege
    components) terwijl de chest nog in de map zit → klikken-in-transit tellen nog mee; pas daarna
    trekken/wissen we. Pop-race-gaatje dicht. (`too_late` kan nu enkel nog bij een echt zombie/oud
    bericht — óók zichtbaar in de log.)
  - **Code:** `db.rs` (tabel + `LogEntry`/`log_event`/`LogRow`/`recent_log`/`log_categories`),
    `bot.rs` (`recent` draagt nu ook de naam; `do_spawn_chest` neemt `triggers` + geeft `msg_id`
    terug; logging in `maybe_spawn_chest`/`handle_chest_click`/`pop_chest`), `web.rs` (`admin_log`
    + route + nav-tab). *(Intussen wél gecommit — commit `515cb9c`.)*

## 📝 Open TODO's
> Nagekeken tegen code + prod-DB op **2026-07-15**; wat af was is hier weggehaald (zie
> "✅ Afgevinkt" onderaan deze lijst). Niets op deze lijst is nog dringend.

- **⚠️ Prod-shopwaarden staan nog op TEST** *(hoogste prioriteit van deze lijst)*: op de **prod-DB**
  heeft de **Hytale Day Pass** `duration = 60` **seconden** (!) en `price = 10`; de **Permanent
  Pass** `price = 10`. Day Pass moet naar **86400s (24u)** + een echte prijs. **Niet acuut** — de
  shop is gate-d (enkel admins raken in `/market`) — maar **moet rechtstaan vóór de gate opengaat**.
  Zetbaar via **Manage Shop**.
- **Permanent Pass `role_id` is leeg** op prod → `Use` zet enkel `perma_access`, kent **geen
  Discord-rol** toe. Invullen via Manage Shop.
- **Shop-graphics**: de shop toont nu álle schappen; gems/boosters zonder afbeelding renderen als
  gekleurde bol. Echte item-graphics maken vóór de shop **members-zichtbaar** wordt (site-gate weg).
- ~~**Prijzen/economie balanceren**~~ → **AFGEHANDELD op 2026-07-17** (user): het ⚙ Settings-panel
  werkt en we sturen live bij indien nodig. De coin-instroom ging weliswaar +53% t.o.v. de oude
  prijs-ijking, maar dat is nu een **live tuning-kwestie** (gewicht +4/+5 of msg-cooldown via
  ⚙ Settings), geen openstaand bouwwerk meer. Gem-prijzen 1000–11000 (2026-07-16), Lucky Horseshoe 120.
- **Lucky Horseshoe — waarschijnlijkheid instellen**: de kans/sterkte van het horseshoe-effect
  (`chest_luck`, verdubbelt de chest-lot-kans) afstembaar/juist zetten. *(todo 2026-07-17)*
- **Lucky Horseshoe — testen**: het effect end-to-end verifiëren (koop → Use → chest). *(todo 2026-07-17)*
- **Gem-naamkleur**: naam van het lid in het **juiste font** tonen bij de achtergrond-instelling
  via een gem (swatch-preview). Cosmetische verfijning.
- **Admin klik op naam** in /admin/coins → toon de **coin-pagina van díe specifieke user**.
- **(WIP) Birthday-present**: registreer verjaardag → claim een cadeau (staat als "WIP" op /info).
- **Faybelle's oude −270** zit enkel in `coins`, niet in `total_earned` (van vóór de checkbox-fix) —
  evt. gelijktrekken via Set met enkel "all time" aangevinkt.
- **Public-profiel**: `coins.is_public` bestaat in de DB maar wordt nergens gelezen (leaderboard
  toont iedereen); ooit een profielpagina met public-filter.

### ✅ Afgevinkt op 2026-07-15 (stonden hier ten onrechte nog open)
- ~~Lucky Horseshoe heeft nog geen effect~~ → **LIVE** sinds `c82b14e` (`chest_luck`, dubbele
  chest-lot-kans).
- ~~Prod-guild: alles draait nog op de dev-guild~~ → achterhaald door de **go-live van 2026-07-13**.
- ~~Losse asset `static/MeadowShard.png`~~ → **bestand bestaat niet meer**.
- ~~Weekly leaderboard bouwen~~ → **bestaat dubbel**: het zaterdag-embed in de bot
  (`weekly_leaderboard`) **én** de "This week"-subtab op de site (`web.rs`, `leaderboard_week`).

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
  ⚠️ **ACHTERHAALD sinds 2026-07-17**: de const bestaat niet meer, de verdeling staat in de
  **`coin_weights`-tabel** (nu +0/+1/+2/+3 gelijk · +4 halve · +5 tiende) en is instelbaar via
  **⚙ Settings**. Zie sessie 2026-07-17b bovenaan.
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
  - ⚠️ **ACHTERHAALD**: het 10-tier-voorstel wérd de live verdeling op **2026-07-14**, en sinds
    **2026-07-17** staan beide consts er niet meer — de verdeling zit in de **`chest_tiers`-tabel**
    (instelbaar via ⚙ Settings; de 10 tiers zijn ongewijzigd overgenomen als seed).
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
  uploaden** · verwijderen. Subtabs (`admin_subtabs`, in volgorde): **🛒 Shop · 🛍 Admin shop
  items · 👁 Admin shop preview · 👥 Accounts · 🪙 Coins · 📋 Channels · ⚙ Settings · 📜 Log ·
  🖥 Server**.
- **⚙ Settings (`/admin/settings`)** — de economie-parameters + **beide weegsystemen**
  (coins-per-bericht, chest-tiers). Bot én site lezen deze **live** → wijzigen werkt meteen,
  zonder deploy of herstart. Velden komen uit `settings::SPECS`. Zie sessie **2026-07-17b**.

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
> Deze lijst dubbelde met **📝 Open TODO's** hierboven en was op 4 punten achterhaald; op
> **2026-07-15** samengevoegd. **Openstaand werk staat nu enkel bij 📝 Open TODO's.**
> Wat hier overblijft zijn feiten die je nog nodig hebt:

- **Tale-integratie**: ✅ LIVE sinds 2026-07-12 (namiddag). Market schrijft grants in
  `hytale_whitelist`; de tale-bot reconcilet elke **1 min** read-only naar `whitelist.json`
  (`whitelist.json` = `enabled:true`, wordt afgedwongen).
- **Weekly leaderboard**: ✅ gebouwd (stond hier lang als "voor later"). Twee stukken:
  `weekly_leaderboard` in `bot.rs` post het embed in prod #general, en de **"This week"**-subtab
  op de site leest `db::leaderboard_week`. Venster = sinds de vorige zaterdag; `earn_log` wordt
  **~8 dagen** bewaard (`prune_earn_log`) net om dit te voeden.
  ⚠️ **Tijdstip**: deze handover zei jarenlang "zaterdag **16:00**", maar de code doet
  **15:00 Brusselse tijd** (`next_saturday_1500_brussels`). De code is de waarheid; als 16:00 de
  bedoeling was, is dát een openstaande fix.

## Zo pik je het op
1. `cd lab/market`, `MARKET_WEB_ONLY=1 DISCORD_ROLE_ID=1525249217897955590 cargo run`, open
   `http://localhost:8700`, log in met Discord (redirect-URI localhost staat geregistreerd).
2. Wijzig → `cargo build --release` → `./deploy/deploy.sh` → commit → `git subtree push`.
3. Economy-ontwerp/achtergrond: `docs/economy-design.md`. Volledige geschiedenis in de git-log
   en in de projectmemory (`market-project`).
