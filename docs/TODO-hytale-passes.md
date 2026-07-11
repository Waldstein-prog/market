# TODO — Hytale-passen = echte whitelist (i.p.v. rol)  (spec 2026-07-11)

## Wat verandert (market)
De aankoop/gebruik van een Hytale-ticket geeft **geen Discord-rol meer**; in plaats daarvan
wordt de speler **gewhitelist op de Hytale-server**. Omdat de **Hytale-naam ≠ Discord-naam**,
moet de koper zijn **Hytale-naam kunnen intypen**.

1. **Hytale-naam bij aankoop/gebruik** — koper geeft zijn Hytale-naam op (validatie
   `^[A-Za-z0-9_]{1,32}$`). Bewaren bij de speler (bv. `coins.hytale_name`, wijzigbaar).
2. **Dagpas → 24u whitelist. Permanente pas → permanente whitelist.**
3. **Meerdere dagpassen koopbaar** (nu blokkeert het model 1×/perma). Gekochte dagpassen
   komen in de **inventory** als **verbruikbaar** item met een **Use**-knop.
4. **Use** = start de 24u-periode + whitelist; de gebruikte dagpas **verdwijnt**. Koop+Use
   van een volgende dagpas terwijl er nog tijd loopt = **+24u bovenop de resterende tijd**
   (stapelt, reset niet).
5. **Shop inperken**: voorlopig **enkel de Hytale-passen** tonen (gems/boosters verbergen tot
   er deftige graphics zijn). Later terug toevoegen.

## BESLIST (2026-07-11): de Discord-rol valt volledig weg
Whitelisten hangt **niet langer af van een Discord-rol**. Alle rol-gebaseerde code eruit:
- **market**: tickets kennen géén `role_id`/Hytaler-rol meer toe; `role_grants` + rol-sweeper
  vervallen voor de passen (rol-mechaniek mag blijven bestaan voor gems/kleur indien nog nodig,
  maar niet voor Hytale-toegang).
- **tale-bot** (`bot/bot.py`): `on_member_update` (rol → `whitelist add/remove`) en de
  rol-gebaseerde `/sync_whitelist` als toegangsbron **verwijderen**. Whitelisten loopt voortaan
  puur via `hytale_users.pass_expires` (het pas/timer-mechanisme dat er al is).

## BESLIST (2026-07-11): whitelist/timer = tale-bot, market voedt het
De tale-bot heeft de tabel `hytale_users(hytale_name, pass_expires)`, de **24u-stapel-logica**
(`base = expires if expires>now else now; new = base + 24u`) én het **FIFO-whitelisten** al.
Die timer/stapeling NIET dupliceren in market. **market voedt de grant in `hytale_users`;
de tale-bot whitelistet + bewaakt de timer + veegt weg bij verloop.** Enige nieuwe koppeling in
de bot: pas-verloop → `whitelist add/remove` (enforcement-loop, nu nog rol-gedreven).

## (achtergrond) De bepalende designvraag — hoe whitelist market écht?
market draait als user `market`; de Hytale-whitelist (whitelist.json + console-FIFO) hoort bij
user `hytale`. Cross-user. **Aanbevolen (laagste risico, hergebruikt bestaande tale-bot):**

- **market = enige bron van waarheid.** Bij Use schrijft market in `coins.db` een tabel
  `hytale_whitelist(hytale_name, expires_epoch NULL=permanent, user_id)`; stapelen = `expires`
  ophogen. Geen rol meer.
- **tale-bot = actuator.** De bestaande sweeper leest `/opt/market/coins.db` **read-only**
  (coins.db groeps-leesbaar zetten — zelfde perms-stap als de secrets-bundle) en verzoent de
  Hytale-whitelist via de FIFO: namen met een geldige/permanente grant → `/whitelist add`,
  verlopen → `/whitelist remove`. Reuse van `hytale_users`-mechaniek in `tale/bot/bot.py`.
- Voordeel: één economy-source, read-only cross-user, geen nieuwe service, geen rol-gedoe.

(Alternatief besproken: HTTP-endpoint of spool-bestand — meer bewegende delen, verworpen tenzij
gewenst.)

## Raakvlakken in de code
- `market/src/db.rs` — `seed_hytale` (2 tickets, `category='boost'`), `try_buy` (regel ~679:
  blokkeert dagpas bij perma; regel ~680 dagpas-duplicaat-regel), `use`-pad, `perma_access`.
- `market/src/web.rs` — `use_boost` (regel ~61 route), `boost_slot` (regel ~491), shop-render
  (`/market`, daily_shop), inventory Boosts-tab.
- `market/src/bot.rs` — sweeper (regel ~256) trekt nu rollen in; wordt/blijft óf reconcilet
  whitelist. NB: de whitelist-reconcile hoort in de **tale-bot** (Python), niet de market-bot.

## tale-side TODO (apart, in tale-project)
- **Welkom-bericht in-game**: als een speler joint die een (dag)pas kocht, toon een
  verwelkoming + **resterende uren/minuten** van de pas. (Plugin/bot leest de expiry uit de
  gedeelde bron; formatteer `Xu Ym`.)
- **Whitelist-reconcile** in `tale/bot/bot.py`: lees market's `coins.db` `hytale_whitelist`
  read-only, sync naar de Hytale-whitelist via FIFO (add/remove op basis van expiry).

## Volgorde bij implementatie
1. market DB+web: hytale_name-veld, meerdere dagpassen in inventory, Use=+24u-stapelen,
   `hytale_whitelist`-tabel, shop→enkel passen.
2. coins.db groeps-leesbaar (perms) — valt samen met de geparkeerde market-secrets-perms.
3. tale-bot whitelist-reconcile + in-game welkom met resttijd.
4. build (`cargo build --release`) → `market/deploy/deploy.sh` → test op dev-guild.
