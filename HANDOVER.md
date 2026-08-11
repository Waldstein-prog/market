# Handover — Meadow Market (2026-08-11)

## ⏭️ Sessie (2026-08-11c) — testfase: enkel een lijst van leden mag een Hytale-pas kopen

**Waarom.** Jo: Faybelle moet tijdens de testfase van de server kunnen aanduiden wie er
een pas mag kopen; al de rest ziet de pas **permanent op Out of Stock**.

**Wat er nu is.**
- **Nieuwe tabel `pass_allow`** (uid + naam + moment van toevoegen) — een lijst, dus een
  tabel en geen instelling, net zoals `coin_weights`/`chest_tiers`.
- **Manage → ⚙ Settings** heeft onderaan het blok *"Hytale-passen — wie mag er kopen"*:
  een tabel met de testers (✕ om iemand eraf te halen) en een keuzelijst met de leden die
  er nog niet op staan (＋ Tester). Toevoegen/verwijderen werkt meteen, niet pas bij
  "Opslaan" — dezelfde vorm als de twee weegsystemen eronder.
- **Schakelaar `pass_allowlist_on`** (groep "Hytale-passen — testfase", **default AAN**).
  Nodig omdat een lege lijst anders even goed "iedereen" als "niemand" kan betekenen; nu
  staat het er expliciet. Uit = de pas staat gewoon voor iedereen te koop.
- **De poort zelf** (`web::may_buy_pass`) hangt op twee plekken: de kaart in de shop toont
  **Out of Stock** (en verzwijgt het voorraadgetal — "3 left" naast een dichte knop is
  tegenstrijdig), en `buy()` weigert de POST met **exact dezelfde zin** als een uitverkocht
  item. Bewust dezelfde zin: voor wie er niet op staat *is* de pas dicht, en een aparte
  tekst zou enkel verklappen dat er een lijst bestaat. Geen nieuwe speler-zichtbare tekst.
- **Admins krijgen géén uitzondering.** Faybelle ziet op de shop dus letterlijk wat een lid
  ziet; wil ze zelf kopen, dan zet ze zichzelf op de lijst.
- **Geldt voor alle pas-items** (`category = 'boost'`), niet enkel de 6u-pas.
- **Twitch-redeems staan hier volledig los van**: die geven hun tijd zoals voorheen. De
  lijst gaat enkel over kopen in de shop.
- Elke wijziging logt als `admin/pass_allow` ("🎟 testers") met naam + uid.

**Verificatie.** 65 tests groen (nieuw: 4 — toevoegen/dubbel/lege uid/verwijderen, naam die
`coins` volgt met de bewaarde naam als terugval, de poort in beide standen van de
schakelaar, en de kaart die Out of Stock toont zonder voorraadregel). Lokaal end-to-end op
8701 tegen een kopie van de DB: lege lijst + schakelaar aan → 2 pas-kaarten dicht, 4 gewone
items koopbaar; tester toegevoegd → alles koopbaar; schakelaar uit → alles koopbaar,
ongeacht de lijst; niet-admin die post verandert niets; dubbel toevoegen en een lege keuze
schrijven geen rij. Backup vóór de deploy: `/opt/market/coins.db.bak-20260811-passallow`.
Gedeployd 23:11, site 200, geen warnings; `pass_allow` staat er (leeg) en
`pass_allowlist_on` is niet bewaard ⇒ de default (aan) geldt.

> ⚠️ **Startstand op prod (bewust, door Jo gekozen): testfase AAN met een lege lijst.**
> Er kan dus op dit moment **niemand** een pas kopen tot Faybelle testers toevoegt. De
> Settings-pagina zegt dat ook met een rode banner boven de tabel.

### ➕ Vervolg dezelfde sessie — aparte testklok, en het koopslot

Jo's aanvullingen: (a) de testfase krijgt een **eigen timer** — koop je 15 min testtijd, dan
loopt díe af en word je gekickt, terwijl je gewone tegoed **onaangeroerd stil blijft staan**;
(b) zet Faybelle de pas op Out of stock, dan zien testers dat óók; (c) zolang je testpas
loopt kan je er geen tweede kopen; (d) de lijst hoort naast het vinkje.

**Wat er aan market-kant bij kwam** (LIVE, deploy 23:59, site 200):
- **(b) was al zo** en is nu vastgelegd met een test: `sold_out` en voorraad 0 winnen van de
  testerslijst — die opent enkel wat verder open staat.
- **(d)** de groep "Hytale-passen — testfase" staat nu **achteraan** in `SPECS`, zodat het
  vinkje en het lijst-blok onder elkaar staan (enkel de Opslaan-knop ertussen; het blok heeft
  eigen formuliertjes en kan dus niet ín dat formulier).
- **(c)** `may_buy_pass` weigert een pas zolang er **testtijd loopt** op de Hytale-naam van de
  koper. Dat komt van de tale-kant: `passes.json` krijgt per speler `"kind": "test"|"normal"`,
  en `pass_ledger.rs` leest dat mee (`Ledger::test`). **Nee bij twijfel** — geen naam, geen
  gegevens, of een tale-bot die het veld nog niet schrijft ⇒ geen slot. De kaart toont dan
  gewoon Out of Stock; opnieuw geen nieuwe speler-zichtbare tekst.
- **(a) zit volledig aan tale-kant** — zie `tale/HANDOVER.md`. Kort: `pass_ledger` heeft twee
  potjes (`granted/used_normal` en `test_granted/test_used`); tijdens de testfase gaat álle
  nieuwe tijd én al het verbruik naar het testpotje. **De testfase-vlag is market's
  `settings.pass_allowlist_on`**, die de bot read-only meeleest — sleutel afwezig = aan, net
  als hier. Eén schakelaar voor beide kanten, geen tweede waarheid.

**Verificatie:** 67 tests (nieuw: `kind` lezen incl. ontbrekend veld = geen slot, en out of
stock wint van de lijst). Lokaal e2e met een nagemaakte `passes.json`: tester met 600s
testtijd → pas dicht; testtijd op → pas open; `kind: normal` → geen slot.

> ⚠️ **De tale-kant is bewust NIET gedeployd** (Jo test eerst op zijn gewone tijd). Zolang dat
> zo is, schrijft de bot geen `kind` en is het koopslot inert — market gedraagt zich exact
> zoals hierboven beschreven, met enkel de testerslijst als rem.

**Niet lokaal te bewijzen:** de weigering in `buy()` zelf — die route eist een échte
Flowerborn-rolcheck bij Discord, en een lokale instance heeft geen geldig bot-token. De
poort die ze gebruikt (`may_buy_pass`) is wel getest, in beide standen.

### 📌 Eindstand van de sessie — open punten voor de volgende

**Waar het staat.** Market: `cf5d703` gedeployd (23:59) en gepusht (subtree `7c93d08`);
vóór de eerste deploy een backup `/opt/market/coins.db.bak-20260811-passallow`.
Tale: `3b96acc` **gecommit maar niet gepusht en niet gedeployd**.

1. **De tale-bot moet nog live** — dát is de enige stap die de testfase écht doet werken.
   Recept: `bot/bot.py` naar `/opt/hytale/bot/` kopiëren + `systemctl restart hytale-bot`
   (raakt de gameserver niet; de regel "geen restart met spelers online" gaat over
   `hytale-server`, niet over de bot). Jo wilde eerst zelf op zijn **gewone** tijd testen —
   **vraag hem of het intussen mag.** Zolang dit wacht, is het koopslot in market inert.
2. **Wat er gebeurt op het moment van die deploy** (met de schakelaar aan): iedereen die
   enkel gewóne tijd heeft, valt van de whitelist en wordt gekickt zodra hij binnen is —
   zijn tegoed blijft volledig staan. Op prod gaat dat vandaag over easycomes (≈1u23),
   Heiji_Cat (≈3u10) en Waldstein (≈6u34). Faybelle is beschermd (`protected_names`).
3. **De testerslijst op prod is leeg.** Zolang dat zo is kan niemand een pas kopen — ook
   Jo en Faybelle niet (admins hebben bewust geen uitzondering). Zetten in
   Manage → ⚙ Settings.
4. **Item 22 `Hytale Test Pass`** (900 s = 15 min) staat nog op **Out of stock** met
   plaatshouder-prijs 100. Faybelle beslist prijs/duur/omschrijving en zet hem open; zolang
   hij dicht staat helpt de testerslijst niemand vooruit.
5. **Onbeslist gelaten, want het viel niet uit de vraag af te leiden:** een Twitch-redeem
   tijdens de testfase levert nu óók **testtijd** op. Reden: wie binnen is moet van een
   lopende klok eten, anders speelt hij gratis. Wil Jo dat redeems gewone tijd blijven
   geven, dan is dat één regel in `observe_grants` — maar dan kan die speler tijdens de
   testfase niet binnen.
6. **De server stond op slot** (`/opt/hytale/access-lock.json`, enkel Faybelle) — met dat
   slot aan komt er sowieso niemand binnen, ook niet met testtijd.
7. Nog altijd open van eerder: **vier redeems manueel terug te betalen** in Twitch
   (easycomes55 ×3, heijicat ×1, van de bug van 04/08), en de bevestigingszin na een
   aankoop rekent nog met wandkloktijd i.p.v. speeltijd (tekstkeuze is aan Jo).

## ⏭️ Sessie (2026-08-11b) — een verkeerde Hytale-naam is nu recht te zetten

**Waarom.** Sinds de vorige sessie ligt de Hytale-naam vast zodra er ergens speeltijd op
staat — precies om te verhinderen dat er een tweede naam naast ontstaat en de klok splitst.
Daardoor was er ook geen enkele weg meer terug: typte een kijker zich mis bij zijn eerste
redeem, dan speelde hij voorgoed onder die typo, en enkel handwerk in `coins.db` op de VPS
kon dat nog rechtzetten.

**Wat er nu is.** Manage → Accounts heeft een kolom **Hytale-naam** met een invulveld per
account (`POST /admin/accounts/name`, admin-gated). De correctie loopt via
`db::correct_hytale_name` en verzet in **één transactie** alle plekken waar die naam staat:
`coins.hytale_name` én elke grant-rij, aan **beide kanten** van de Twitch↔Discord-koppeling
(`twitch:<id>` ⇄ Discord-uid, via `coins.twitch_id`). Bleef er één achter, dan landde de
eerstvolgende aankoop of redeem alsnog op de oude naam. Zonder koppeling blijft de correctie
bij het gekozen account — twee vreemden zijn geen zelfde persoon. Een `twitch:`-pseudo-account
krijgt nooit een eigen `coins`-rij (enkel UPDATE, geen INSERT): dat zou een spookaccount in
het leaderboard zetten. Elke correctie schrijft `admin/hytale_name` in het logboek, met de
oude naam en de geraakte accounts erbij.

**Nevenfix.** `list_accounts` toonde de naam enkel uit de **grant**. Wie zijn naam op de site
zette maar nog niets kocht, kreeg dus een leeg vakje naast een vastgezette naam — misleidend
zodra je dat vakje kan bewerken. De query neemt nu `coins.hytale_name` als terugval.

**⚠️ Wat NIET meeverhuist:** speeltijd die aan tale-kant al onder de oude naam staat. Die
boekhouding is van de server (per naam in kleine letters); deze correctie stuurt enkel waar
**nieuwe** tijd landt. Een correctie ná opgebouwde tijd vraagt dus nog een ingreep aan
tale-kant. Dat staat ook als waarschuwing onder de tabel.

**Verificatie:** 61 tests groen (nieuw: beide kanten van de koppeling, "zonder koppeling blijft
het bij één account", en de naam-terugval in de accountlijst). Lokaal end-to-end gedraaid tegen
een kopie van de DB: gekoppeld paar met typo → één POST zet beide grant-rijen + de coins-rij
recht, ongeldige naam en onbekend account geven een foutbanner, niet-admin wordt weggestuurd.
Gedeployd 12:53, site 200, geen warnings in journalctl. Gecommit `f72fae1`, subtree → `12eb742`.

### 📌 Eindstand van de sessie — open punten voor de volgende

1. **Het naam-model is nu af**, en het is precies wat Jo beschreef: bij een Twitch-redeem
   (a) ligt de naam van dat Twitch-account vast vanaf de eerste keer, (b) moet een volgende
   redeem diezelfde naam typen — anders geen tijd, een whisper en een `twitch/name_mismatch`,
   (c) landt de tijd op die naam, en telt tale alle grants van dezelfde naam bij elkaar op.
   De Twitch↔Discord-koppeling (`coins.twitch_id`) dient enkel om de teller in de juiste
   Discord-inventaris te tonen. **Enige weg om een naam te wijzigen: Manage → Accounts.**
2. **Onoplosbaar en bewust zo gelaten:** dezelfde persoon met **ongekoppelde** accounts die
   op Twitch een andere naam typt dan op de site. Market kan niet weten dat het één iemand is.
3. **Vier redeems staan nog open om manueel terug te betalen** in de Twitch-wachtrij (van de
   bug van 04/08): easycomes55 (12:28, 12:41, 12:46) en heijicat (13:11). Beiden geraakten
   nadien wél binnen via een aankoop van 500 coins op de site.
4. **Waldstein houdt zijn resterende testtegoed** (~6 u 34): user zei uitdrukkelijk neen op
   wissen (11/08). `protected_names` op de VPS staat op enkel `["Faybelle"]`.
5. **De server stond op 11/08 op slot** (`/opt/hytale/access-lock.json`: `locked: true`, enkel
   Faybelle). Met dat slot aan komt niemand binnen, ook niet met een geldige pas.
6. **Stale commentaar aan tale-kant** (niet aangeraakt — andere sessie): de docstring van
   `observe_grants` in `/opt/hytale/bot/bot.py` zegt nog dat er niets bijgeboekt wordt als het
   grootboek de naam al kent. De code doet dat wél, en dát is het gewenste gedrag.

## ⏭️ Sessie (2026-08-11) — één Hytale-naam per persoon, over beide pas-bronnen heen

**Waarom.** Tale telt speeltijd **per Hytale-naam** (`pass_ledger.name_lc`): alle grant-rijen
van dezelfde naam voeden één klok, dus een Twitch-redeem + een shop-aankoop stapelen vanzelf
op. Live bewijs: Waldstein heeft twee grant-rijen (`391337551543271433` + `twitch:497218221`)
en één klok van 25 238 s. **Twee verschillende namen** = twee klokken, en de tijd onder de
naam waarmee je niet inlogt is onbereikbaar. De naam moest dus vastliggen zodra er tijd op
staat — en dat gold nog niet over de bronnen heen.

**Wat er nu gebeurt** (enkel mogelijk bij een **gekoppeld** account — `coins.twitch_id` uit de
geverifieerde Discord-verbindingen; zonder koppeling valt niet te wéten dat het dezelfde
persoon is, en dan blijven het bewust twee vreemden):
1. **Bij de login** — is `coins.hytale_name` leeg en zette zijn Twitch-pas al een naam vast,
   dan wordt die overgenomen. Gevolg: de shop vraagt niet meer om een naam en er kan er geen
   tweede naast getypt worden. (`db::linked_twitch_name`)
2. **Bij de aankoop** — hetzelfde, als vangnet voor wie sinds de koppeling niet opnieuw
   inlogde. Een meegestuurde naam wordt dan genegeerd, net zoals dat al gold voor wie al een
   naam had.
3. **Bij een redeem** — de "geregistreerde naam" is voortaan de eigen Twitch-grant **of**, als
   die er niet is, de naam die het gekoppelde Discord-lid op de site gebruikt
   (`db::linked_discord_name`). Typt hij dan iets anders, dan geldt de bestaande
   mismatch-weg: **geen tijd**, de whisper die Faybelle daarvoor schreef, en een
   `twitch/name_mismatch`-regel. Geen nieuwe speler-zichtbare tekst nodig.

**Niet opgelost, en niet oplosbaar:** dezelfde persoon met **ongekoppelde** accounts die op
Twitch een andere naam typt dan op de site. Market kan niet weten dat het één iemand is. Enige
verweer blijft dat de whisper en de site dezelfde naam vragen.

**Randgeval dat blijft:** de naam later wijzigen verplaatst een bestaand tegoed niet — dat
blijft aan tale-kant onder de oude naam staan. Er is nu alleen geen weg meer om er per
ongeluk een tweede naam bij te maken.

**Verificatie:** 58 tests groen (nieuw: het slot in beide richtingen, inclusief "ongekoppeld =
geen slot" en "andermans Twitch-id levert niets op"). Prod-check vóór de deploy: geen enkel
lid heeft een lege `hytale_name` naast een Twitch-grant, dus de overname vuurt nergens
met terugwerkende kracht. Gedeployd 09:49, gecommit `bee64f4`, subtree → `f1048ef`.

### 📌 Eindstand van de sessie — open punten voor de volgende

1. **Vier redeems staan nog open om manueel terug te betalen** in de Twitch-wachtrij:
   easycomes55 (04/08 12:28, 12:41, 12:46) en heijicat (04/08 13:11). Beiden kochten daarna
   zelf een pas van 500 coins op de site, dus ze zijn wél binnengeraakt.
2. **Waldstein houdt zijn resterende testtegoed** (~6 u 34 van de 7 u): user zei uitdrukkelijk
   **neen** op wissen (11/08). Verder is hij een gewone speler — `protected_names` op de VPS
   staat al op enkel `["Faybelle"]` en er bestaat nergens nog een permanente pas.
3. **De server stond op 11/08 op slot** (`/opt/hytale/access-lock.json`: `locked: true`, enkel
   Faybelle). Niet door deze sessie gezet; met dat slot aan komt niemand binnen, ook niet met
   een geldige pas.
4. **Hoofdletters maken niets uit, de rest van de spelling wel.** De toegangslijst van de
   server bewaart UUID's; de naam dient enkel om het account op te zoeken, en die opzoeking is
   hoofdletter-ongevoelig (bewijs 04/08: `whitelist add easycomes` → speler `EasyComes` kwam
   binnen). Onze eigen lagen sleutelen allemaal op de naam in kleine letters, dus kapitaal kan
   een speeltijd-klok nooit splitsen.
5. **Stale commentaar aan tale-kant** (niet aangeraakt — andere sessie): de docstring van
   `observe_grants` in `/opt/hytale/bot/bot.py` zegt nog dat er niets bijgeboekt wordt als het
   grootboek de naam al kent. De code doet dat wél, en dát is het gewenste gedrag (een tweede
   bron moet zijn tijd krijgen). Enkel het commentaar is achtergebleven bij de omslag van 04/08.

# Handover — Meadow Market (2026-08-10)

## ⏭️ Sessie (2026-08-10) — Twitch-redeem matcht op reward-**id**, niet meer op titel

**Gedeployd** (market draait sinds 11:25) — nog **niet gecommit/gepusht**.

### De bug van 2026-08-04 (pas nu gevonden)
Faybelle hernoemde die ochtend haar rewards en zette er emoji's voor
(`Meadowland Pass` → `🎫Meadowland Pass`; om 07:06 stond de oude naam nog in de log, om
09:07 de nieuwe). `settings.twitch_reward_title` bleef `Meadowland Pass` en de vergelijking
was exact (op trim/kapitaal na), dus **elke pas-redeem viel in de "niet van ons"-tak**:
geen pas, geen whisper, punten weg, enkel een regel in journalctl.

| tijd | kijker | wat er gebeurde |
|---|---|---|
| 12:28 / 12:41 / 12:46 | easycomes55 | 3× `🎫Meadowland Pass` genegeerd |
| 13:11 | heijicat | genegeerd |

Beiden geraakten uiteindelijk binnen via de site: `shop pass_day` van **500 coins** (12:33 en
13:13), bot whitelistte binnen ~30 s. **Er staan dus nog 4 redeems open om manueel terug te
betalen in Twitch.** De naam-mismatch-weigeringen van 07:13/07:14 (`Flupke`,
`herr waldstein`) waren Waldsteins eigen test en werkten zoals bedoeld.

### De fix
- **`twitch_reward_title` → `twitch_reward_id`** (idem voor perma). Een reward-id verandert
  nooit, ook niet bij hernoemen. `pass_kind_for` vergelijkt sindsdien id's.
- **Nieuwe `Kind::Choice`** in `settings.rs`: opgeslagen als tekst (de id), in de GUI een
  **keuzelijst met de titels** van het kanaal. Nodig, want een reward-id staat nérgens in het
  Twitch-dashboard — die valt niet over te typen.
- **Reward-lijst-cache** in `kv["twitch_rewards"]`, door het Twitch-luik ververst bij de start
  en **elke 5 min**; de Settings-pagina tekent haar lijst daaruit (het web-luik heeft geen
  token). Mislukt het ophalen, dan blijft de vorige lijst staan en blijft de gekozen optie
  geselecteerd — **opslaan mag de keuze nooit stil wissen**.
- **Eenmalige overgang** `adopt_reward_ids`: koppelt de oude titel aan een reward — eerst
  letterlijk, anders op de titel herleid tot letters/cijfers (zo valt `🎫Meadowland Pass`
  samen met `Meadowland Pass`), en **enkel bij precies één kandidaat**. Marker
  `kv["twitch_reward_id_migrated"]`, want "niets gekozen" is een geldige keuze.
- **Vroege waarschuwing** voor de enige stille faalmodus die overblijft: staat de ingestelde
  id niet meer tussen de rewards (reward gewist en heraangemaakt = nieuwe id), dan zegt de
  startregel `⚠️ STAAT NIET MEER TUSSEN DE REWARDS VAN HET KANAAL` en logt de verversing een
  WARN. Het brede EventSub-abonnement blijft bewust breed: zo blijft élke genegeerde redeem
  zichtbaar in de log — dat was op 04/08 het enige spoor.

### Verificatie
- **56 tests groen.** Nieuw: id-routing, "hernoemen breekt de koppeling niet", de overgang
  tegen een echte DB (inclusief: één keer, en wachten tot er een lijst ís), het lezen van de
  Helix-lijst, en de **echte prod-rewardlijst** van faybelle___ als anker.
- **Prod na de deploy:** `twitch_reward_id overgenomen uit de oude titel-instelling —
  'Meadowland Pass' is nu '🎫Meadowland Pass' (430733ab-e1fa-40a0-98ce-706743696c3e)` en
  `Twitch-luik actief — reward='🎫Meadowland Pass' (430733ab…), pas=6u`. Cache 13 rewards,
  marker gezet, site 200. Backup vóór de deploy: `/opt/market/coins.db.bak-20260810-rewardid`.
- ⚠️ De oude rij `twitch_reward_title` blijft in `settings` staan (geen Spec meer, dus
  onzichtbaar en ongelezen). Niet wissen: het is het bewijsstuk van de overgang.

## Handover — Meadow Market (2026-08-04)

## ⏭️ Sessie (2026-08-04c, vanuit de tale-sessie) — passen zijn onbeperkt stapelbaar, permanente pas weg

**Gedeployd** (market draait sinds 09:07). Op vraag van Jo, en in één lijn met de omslag aan
serverkant: **toegang is een tegoed aan speeltijd**, dus "één pas tegelijk" sloeg nergens meer op.

**Wat er veranderd is:**
1. **`db::purchase`** — de twee blokkades op een pas met looptijd zijn weg: `"You already have an
   active pass."` en `"You already have permanent access."`. Elke aankoop schrijft dus tijd bij; een
   lid mag zoveel passen kopen als het wil en de uren stapelen.
   ⚠️ De eerste keek naar `expires`, en die waarde zegt sinds 2026-08-04 **niets meer** over wie er
   binnen mag — dat beslist de server op het tegoed.
2. **`shop_slot` (web.rs)** — `day_pass_active` weg (plus de parameter `has_pass`). De pas-kaart
   toont niet langer "Bought" zolang er een pas loopt, ze blijft gewoon koopbaar.
3. **Permanente pas geschrapt.** `buy()` weigert nu een pas-item zónder looptijd
   (`"This pass has no duration set."`) i.p.v. permanente toegang uit te delen; het
   `pass_perma`-logpad is weg. `set_perma_access`/`grant_perma_whitelist` blijven staan voor de
   aparte Twitch-perma-reward en admin-toekenningen. Op prod bestond geen enkele permanente grant en
   geen enkele `perma_access`-vlag, dus dit ging schoon.
4. **Item 22 (`Hytale Permanent Pass`) is hergebruikt als `Test Pass`** — nooit gekocht, dus geen
   historiek die breekt. Staat op **900 s (15 min), prijs 100, en `sold_out = 1`**: dat zijn
   plaatshouders, hij is bewust **dicht** tot Jo prijs, duur en beschrijving zelf zet in
   Manage → Shop. Meerdere pas-items naast elkaar mogen: de server leest bij elke aankoop de
   `duration` van het item dat écht gekocht werd (via `inventory`).

**⚠️ Contract met tale — gewijzigd, lees dit vóór je aan de passen raakt.** De tale-bot leidt het
tegoed **niet meer** af uit de grootte van de stijging van `expires` (dat gaf gratis uren na
bot-downtime). Nu geldt: **gaat `expires` omhoog, dan is er één pas bijgekomen**, en tale boekt de
duur die bij díe pas hoort — `settings.twitch_pass_hours` voor een redeem, `items.duration` van het
laatst gekochte pas-item voor de shop. Gevolg voor market: `expires` met exact de itemduur ophogen
per aankoop (zoals `grant_day_whitelist` doet) en er verder van afblijven. Eén klok per speler:
market houdt één rij per bron (Discord-id en `twitch:<id>`), tale telt ze samen op één tegoed.

**Nog open aan market-kant:** de bevestiging na een aankoop zegt
`"Whitelisted as X — N of access left."` en rekent dat uit `expires - nu`. Dat is wandkloktijd, geen
speeltijd — het klopt zolang iemand niet gespeeld heeft en overschat daarna. De echte stand staat in
`/opt/hytale/passes.json` (`granted - used`, wereld-leesbaar), maar die is pas ~15 s na de aankoop
bijgewerkt. **Tekstkeuze is aan Jo**, dus hier niets aan veranderd.

## ⏭️ Sessie (2026-08-04b) — Twitch-pas hoort nu bij een lid + weergave van de speeltijd

**Gedeployd + gecommit + gepusht** (`d1dcdd4`; subtree `market-gh` → `8e7c442`). Aanleiding:
Waldstein wisselde een Twitch-redeem in, geraakte op de server, maar zag géén pas-embleem op
zijn inventarispagina.

### Waarom dat embleem ontbrak
De grant staat onder `twitch:497218221`, de pagina zocht op zijn Discord-id. Geen match.
**Opgelost via de Discord-verbindingen**, niet via de Hytale-naam: de login vraagt nu ook de
OAuth-scope **`connections`** en bewaart het (door Discord **geverifieerde**) Twitch-account in
`coins.twitch_id`. Matchen op naam was de alternatieve weg en is bewust **verworpen** — de
kijker typt die naam zelf, dus wie "FayBelle" intikt zou een pas op háár pagina zetten.
Geen koppeling ⇒ geen embleem, want dan valt niet te wéten van wie die pas is.
> ⚠️ Bestaande leden zien de extra toestemming één keer bij hun **volgende login**; pas dán is
> `twitch_id` gevuld. Wie niet opnieuw inlogt, ziet zijn Twitch-pas niet.

### ⚠️ Botsing met de tale-sessie — en hoe die beslecht is
Halverwege bleek de tale-kant **dezelfde functie** te bouwen (`UsageStore.java`,
`test_speeltijd.py`) en al live te hebben. Ik had toen al een eigen boekhouding staan
(`presence.rs` las join/leave uit `chat_mirror.log` en pauzeerde `expires`). **Die is er weer
uit.** Reden is niet enkel dubbel werk: hun bot leidt het toegekende tegoed af uit de
**stijging** van market's `expires`, dus mijn keepalive-tik zou stilzwijgend **gratis uren**
hebben uitgedeeld.

**Afspraak (user):** tale beslist over whitelisting en speeltijd, **market doet verkoop +
weergave** en leest enkel af wat tale aanlevert. Gevolg in de code:
- `grant_day_whitelist` stapelt weer **exact** zoals vroeger — daar leest de bot op mee, dus
  daar mag niets aan veranderen.
- Nieuw **`pass_ledger.rs`** leest `/opt/hytale/passes.json` (v2:
  `{"granted","used","remaining"}` per Hytale-naam, `644`).
- **Online-zijn staat niet in dat bestand**, maar valt af te leiden: `used` loopt enkel op
  terwijl iemand speelt. Daarom bemonsteren we elke 20 s op de achtergrond — enkel bij een
  paginabezoek kijken zou betekenen dat wie nooit z'n inventaris opent, nooit als online geldt.
- Geen gegevens (onleesbaar bestand of onbekende naam) ⇒ **terugval** op de oude weergave.

### Weergave (user-beslissingen)
Tijd staat **onder** het logo i.p.v. erover; het logo draagt een **pauzeteken** zolang de klok
stilstaat. Bewust **géén tekstregel** die uitlegt waarom hij stilstaat — *"het pauzesymbool is
evident en de context"*. Speelt hij, dan loopt de afteller gewoon door.

### Toegang die daarvoor nodig was
`/opt/hytale` was `750 hytale:hytale`, dus market kon er niet bij. Toegevoegd:
`setfacl -m u:market:x /opt/hytale` — **enkel doorloop**, geen leesrecht op de map zelf. Op dat
niveau staan geen geheimen (`passes.json` is `644`; de bot-config met het Twitch-secret staat in
`/opt/hytale/bot/`, `600`). Terug te draaien met `setfacl -x u:market /opt/hytale`.

### Verificatie
- **45 tests groen.** Nieuw: de koppeling (naam alleen koppelt níét, andermans id evenmin) en
  het grootboek (formaat, hoofdletter-ongevoelig, kapot/afwezig bestand geeft niets).
- **Weergave met de echte binary** in drie toestanden nagekeken: offline → pauzeteken +
  stilstaande tijd; online (`used` steeg) → lopende afteller op `now + remaining`; naam niet in
  het grootboek → terugval op `expires`.
- **Prod na de deploy:** `Pas-grootboek: /opt/hytale/passes.json gelezen — 1 pas(sen)`,
  Twitch-luik actief (reward heet nu **'Meadowland Pass'**, pas **6u**), site 200.

### ✅ Eindstand van de sessie (alles bevestigd op prod)
- **Het embleem staat er** — user opnieuw ingelogd na de sessie-wis en de pas is zichtbaar.
  Twee koppelingen al binnen: `Waldstein → 497218221` en `FayBelle → 934674170`.
- **De volledige ketting is bewezen**, van begin tot eind: redeem → grant onder `twitch:<id>` →
  tale whitelistet → speeltijd loopt af (`used` 0 → 139 s tijdens de sessie) → embleem met de
  juiste tijd op de site → whisper bij de kijker.
- **De whispers werken** (user, 2026-08-04): het streamer-account heeft een geverifieerd
  telefoonnummer, dus de gevreesde 401 doet zich niet voor. Geldt voor alle drie de berichten
  (geslaagde pas, permanente pas, afwijkende naam).
- **De koppeling werkt**: na uitloggen + opnieuw aanmelden staat `Waldstein → twitch:497218221`
  in `coins.twitch_id`, precies de id waar de grant onder staat, en **het embleem verschijnt**.
- **De tale-teller loopt**: `used` ging van `0.0` naar `112 s` — de mod-kant van de speeltijd is
  sindsdien echt aan het werk. Hele ketting rond: redeem → grant → tale telt af → market toont.

### 🐛 Onderweg gefixt: login gaf *"failed to parse header value"* (`1448025`)
Het commentaar over de nieuwe scope stond **binnen** de string-literal van de autorisatie-URL;
door de `\`-regelvoortzetting werd die uitleg deel van de URL, en die gaat rechtstreeks in een
`Location`-header. Compileerde probleemloos — als string klopte het. De URL-bouw zit nu in
`authorize_url()` met een test die de vorm vastlegt én `HeaderValue::from_str` erop loslaat.
**Les:** commentaar hoort nooit in een `\`-voortgezette literal.

### ➕ Afwijkende naam bij een volgende redeem (`574ad69`, gedeployd)
De naam ligt na de eerste redeem vast op het Twitch-account. Tot nu werd afwijkende invoer
**stilzwijgend genegeerd** en ging de tijd naar de oude naam — de kijker betaalde dan punten
voor iets wat hij niet vroeg. Nu: **geen tijd**, een whisper, en een
`twitch/name_mismatch`-regel als signaal om manueel terug te betalen.
- `name_conflicts()` telt **hoofdletters en spaties eromheen niet** mee (dezelfde speler die
  zich anders intikt hoort niet gestraft te worden) en een **leeg** invoerveld evenmin — dan is
  er niets nieuws beweerd en blijft de vastgezette naam gelden.
- Tekst = **letterlijk van de user**, als startwaarde in de nieuwe setting
  `twitch_mismatch_whisper_text` (Manage → ⚙ Settings). `{naam}` = de vastgezette naam.
- Daarvoor kreeg `Spec` een **`text_default`**. Die geldt **enkel zolang de sleutel nooit
  bewaard is**: maakt Faybelle het veld leeg, dan blijft het leeg (bericht uit) i.p.v. terug te
  springen — anders viel zo'n bericht nooit meer af te zetten. De GUI leest via `str_of`, dus
  ze ziet de tekst meteen in het tekstvak staan.
- **48 tests.** Mock-e2e uitgebreid naar **vijf** paden; kijker 555 staat op een andere naam,
  blijft op zijn 1u staan en krijgt de mismatch-whisper.
  > ⚠️ Valkuil in die test: de **Twitch-CLI kan `user_input` niet zetten** (altijd
  > `"Test Input From CLI"`). Daarom dragen de kijkers die wél tijd moeten krijgen net die
  > tekst als vastgezette naam — anders zou élke redeem in de test een mismatch zijn.

### 🔑 Iedereen uitgelogd (user-beslissing, uitgevoerd 2026-08-04 07:2x)
Om de nieuwe scope bij ál de leden op te halen: **alle 21 sessierijen gewist** in prod
`coins.db` (backup vooraf; `sessions` is wegwerpdata — niemand raakt coins, inventaris of pas
kwijt; leden 25 en passen 6 stonden er ná de wis nog). De oudste sessie dateerde van 10 juli, dus
zonder deze ingreep had een deel van de leden het toestemmingsscherm pas in oktober gezien.
Iedereen moet nu één keer opnieuw "Sign in" doen en passeert daarbij langs
`scope=identify+connections` (na de wis geverifieerd op prod). Pas dán is hun `twitch_id`
gekoppeld en verschijnt een Twitch-pas op hun pagina.

### 📌 Open
1. De tale-commit `5f731cf` staat wél lokaal maar is **niet naar tale-gh gepusht** — dat is
   werk van de andere sessie, niet aan mij om te publiceren.

---

## ⏭️ Sessie (2026-08-04) — Twitch-luik STAAT LIVE op prod (was inert) + DNS-bug in de Helix-basis

**Gedeployd + gecommit + gepusht.** Het Twitch-luik draaide tot vandaag **niet** op prod bij
gebrek aan creds. Dat is nu opgelost en `twitch_ready()` is waar: market luistert live mee op de
channel-points-redemptions van **faybelle___**.

### De creds waren niet weg — ze stonden in het tale-luik
Zoekactie leverde ze op in `/opt/hytale/bot/config.toml` (`[twitch] enabled=false`), van de oude
Python-bridge uit juli. Zelfde app-registratie hergebruikt (`app_id`
`f70589odg5k0v76e1o0qrbzmbs8xw9`), gekopieerd naar `/opt/market/secrets.json` (backup
`.bak-20260804-044853`, 600, market:market). Geldigheid eerst bewezen met een
`client_credentials`-token vóór er iets herstart werd.

### 🐛 Bug: `HELIX` wees naar een hostnaam die niet bestaat
`const HELIX = "https://helix.twitch.tv"` — **die host resolvet niet**; de Helix-API woont op
`api.twitch.tv` (het `/helix` zit in het *pad*). Bij de eerste echte start dus meteen
`Twitch-luik start niet: helix netwerkfout` op `/helix/users`. **De mock-e2e kon dit nooit
vangen**: die wijst de basis naar loopback, dus alle 39 tests waren groen terwijl geen enkele
echte call ooit kon slagen. Gefixt + regressietest `helix_base_is_the_real_api_host` die de vier
echte endpoint-constanten vastzet (**40/40 groen**). Diagnose liep via DNS, niet via gokken:
`getent ahosts helix.twitch.tv` leeg, `id.twitch.tv` wél resolvend.

### OAuth: device flow i.p.v. de redirect
De browser-OAuth uit de doc gaf **`redirect_mismatch`** — `http://localhost:17563` staat niet in
de app-registratie. Niet gerepareerd maar omzeild: **device code flow**
(`POST /oauth2/device`), waarbij de streamer een code typt op `twitch.tv/activate`. Geen
redirect-URL, geen listener, werkt vanaf gelijk welk toestel — en dus ook bruikbaar als we enkel
via SSH werken. Token binnen, geverifieerd via `/oauth2/validate`: **faybelle___** (id
`934674170`), scopes `channel:read:redemptions` + `user:manage:whispers`. Weggeschreven als
`/opt/market/twitch_tokens.json` (600, market:market).

### Stand op prod na de deploy
```
Twitch-luik actief — kanaal=faybelle___, reward-titel='Meadowland Day Pass', perma-titel=(uit), pas=2u
Twitch EventSub: geabonneerd op alle reward-redemptions van het kanaal
```
Nagekeken via Helix + de settings-tabel, want dit zijn de twee stille faalmodi:
- Reward **'Meadowland Day Pass'**: `invoerveld=True`, kost **1**, ingeschakeld. ✅
- `twitch_whisper_text` **ingevuld** door Faybelle, mét serveradres `167.235.142.113:5520`.
  Gebruikt geen `{naam}`/`{uren}` — mag, de plaatshouders zijn optioneel.
- `twitch_pass_hours = 2`; perma-titel leeg ⇒ permanente redeem **uit**.

### ⏳ Wat nog niet bewezen is
1. **Een echte redeem** — nog niemand heeft de reward ingewisseld. Dát is de enige test die
   overblijft; alles ervoor is geverifieerd.
2. **De whisper** kan 401 geven als het streamer-account **geen geverifieerd telefoonnummer**
   heeft. Onbekend tot de eerste redeem. Gevolg is beperkt: de **pas wordt toch toegekend**,
   enkel het bericht met het serveradres ontbreekt. Zichtbaar in `journalctl -u market`.
3. **Kost = 1 channel point** — dat oogt als een test-instelling; als de reward publiek gaat,
   beslist Faybelle de echte prijs (in de Twitch-UI, market raakt de kost niet aan).
4. Het secret is in juli ooit in plaintext-chat geplakt. Roteren mag, maar dan moet de
   device-flow-stap opnieuw.

---

## ⏭️ Sessie (2026-08-03) — Twitch-redeem omgebouwd: streamer bezit de reward, whisper i.p.v. chat

**Gecommit + gepusht + gedeployd** (`ec0ba96`; subtree `market-gh` → `4e0902d`; deploy 12:03,
service active, site 200, schone log). Het Twitch-luik zélf draait op prod **niet**: prod
`secrets.json` heeft geen twitch-velden, dus `twitch_ready()` is false. De deploy zet vooral de
**nieuwe Settings-velden** klaar zodat Faybelle ze kan invullen. Drie user-beslissingen zijn de
kern:

1. **Faybelle maakt de reward zelf aan** in haar Twitch-dashboard (met invoerprompt voor de
   Hytale-naam). Market maakt/beheert **geen** rewards meer.
2. **Geen automatische terugbetaling** — manueel in de Twitch-wachtrij als het nodig is.
3. **De duur is instelbaar** op Manage → ⚙ Settings (test: **2 uur**).

### Wat dat technisch afdwingt
Helix laat een app enkel redemptions **fulfillen/annuleren** van rewards die ze **zélf**
aanmaakte (anders 403). Wie de reward bezit en wie kan terugbetalen is dus dezelfde vraag —
punt 1 en 2 zijn onlosmakelijk. Gevolgen in `twitch.rs`:
- **`ensure_reward` weg** (aanmaken + kost-syncen), **`set_redemption_status` weg**,
  **chat-bevestiging weg**.
- **Herkennen op TITEL i.p.v. reward-id.** We kennen de id van haar reward niet, dus het
  EventSub-abonnement is nu **breed** (`reward_id` in de condition is optioneel volgens de
  docs): álle redemptions van het kanaal komen binnen en `on_redeem` filtert op titel
  (getrimd, hoofdletter-ongevoelig). Bijvangst: een hernoemde of pas aangemaakte reward werkt
  **meteen**, zonder herstart. Elke andere beloning van het kanaal wordt genegeerd — mét
  logregel, want een titel die net niet klopt is anders onzichtbaar.
- **Ongeldige naam** → geen grant, geen refund, wél `twitch/rejected` in het logboek met
  "refund manually in Twitch". Dát is het signaal voor Faybelle.
- **Naam blijft vastgezet** op het Twitch-account bij de eerste redeem (user-beslissing tegen
  gesmoemel; foute namen ruimen jullie zelf op).

### Whisper i.p.v. chatbevestiging
De bevestiging gaat als **Twitch-DM (whisper)** naar de kijker — daar staat ook het
**serveradres** in, want zonder adres geraakt hij er niet op. `POST /helix/whispers`.
⚠️ **Twitch-eisen** (uit de docs, niet af te leiden uit onze code): scope
**`user:manage:whispers`** — die zit **niet** in het huidige token, dus de OAuth-stap moet
één keer opnieuw — en het **zendende account moet een geverifieerd telefoonnummer** hebben
(anders 401). Kijker die whispers van vreemden blokkeert → 403. Mislukt de whisper, dan is de
**toegang toch toegekend**; enkel het bericht ontbreekt.

### Nieuw: tekst-instellingen (`Kind::Text` in `settings.rs`)
De Settings-tab kende enkel getallen en vinkjes. Nu ook vrije tekst (`str_of`, opgeslagen
getrimd, geen grenzen; een sleutel op `_text` krijgt een `<textarea>`). Nieuwe groep
**"Twitch-redeem → Hytale-pas"** met 5 velden: reward-titel, duur in uren, whisper-tekst
(plaatshouders `{uren}`/`{naam}`), plus perma-titel en perma-whisper. **Leeg = uit** bij elk
tekstveld — inclusief de reward-titel, dus zolang die leeg is negeert market álle redeems.
De titels/duur/tekst zijn uit `secrets.json` **weg**: `config.rs` houdt enkel nog de geheimen.

> Speler-zichtbare tekst blijft van de user: de whisper-velden starten **leeg**, ik verzin er
> geen. Zolang Faybelle ze niet invult, krijgt een kijker wél zijn pas maar géén bericht.

### Verificatie
- **39/39 tests groen** (was 37): titel-routing (incl. hoofdletters, lege titels, "niet van
  ons"), template-invulling, en het kappen op 500 tekens op een **tekengrens** (geen kapot
  UTF-8).
- **Mock-e2e `docs/twitch_e2e.sh`** (vervangt `perma_e2e.sh`) — Twitch-CLI EventSub-mock,
  market op poort 8701 zodat het naast een draaiende market kan. Vier redemptions, alle vier
  bewezen: vreemde titel → genegeerd (geen rij), ongeldige naam → `rejected` + geen rij,
  geldige → **1u seed + 2u = 3,00u** (duur uit de settings!) + de whisper-tekst ingevuld,
  perma-titel → `expires = NULL`.
- **Settings-pagina** lokaal gerenderd en een echte opslag-ronde gedaan: multiline-tekst
  overleeft, `&`/`"`/`<` komen correct ge-escaped terug, spaties eromheen worden getrimd,
  leeg blijft leeg.

### 📌 Nodig vóór de live-test (allemaal user-/Faybelle-kant)
1. **Reward aanmaken** in Twitch mét "Require Viewer to Enter Text" **aan**.
2. **Nieuw OAuth-token** met `user:manage:whispers` (recept in `docs/twitch-setup.md`), en een
   **geverifieerd telefoonnummer** op het streamer-account.
3. **De vijf settings invullen** — vooral de reward-titel (letterlijk dezelfde) en de
   whisper-tekst **met het serveradres erin**.
4. `[twitch]`-creds in prod `secrets.json` + herstart.

---

## 📌 Openstaand na 2026-07-31 (kort)
1. **Faybelle test de shoprotatie** — gewichten naar smaak in Manage → Shop. De huidige
   getallen (gems 10, horseshoe 2) zijn enkel gekozen om de bestaande zeldzaamheid **niet** te
   veranderen; ze zijn géén balansoordeel. ⚠️ Een gewijzigd gewicht slaat pas aan bij de
   volgende rotatie (02:00 Brussel) of meteen via **↻ reroll** op Admin shop preview.
2. **Balansoordeel over de horseshoe zelf**: 2× lot-kans bij een chest voor 7777 coins, nu ook
   echt te koop als hij in de rotatie valt. Nooit beoordeeld — het oude testprotocol is
   afgerond t.e.m. het odds-bewijs, dít punt bleef over.
3. **Telefoonbevestiging uit sessie 07-29b** (los van vandaag): de naamkleur op de Gems-tab en
   de strook die niet meer terugspringt. Beide fixes staan live, enkel nooit op een echt
   toestel nagekeken.
4. Untracked in de map, niet van deze sessies: `artwork/Grannys_2.png`, `artwork/toeter.png`,
   `screenshots/vakjes passen.png` — user beslist of ze in git mogen.

---

## ⏭️ Sessie (2026-07-31b) — Chest-tellers op de Coins-tab

**LIVE op prod + gecommit + gepusht** (`1fe2c5c`; subtree `market-gh` → `1575cc4`). Deploy om
10:02, site 200, geen fouten in het log. Geen schemawijziging, dus geen backup nodig.
Op de inventory-pagina, Coins-tab, staan onder *Coins earned all-time* nu twee regels:
**Chests opened** en **Chests won** (user-verzoek, exacte woorden).

- Bron = het **logboek**, geen nieuwe telling: `db::chest_counts(pool, uid)` telt in één query
  de `chest/join`- en `chest/win`-regels van dat lid. Werkt dus met terugwerkende kracht vanaf
  de eerste chest ooit, en er valt niets uit de pas te lopen.
- **Opened = enkel een échte deelname.** Een tweede klik logt `already_in`, een klik op een
  verdwenen chest `too_late` — die tellen niet mee. Een chest waaraan je meedeed en die daarna
  despawnde (te weinig klikkers) telt **wél**: geopend, maar hij ging niet open. Op prod maakt
  dat verschil vandaag nul (alle 19 chests tot nu toe zijn ook echt opengegaan). Wil de user
  later enkel uitbetaalde chests tellen, dan is dat een join ⋈ win op `ref_id`.
- **Won** = de `chest/win`-regels, inclusief die van een `!chestrescue`.
- Weergave = dezelfde `.statrow` als de all-time-regel (label links, getal rechts, geen
  dubbele punt), zodat het één blok blijft.
- Test `db::chest_counts_test`: dubbelklik, te late klik, andermans winst, een spawn zonder
  actor en een aankoop van hetzelfde lid mogen géén van alle meetellen. **37/37 groen.**

Stand op prod bij de deploy: FayBelle 16/6, easycomes 11/2, Waldstein 9/3, HeijiCat 7/2.

---

## ⏭️ Sessie (2026-07-31) — Shoprotatie met gewicht per item (Faybelle) + horseshoe in gebruik

**LIVE op prod + gecommit + gepusht** (`16bcceb`; subtree `market-gh` → `6adb7d2`). Deploy om
09:02, service draait, gem-kleursync normaal (12/12), geen fouten in het log. Vóór de deploy een
consistente online backup van de prod-DB gemaakt (`/opt/market/coins.db.bak-20260731-090100`),
want de migratie voegt kolommen toe aan een draaiende database.

**Stand op prod na de migratie:** 12 gems op gewicht 10, Lucky Horseshoe op 2, beide passen uit
de rotatie → pool van 13 items, som 122. Een gem staat **32,7%** van de dagen in de shop, de
horseshoe **7,3% ≈ 1 dag op 14**. (Controle: 12 × 32,7% + 7,3% = 400% = de 4 slots. De som van
alle kansen is altijd het aantal slots.) De verouderde rij `horseshoe_shop_odds_days` is uit de
`settings`-tabel verwijderd.

⚠️ **De dagselectie van 31/07 was al getrokken vóór de deploy** (onder de oude, uniforme regels).
Nieuwe gewichten slaan pas aan bij de volgende rotatie (02:00 Brussel) of meteen via de **↻
reroll** op Manage → Admin shop preview. Wie het effect direct wil zien: één item tijdelijk op
een fors gewicht zetten, rerollen.

### Waarom
Faybelle wil kunnen sturen hoe vaak een item in de dagshop verschijnt — niet enkel voor de
Lucky Horseshoe, maar **voor elk shopitem**: "een vakje bij elk shop item in de management met
gewicht en kans, en ik kan toggelen of iets in de shop komt of niet" (model: de chest-tiers).
Dat vervangt de aparte instelling `horseshoe_shop_odds_days` (1-op-N enkel voor boosters).

### Wat er nu staat
- **Twee kolommen op `items`** (idempotente migratie, `db.rs`): `shop_weight REAL DEFAULT 10`
  en `in_rotation INTEGER DEFAULT 1`. Twee velden i.p.v. één, zodat een item tijdelijk uit de
  shop kan **zonder** dat het ingestelde gewicht verloren gaat. Migratie zet de **passen**
  (category 'boost') op `in_rotation = 0` (die staan al permanent op de shop) en de **booster**
  op gewicht 2 — zie "zeldzaamheid" hieronder. De seeds doen hetzelfde voor een verse DB.
- **De dagtrekking is gewogen** (`db::shop_offers`, nu 3 argumenten): pool + verhoudingen komen
  volledig uit de items zelf. Methode = de **exponentiële race** (Efraimidis–Spirakis): sleutel
  `-ln(u)/w`, de `n` kleinste winnen — aantoonbaar gelijk aan "trek er één op gewicht, haal hem
  eruit, trek de volgende", maar in één pass én in de vorm waarvoor de kans exact te berekenen is.
- **`db::rotation_odds`** rekent per item de kans uit dat het **op een dag in de shop staat**.
  Dat is bewust níét het aandeel `w/Σw`: er worden 4 slots uit dezelfde pot getrokken, dus een
  item met 10% aandeel staat er veel vaker dan 10% van de dagen. Exact gerekend (integraal over
  een Poisson-binomiale staartkans, Simpson met 1024 panelen), **niet** bemonsterd.
- **Manage → Shop**: elk item heeft nu een blokje *Daily rotation* — vinkje "In the rotation",
  een gewicht-vakje en daarnaast de kans met een balkje, met eronder de praktische vertaling
  ("≈ 1 dag op 14"). Eigen formuliertje met ✓ (niet de autosave van het hoofdformulier): één
  gewicht wijzigen verandert de kans van **alle** items, dus de pagina moet opnieuw renderen.
  Bovenaan een regel met "4 slots per dag, getrokken uit N items, som van de gewichten = X".
- **Weg**: de instelling `horseshoe_shop_odds_days` (spec + beide aanroepen). De groep "Shop"
  in ⚙ Settings staat daardoor leeg.
- **Logboek**: elke wijziging logt als `admin/rotation` ("🎲 rotation") met oud → nieuw gewicht.

### Zeldzaamheid van de horseshoe (ongewijzigd overgenomen)
Gems staan op gewicht **10**, de horseshoe op **2**. Op prod (12 gems + horseshoe, 4 slots) geeft
dat **7,3% ≈ 1 dag op 14** — vrijwel exact de oude `horseshoe_shop_odds_days = 14`. Bij de
overgang verspringt er dus niets; Faybelle kan het nu zelf bijstellen in Manage → Shop.
Gewicht 10 als basis (niet 1) geeft ruimte om met **gehele** getallen fijn te regelen.

### Tests — 36/36 groen (was 27)
- `bot::horseshoe_odds` (4 tests): de 2× van de Lucky Horseshoe bij de **chest**-trekking,
  **uitputtend** bewezen i.p.v. bemonsterd (elke mogelijke roll één keer, 2 t/m 6 deelnemers).
  Daarvoor is de dubbel uitgeschreven winnaartrekking in `bot.rs` één pure helper geworden
  (`pick_weighted`), gebruikt door zowel de gewone opening als `!chestrescue`.
- `db::rotation_odds_tests` (3 tests): de **formule tegen een simulatie van de échte trekking**
  (60k rondes, ≤1 procentpunt verschil), ook bij scherpe verhoudingen (100 vs 1); som van de
  kansen = aantal slots; randgevallen (meer slots dan items, gewicht 0, 0 slots).
- `db::horseshoe_dryrun`: passen buiten de rotatie, uitgezet item wordt nooit getrokken en
  behoudt zijn gewicht, en "alles uitgezet" geeft een lege shop **zonder** iets op te slaan
  (anders zou elke paginaweergave opnieuw een schrijf-lock nemen).
- Handmatig end-to-end getest tegen de lokale server: opslaan (303 + juiste kans na render),
  komma-getal `2,5`, typfout `abc` laat het gewicht ongemoeid, vinkje uit, en een niet-admin
  die post verandert niets.

### 📌 Openstaand
Verhuisd naar de lijst **bovenaan dit bestand**, samen met de rest van 31/07 — één plek.

---

# Handover — Meadow Market (2026-07-29)

## ⏭️ Sessie (2026-07-29b) — Twee mobiele bugs: strook sprong terug naar links + naamkleur week af

**LIVE + gedeployd.** Twee user-meldingen, allebei enkel op een telefoon zichtbaar, allebei in
`web.rs`. De kleurmechaniek zelf is **niet** aangeraakt (uitdrukkelijke user-instructie).

### 1. Schuif je een strook naar rechts, dan sprong ze vanzelf terug naar links
Oorzaak: `auto_refresh_js` herlaadt de hele pagina periodiek — op de **Shop elke 5s**
(`AUTO_REFRESH_SHOP_MS`), op admin Coins/Log/Channels elke 20s. Een browser herstelt bij een
reload wél de scrollpositie van het *venster*, maar **nooit** die van een element met eigen
`overflow-x`. En sinds de mobile-fix van 07-27 schuiven precies de shop-strook (`.shelf`) en de
brede tabellen (`.ctable`/`.wtable`/`table.log`) binnen zichzelf. Dus: naar rechts schuiven →
enkele tellen later stond je weer links. Op desktop viel het amper op omdat de hele strook daar
meestal past.

Twee maatregelen in `auto_refresh_js`, samen:
1. **Niet herladen terwijl je bezig bent** — `scroll`/`touchstart`/`touchmove`/`pointerdown`/
   `wheel`/`keydown` (capture, passive) zetten een teller terug; pas na **8s rust** mag er
   herladen worden. De bestaande "niet herladen terwijl je in een veld typt"-gate blijft.
2. **Zijwaartse posities overleven de reload** — vlak vóór het herladen gaan alle `scrollLeft`-
   waarden in `sessionStorage` (sleutel `mmX:<pad>`, volgorde = DOM-volgorde), na de load worden
   ze teruggezet en meteen gewist (geen stale positie bij een verse navigatie).

De verversing zelf blijft dus intact (een admin ziet voorraad landen), ze wacht enkel haar beurt
af. Nieuwe test `web::auto_refresh_script` bewaakt de `format!`-escaping van het script (accolade-
balans + de markers); de gegenereerde JS is los met `node --check` gevalideerd. **27/27 groen.**

### 2. De naamkleur-preview week op mobiel af van de gekozen gem
User: je kiest een gem (bv. een rood), die gem hoort bij een Discord-rol met een kleur — op
desktop toont het previewveld exact die kleur, **op mobiel een andere tint**. Zelfde HTML, zelfde
hex, zelfde CSS: de afwijking komt dus van de telefoonbrowser zelf.

Diagnose: `:root` stond op **`color-scheme:light dark`**, terwijl de site één vast donker ontwerp
is en helemaal geen lichte variant heeft. Daarmee vertelt de pagina de browser "ik kan beide aan".
Staat de telefoon in lichte modus, dan is het *gebruikte* schema licht → een force-dark-modus
(Chrome's **Auto Dark Theme**, Samsung Internet's donkere modus) beschouwt de pagina als een
lichte pagina en hertint ze zelf. Zo'n algoritme herrekent precies **tekstkleuren** → de naam in
de preview kreeg een andere tint dan de rol-hex, terwijl de rest ongeveer goed bleef.

Fix (drie regels, géén wijziging aan de kleurmechaniek):
- `<meta name="color-scheme" content="dark">` in de `<head>` (wordt gelezen vóór de CSS geparsed
  is, en door sommige mobiele browsers als enige).
- `:root{color-scheme:only dark}` — `only` is de gestandaardiseerde manier om te zeggen: deze
  pagina is al donker, geen UA-override.
- `.swatch.light{…;color-scheme:only light}` — dat vakje is met opzet wit (het toont hoe je naam
  op Discord's lichte thema oogt), dus ook dáár mag force-dark niet ingrijpen; anders klopt de
  vergelijking dark/light niet meer.

**Verificatie:** de Gems-tab op 390px gerenderd met een echte sessie tegen de lokale server,
vóór en na → **pixel-identiek** (zelfde md5), dus aan de al goede weergave verandert niets.
⚠️ Het mobiele effect zélf is **niet lokaal te bewijzen**: headless Chrome op Linux kent
force-dark niet (met `--force-dark-mode` én `--enable-features=WebContentsForceDark` bleef de
render byte-identiek). Bevestiging moet van een echte telefoon komen. Helpt het niet, dan is de
volgende verdachte geen browser-force-dark maar een **OS-kleurfilter/inversie** op het toestel.

### 🔎 Wat onderweg geverifieerd is (en dus géén oorzaak was)
De gem-kleuren komen wél degelijk correct uit Discord: bij élke start logt de app
`gem-kleuren gesynct: 12 items (19 rollen, guild 1296469405651435592)` — **12 van 12** gems
matchten een gelijknamige rol, dus geen enkele gem viel terug op een oude seed-kleur. De hexen in
`items.color` op prod zijn letterlijk wat de Discord-API teruggaf, en de `.swatch`-CSS zet die hex
ongewijzigd op de tekst (geen `opacity`/`filter`/`text-shadow`). De sync-kant is dus gezond.

### 📌 Openstaand uit deze sessie
1. **Bevestiging op een échte telefoon** van fix 2 (naamkleur). Hard verversen kan nodig zijn — de
   CSS zit ingebakken in de HTML, maar de pagina zelf kan gecached zijn. Klopt de kleur nóg niet,
   dan is het geen browser-force-dark meer maar een **kleurfilter/inversie op OS-niveau**; volgende
   stap is dan de tint mét en zónder dat filter vergelijken vóór er nog code verandert.
2. Fix 1 (strook) is te controleren door op de Shop een strook naar rechts te schuiven en ~10s te
   wachten: ze mag niet meer terugspringen.

### ⚠️ Werkwijze-les uit deze sessie
De user meldde beide bugs als "**enkel op mobile**". Dat is een **scope-afbakening**: de
onderliggende mechaniek staat dan niet ter discussie. Ik ben eerst de Discord-rolkleuren, de
kleur-sync en de rol-hiërarchie gaan uitpluizen en stelde voor de kleurmechaniek aan te passen —
mis, en terecht geïrriteerd afgekapt. Werkwijze die wél werkte: de **echte** pagina met een
geldige sessie tegen een lokale server renderen op 390px én op 1280px en de twee vergelijken.
Verdachten bij zo'n melding zijn UA-gedrag (force-dark/`color-scheme`, scrollposities die een
reload niet overleven, touch-vs-click), niet de businesslogica.

### 🧰 Recept: de site op mobiele breedte bekijken (lokaal)
Handig, want dit kostte deze sessie het meeste uitzoekwerk:
- `MARKET_WEB_ONLY=1 ./target/release/market` (poort 8700; kill eerst een oude `release/market`).
- Sessie fabriceren in de lokale `coins.db`: `INSERT INTO sessions(token,user_id,username,created)`
  **met `created = nu`** — een rij met `created=0` wordt door de sessie-TTL geweigerd (dat is
  waarom de oude testsessies `s3`/`smoke2` op de loginpagina uitkomen).
- Pagina ophalen met `curl -b "session=<token>"`, de relatieve `src="/…"` naar
  `http://localhost:8700/…` herschrijven en als **bestand** wegschrijven (Chrome headless kan geen
  cookie meesturen; via `file://` mét absolute asset-URL's klopt de render wel).
- Screenshotten met de flatpak-Chrome: `flatpak run com.google.Chrome --headless --disable-gpu
  --no-sandbox --hide-scrollbars --window-size=390,1400 --screenshot=… file://…`.
  **Valkuil:** de flatpak-sandbox mag enkel in `~/Downloads` schrijven — een pad in `/tmp` of
  `~/.cache` geeft "Failed to write file: No such file or directory".
- **Force-dark is zo niet te reproduceren** (Android-only), dus mobiele kleurafwijkingen blijven
  een test op het toestel zelf.

---

## ⏭️ Sessie (2026-07-29) — Level-up-embed enkel in coin-kanalen (correctie op 07-27)

**LIVE + gepusht + gedeployd** (`1192c0d`; subtree `market-gh` → `74dd720`).

User-melding: level-up-berichten verschenen **in het marktkanaal** en dat is niet gewenst.
Oorzaak = de verhuizing van 07-27: het embed ging naar het kanaal van de uitlokker, maar een
level-up wordt ook getriggerd door **knopklikken** (daily-check-in, 🎁-gift-claim, weekly-claim)
en die knoppen kunnen in **élk** kanaal staan.

**Nieuwe regel (user):** het embed komt in het kanaal waar je levelde **als dat kanaal op de
lijst staat waar treasure chests mogen spawnen** (`coin_channels`, threads via hun parent) —
staat het er niet op → **prod #coins**.

- Nieuwe helper **`levelup_target(ctx, data, channel)`** in `bot.rs` (vlak boven `handle_message`):
  exact dezelfde check als `maybe_spawn_chest` — `db::is_coin_channel` op het kanaal, anders op
  het `thread_parent`. Bewust één lijst om te beheren.
- **Toegepast op de 3 knop-paden**: daily (`mc.channel_id`), gift-claim, weekly-claim.
- **Ongewijzigd (per definitie al goed)**: het chat-award-pad zit ná de `coin_here`-gate, en een
  chest spawnt enkel in een coin-kanaal → beide geven hun kanaal rechtstreeks door. Enkel in de
  comments vastgelegd waarom, plus een ⚠️ op `maybe_levelup` dat de **caller** een toegelaten
  kanaal aanlevert.

**Prod-lijst geverifieerd** (16 coin-kanalen): #meadowmarket en #coins staan er **niet** op —
dat bevestigt de diagnose. Verificatie: `cargo build` schoon, **26/26 tests groen**, service
active, home 200, geen warnings in het journal na de restart.

---

## ⏭️ Sessie (2026-07-27) — Level-up in het juiste kanaal + tag die écht pingt + site mobile-proof

**LIVE + gedeployd.** Twee user-vragen, twee bestanden (`bot.rs`, `web.rs`).

### 1. Level-up-embed verhuist naar het kanaal van de uitlokker
Het embed ging altijd naar prod #coins, ook al levelde je door te typen in een ander kanaal —
het feestje stond dus los van het gesprek. `maybe_levelup` kreeg een **`channel`-parameter**;
alle **6 aanroepplekken** geven nu het kanaal mee dat de level-up uitlokte:
- chat-award → `msg.channel_id` (**de eigenlijke vraag**) · daily → `mc.channel_id` ·
  gift-claim → `mc.channel_id` · chest-open → `channel_id` · chestrescue → `channel` ·
  weekly-claim → `mc.channel_id`.
- `PROD_COINS_CHANNEL_ID` blijft **enkel als terugval** (channel-id 0).

### 2. De tag pingte niet — nu wel
`<@uid>` **binnen** een embed rendert wel als naam maar **Discord stuurt er geen melding voor**.
Dát was "de tag werkt niet". Opgelost door de mention óók als **gewone berichttekst vlak boven
het embed** te zetten (`.content(format!("<@{uid}>"))`). De tag in de embed-beschrijving blijft
staan (cosmetisch); de ping komt van de content-regel.

### 3. Site was niet bruikbaar op een telefoon — echte oorzaak was layout, geen padding
- **Kern (`.content`)**: het is een grid met een **impliciete auto-kolom**. Een auto-track wordt
  op **max-content** gesorteerd, en `.shelf` (shop-strook, vaste kaartbreedtes van 170–210px)
  heeft een max-content van honderden pixels → de kolom werd **751px** breed, dus élke kaart werd
  breder dan het scherm en de héle pagina schoof zijwaarts. Gefixt met
  **`grid-template-columns:minmax(0,1fr)`**. Op desktop **identiek** (de track vulde daar toch al
  de volle breedte); op smal scherm mag hij niet meer groeien en scrollt de strook binnen zichzelf.
- **Additief `@media (max-width:640px)`-blok** onderaan de CSS (alles erboven = desktop-waarheid,
  ongemoeid): kleinere padding/koppen; **tabellen** (accounts · coins · log · weging) krijgen
  `display:block;overflow-x:auto` zodat ze binnen zichzelf schuiven i.p.v. de pagina te rekken;
  **Settings-velden stapelen** (label was `flex:0 0 13rem` + hulptekst op 13,7rem marge — past
  niet); `.addbar`-input krijgt `min-width:0` (een `<input>` weigert anders onder ~20 tekens te
  krimpen en duwde de rij buiten beeld); `align-content:start` haalt een **lege kloof van ~150px**
  tussen nav-kaart en inhoud weg (de grid-rijen rekten mee met de schermhoogte).

**Geverifieerd** met headless Chrome op 390px over **8 pagina's** (inventory, shop, leaderboard,
admin coins/settings/shop/log/accounts): overal `scrollWidth == viewport`, geen pagina-overflow.
Desktop op 1280px nagekeken → ongewijzigd. *(Caveat: headless Chrome doet geen echte mobiele
viewport-emulatie — het rendert als een smal desktopvenster. Test op een échte telefoon blijft
de doorslag.)*

### ⚠️ Valkuil voor de volgende sessie
**NIET `cargo fmt` draaien op dit project.** Het is niet rustfmt-conform: één `cargo fmt`
herschreef 7 bestanden / ~1200 regels die niets met de wijziging te maken hadden. In deze sessie
teruggedraaid (`git checkout -- src/`) en handmatig opnieuw gedaan.

---

## ⏭️ Sessie (2026-07-24) — Streak-reset uitgelegd + streak-venster 30→47u + Waldstein-streak hersteld

**Config-fix, GEEN code/deploy.** User-melding: "gisteren streak 3, vandaag opnieuw ingecheckt en
mijn streak staat weer op nul." Diagnose op prod → **geen bug, wél een ontwerpprobleem** in het
streak-model. Opgelost door het **streak-venster te verruimen** (via panel/Settings) en de verloren
streak handmatig te herstellen.

### De diagnose (geen kapotte code)
De streak telt **rollende uren tussen twee klikken**, geen kalenderdagen. Prod-instellingen:
`daily_cooldown_hours = 20`, `daily_streak_window_hours = 30`. Klik je opnieuw **binnen** het venster
→ streak +1; erbuiten → reset naar 1 (`bot.rs:454-462`, logica correct).
- **Wat er gebeurde bij Waldstein**: check-in **07-23 00:51** (streak 3, heel vroeg) → volgende
  check-in **07-24 14:00** (namiddag) = **37 u ertussen** > venster 30 u → reset naar 1.
- **Waarom het als een bug voelt**: 20 u cooldown + 30 u venster = maar **~10 u geldig venster per
  dag**, dat bovendien **verschuift** met het klik-uur. Vroeg de ene dag + laat de volgende → je
  schiet er onbewust overheen. Voor de speler waren het twee opeenvolgende kalenderdagen, dus
  verwachting = streak loopt door. Uren-model ≠ kalenderdag-verwachting.

### De fix (LIVE)
- **`daily_streak_window_hours` 30 → 47** in de prod-DB (`/opt/market/coins.db`, `settings`-tabel).
  User zette het via het panel (eerste poging: waarde ingevuld maar **niet opgeslagen** → prod bleef
  30; tweede poging opgeslagen → geverifieerd op **47**). **Geen deploy/herstart nodig**: de code
  leest de setting vers per check-in (`settings::f64_of`). Nieuw geldig venster ≈ 27 u/dag → drift
  wordt opgevangen, een échte gemiste dag (>47 u) reset nog steeds.
- **Waldstein-streak hersteld** `1 → 3` (`UPDATE coins SET daily_streak=3 WHERE username='Waldstein'`,
  uid `391337551543271433`). Puur de teller; `last_daily` ongemoeid.

### ⚠️ Openstaand / ter overweging (niet gedaan deze sessie)
- **Nette fix = kalenderdag-model** i.p.v. rollende uren (streak = opeenvolgende kalenderdagen met
  een check-in). Lost de drift structureel op, matcht de speler-verwachting. Vereist een codewijziging
  in `bot.rs` (`daily`-handler) + deploy. **Bewust NIET gedaan** — user koos de snelle config-fix (47u).
  Als de klachten terugkomen, is dit de volgende stap.
- Streak-teller-herstel voor andere spelers: niet gevraagd, niet gedaan (enkel Waldstein).

---

## ⏭️ Sessie (2026-07-23) — Treasure chests spawnen nu in ÁLLE coin-kanalen + threads (niet meadowland)

**LIVE + gepusht + gedeployd** (`9546d2f`, subtree → `market-gh` `1d15d6a..8de1090`). Chests
verschenen tot nu enkel in **#general**; nu in **elk van de 16 coin-kanalen + hun threads**.
De **in-game chat-bridge (meadowland)** blijft chest-vrij.

### Wat & waarom
- **Kern** (`bot.rs` `maybe_spawn_chest`): de hardcoded gate `msg.channel_id != CHEST_SPAWN_CHANNEL_ID`
  (= #general) is vervangen door **dezelfde coin-kanaal-check die coins gebruikt** —
  `db::is_coin_channel(...)` + `thread_parent(...)` voor threads. Meadowland staat **niet** op de
  `coin_channels`-lijst (op prod geverifieerd: 16 kanalen, geen meadowland) → automatisch uitgesloten,
  géén expliciete blacklist nodig. Keuze bewust **"chests = coin-kanalen"** (één lijst beheren).
- **Waarom bijna gratis**: de `ChestTracker`-boekhouding (`recent`/`active`/`cooldown_until`/`chests`)
  was **al per-kanaal** (HashMap op channel_id). En `maybe_spawn_chest` werd sowieso enkel bereikt ná
  de coin-gate in `handle_message`. De nieuwe check binnen `maybe_spawn_chest` is dus vandaag redundant,
  maar houdt de functie **zelfstandig** (goedkope indexed query + gememoïseerde thread_parent).
- **Threads**: een thread-bericht keyt op de **thread-id** → de chest verschijnt **ín de thread**.
  Verwacht daar weinig chests (de `chest_distinct_users`-drempel wordt zelden gehaald in een stille thread).

### Bonus-fix: `chestrescue` postte in het verkeerde kanaal
`!chestrescue` (admin: verweesde chest heropenen) gebruikte hardcoded **#general** voor de uitslag,
de cooldown én het opruimen van het dode chest-bericht. Met multi-kanaal was dat fout geworden. Nieuw:
`db::chest_channel_from_log(msg_id)` diept het **originele kanaal** op uit `server_log` (elk chest-event
draagt `channel_id`); #general enkel nog als **terugval** als het logboek niks kent.
- `CHEST_SPAWN_CHANNEL_ID` blijft bestaan als die terugval; `COIN_CHANNEL_ID` (dev) enkel nog voor het
  dode `CHEST_SPAWN_ON_START`-testpad. Comments bijgewerkt.

### ⚠️ Op te volgen nu het live is
- **Economie**: 1 → 16 kanalen = veel meer spawn-oppervlak → meer uitbetaling. Hou #coins/logs in het oog.
  Bijsturen kan **zonder deploy** via Manage → ⚙ Settings: `chest_distinct_users`, `chest_window_min`,
  `chest_channel_cooldown_min`, of `chest_prize`.
- **De 5 onzichtbare coin-kanalen** (zie sessie 2026-07-21b): daar komen **geen chests** tot de bot
  `View Channel`/`Send` krijgt — zelfde Discord-rechtenkwestie, nog steeds open.

---

## ⏭️ Sessie (2026-07-21b) — BUG opgelost: threads leverden geen coins op (retro-payout AFGEBLAZEN)

**Forward-fix LIVE + gepusht + gedeployd + IN-GAME GEVERIFIEERD** (`bc25a77` → gepolijst in
`21e076f`, subtree t/m `21e076f`). Threads onder een coin-kanaal leveren nu coins op. De
**retroactieve inhaalslag is bewust NIET uitbetaald** (user-beslissing, zie onder) — de
`thread_backfill`-tabel op prod is **leeggemaakt**, dus `!threadfix_commit` betaalt niets.

### De bug (user-melding) + fix
Berichten in een **thread** in een coin-kanaal (bv. arts-crafts) leverden **geen coins** op:
`handle_message` checkte `is_coin_channel(msg.channel_id)`, en bij een thread is `msg.channel_id`
de **thread zelf**, niet het parent-kanaal → niet op de lijst → niks. Fix = helper `thread_parent()`
in `bot.rs`: bij een thread telt het **parent-kanaal** voor de coin-check. **Valkuil dichtgezet**:
een gewoon kanaal heeft óók een `parent_id` (categorie) → strikt gate op de thread-types
(`PublicThread`/`PrivateThread`/`NewsThread`). **Live bewezen** op prod: een bericht in een
arts-crafts-thread kende +1 coin toe (via tijdelijke DIAG-log, nadien verwijderd).
- **Perf**: `to_channel` is in serenity 0.12 GÉÉN cache-lookup maar een echte `get_channel`-HTTP-call.
  Daarom `thread_parent` **gememoïseerd** (`Data.parent_cache`: channel_id → Some(parent)/None);
  fouten worden bewust niet gecachet (transiënte rate-limit sluit geen thread permanent uit).
- **NB coins zijn stil**: per-bericht-coins geven geen zichtbare feedback (enkel #fortuna-log), dus
  "het lijkt niks te doen" ≠ kapot. Enkel level-ups/aankopen posten in #coins.

### ⚠️ 5 coin-kanalen zijn ONZICHTBAAR voor de bot (openstaand!)
Tijdens de scan bleek: de bot mist **View Channel** op **5 coin-kanalen** → `403 Missing Access` bij
`GET /channels/{id}`: **☀️hychat, ☀️parachat, 🌄hypics, 👋introductions, 📔style-magazine**. Gevolg:
daar vallen **live óók geen coins** (bot ziet de berichten niet, thread of niet). **Fix = de bot
View Channel + Read Message History geven op die 5 kanalen** (Discord-rechten, user-actie). Nog open.

### Retroactieve inhaalslag — gebouwd maar AFGEBLAZEN (user-beslissing)
Tooling staat LIVE (`b774c15`, `c4129a3`): 3 admin-commando's `!threadfix_preview` / `_commit` /
`_reset` (patroon zoals oud `!levelfix`) + tabel `thread_backfill` + `discord_rest` helpers
(`active_threads`, `archived_threads`, `get_messages_detailed`) + `thread_backfill_test` (2 tests,
26 totaal groen). De scan is **buiten de binary om** (Python + bot-token) meermaals gedraaid als
preview in dev-coins. Belangrijke bevinding onderweg: berichten **ná** de fix-deploy leverden live al
coins op → een correcte backfill vereist een **cutoff op de fix-deploytijd** (anders dubbel). Gecorrigeerde
preview = ~160 coins / 89 berichten / 6 leden (FayBelle leeuwendeel). Dekking bleef **niet 100%**
(5 onzichtbare kanalen + private threads → Manage Threads nodig).
**User-beslissing: NIET uitbetalen** — "voor de toekomst is het nu correct; liever niks dan iets fout."
→ `thread_backfill` leeggemaakt (0 rijen). De commando's + tabel blijven dormant in de code (geen
kwaad; lege tabel = commit doet niks). Eventueel later op te ruimen als ze nooit gebruikt worden.

**→ Open follow-up:** de 5 onzichtbare coin-kanalen (View Channel voor de bot) — puur een Discord-
rechtenkwestie, geen code. Zolang dat niet gebeurt, verdienen leden daar helemaal geen coins.

---


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

## ⏭️ Sessie (2026-07-21) — ∞-voorraadknop grijst uit i.p.v. te verdwijnen

**LIVE op prod + gecommit + gepusht + gedeployd** (commit `75a4788`; subtree `market-gh` →
`77c6dae`; deploy geverifieerd: `market.service` active, home HTTP 200).

Kleine admin-UX-fix op **Manage → shop** (item-editor, voorraadblokje). De **∞-knop** (voorraad
op *unlimited* = niet meer tellen) **verdween** volledig zodra een item al onbeperkt was — dat
oogde alsof er iets weg/kapot was (user-melding). Nu blijft de knop staan maar wordt hij
**grijs + disabled**:
- `web.rs` `admin_item()` (bij de `let inf`): rendert nu áltijd een ∞-knop; bij `stock >= 0` de
  echte submit-knop, bij `stock < 0` een `type="button" disabled` variant met tooltip
  *"Already unlimited — add stock to count again"*.
- `web.rs` CSS `button.btn:disabled`: grijst nu écht uit (`opacity:.4;cursor:default` + geen
  hover-filter) — voorheen haalde die regel enkel de schaduw/transform weg.
- Terugweg ongewijzigd: `+ Add stock N` op een unlimited item begint weer bij 0 (`add_stock`
  behandelt `cur<0` als base 0) → knop wordt weer actief. Geen DB-wijziging, puur render/CSS.

## ⏭️ Sessie (2026-07-20) — Twitch permanente-pas-redeem gebouwd + verjaardag-rol-ID vastgelegd

**Gecommit + gepusht, NIET gedeployd** (commit `8be4ef9`; subtree `market-gh` → `512264d`). Bewust
géén prod-deploy: de code is inert zonder `[twitch]`-config (net geverifieerd: prod `secrets.json`
heeft géén twitch-blok → `twitch_ready()`=false). Live-test met een echte streamer volgt later.

### Twitch: permanente-pas-redeem naast de dagpas
Er bestond al één channel-points-reward (dagpas, 24u → `grant_day_whitelist`). Nu erbij: een **2e
reward** voor een **permanente** pas (`db::grant_perma_whitelist`, `expires = NULL`, geen afteller).
`grant_perma_whitelist` bestond al in db.rs — de rest van de machinerie is nieuw:
- **`config.rs`**: `twitch_perma_reward_title` + `twitch_perma_reward_cost` (secrets.json/env
  `TWITCH_PERMA_REWARD_TITLE`), helpers `twitch_perma_enabled()` (= titel ingevuld) +
  `twitch_perma_reward_cost()` (dev 0 / prod **5000 = placeholder**). **Leeg titel ⇒ perma-redeem
  UIT** — bewust géén verzonnen speler-zichtbare tekst (huisregel [[geen-eigen-publieke-teksten]]).
- **`twitch.rs`**: `ensure_reward()`-helper (dag+perma zoeken/aanmaken/kost-syncen, ontdubbeld uit
  bootstrap), EventSub abonneert op **beide** reward-ids, `pass_kind_for(event.reward.id)` routet
  dag→24u vs perma→NULL (onbekend/leeg → veilige val Day), perma-tak met eigen log
  (`whitelist-perma`) + chat. `Ctx` kreeg `perma_reward_id`.
- **Tests**: unit-test `pass_kind_routing` (24/24 groen) + **mock-e2e `docs/perma_e2e.sh`** — start
  de Twitch-CLI EventSub-mock + market in mock/web-only en injecteert een dag- én perma-redeem;
  **bewezen** dag→`expires=now+24u`, perma→`expires=NULL`. Doc `docs/twitch-setup.md` bijgewerkt.

**⚠️ Openstaande user-beslissingen vóór de live-test** (alle speler-zichtbaar → user levert):
1. **Exacte reward-titel(s)** in het channel-points-menu (perma-titel is nu leeg = feature uit).
2. **Kost** permanente pas in channel points (code-default 5000 is placeholder).
3. **Chat-bevestigingsteksten** — nu **NL**, gespiegeld op de dagpas ("✅ permanente toegang voor
   Hytale-naam '…'"). Rest van market is Engels → evt. deze twee ook naar Engels.

Daarna is de live-test enkel nog: `[twitch]`-creds + `twitch_tokens.json` van de streamer +
`twitch_perma_reward_title`/`_cost` in prod `secrets.json` → `systemctl restart market`. De
whitelist-keten (grant → tale-bot `reconcile_market` → `whitelist.json`) staat al klaar. Overweeg
de ooit-gelekte Client Secret te roteren. Volledige status: memory [[tale-twitch-tickets]].

### Verjaardagen: birthday-rol-ID vastgelegd
De verjaardag wordt op prod (Magic Meadow, guild `1296469405651435592`) getriggerd door de
**MEE6-rol `🎂Birthday!🎂` = ID `1422232059815919697`** (opgezocht via de bot-token op de VPS).
Detectie-plan = `GuildMemberUpdate`-handler die kijkt of die rol-id net is **toegevoegd**. Vastgelegd
in memory [[market-birthday-role]]. Nog te beslissen: cadeaubedrag (handover noemt 500 coins) +
birthday-chest-ontwerp. **Nog niets gebouwd.**

## ⏭️ Sessie (2026-07-19/20) — diepe code-review + volledige fix-inhaalslag (5 ronden)

**Alles LIVE op prod + gecommit + gepusht.** Op vraag een **diepe code review** gedaan van de
hele market-codebase (~10,3k regels Rust, 8 modules) via 4 parallelle review-agents (db/web/bot/
twitch+rest+settings), daarna elke topbevinding zélf in de bron geverifieerd en in **5 ronden**
gefixt-getest-gedeployd-gepusht. Van **0 → 23 tests** (concurrency- + security-regressievangnet).

**Rode draad van de bugs:** check-then-write over de gedeelde r2d2/SQLite-pool → races tussen de
bot-, web- en twitch-taak. Fix-patroon overal: **atomische DB-guard of `IMMEDIATE`-transactie**,
geholpen door de `busy_timeout` uit ronde 1.

| Ronde | Commit | Niveau | Fixes |
|---|---|---|---|
| 1 | `ce5a527` | KRITIEK | daily dubbel-claim (atomische `WHERE last_daily<=guard`) · `busy_timeout=5000` |
| 2 | `ea8a939` | HOOG | `maybe_levelup`-race (CAS `advance_gifted_level`) · bericht-cooldown (`award_if_ready`) · `admin_adjust` lost-update (IMMEDIATE-tx) |
| 3 | `a6ce24b` | HOOG/MIDDEL | open redirect (`\`-bypass) · sessie-TTL server-side + `Secure`-cookie · Twitch dedup (message_id) · Twitch WS-keepalive · `claim_level_gift` claim+credit in één tx · discord_rest 15s-timeout |
| 4 | `9fc83d3` | MIDDEL | `add_stock` (IMMEDIATE-tx) · `shop_offers` geserialiseerd (lockless read + IMMEDIATE-tx, canonieke set) · weekly-inhaal na herstart (kv-marker `weekly_last_fired`) |
| 5 | `e0ea4d2` | LAAG | `/logout` GET→POST (CSRF) · `esc()` escapet `'` · `leaderboard_week` tie-break (`COALESCE AS username`) · `settings.rs` `clamp_bounds` (min>max) · Twitch mock enkel bij vlag/loopback |

Subtree `market-gh` bijgewerkt t/m `40c9164`. Elke deploy geverifieerd: service active, geen
panics, home 200, en gerichte checks (Secure-cookie live, `GET /logout`→405).

**⚠️ Belangrijke architectuur-vondst (memory [[market-db-no-wal]]):** `coins.db` blijft bewust op
**rollback-journal + busy_timeout, GEEN WAL**. Het Hytale-panel (`lab/tale/panel/panel.py`, user
`hytale`) **leest `coins.db` rechtstreeks read-only** (`?mode=ro`) uit `/opt/market/` — een map
waar het niet kan schrijven. Onder WAL faalt die read tijdens het deploy-venster (empirisch
gereproduceerd). Alleen het *schrijven* (pas intrekken) gaat via `/internal/pass/revoke`. WAL kan
pas als de panel-read óók achter `/internal/*` verhuist (kleine ingreep: `/internal/passes`-route
in market + HTTP-call in panel.py — panel-kant is tale-domein, met de tale-kant af te stemmen).

**Nog open uit de review:** niets — alle KRITIEK/HOOG/MIDDEL/LAAG-bevindingen zijn afgewerkt.
Losse latente noten leven in de code-comments (bv. `list_members` cap 1000 zonder paginatie —
by-design bij ~33 leden).

## ⏭️ Sessie (2026-07-19a) — Absent-tab (rename) + kanaal-backfill van afwezigheid

**Alles LIVE op prod + gecommit + gepusht + gedeployd** (commit `41a0092`; subtree
`market-gh` → `41dd827`). Backfill **op prod uitgevoerd en geverifieerd**.

User wilde tóch retro terug in de tijd: neem alle members, ga alle kanalen af, zoek hun
laatste bericht. Bij benadering — ze checken alles handmatig vóór een kick.
- **Inactives → Absent** hernoemd (tab `💤 Absent`, route `/admin/absent`, kolom "Dagen
  afwezig", aflopend gesorteerd). `admin_inactives` → `admin_absent`.
- **Backfill** (`/admin/absent/backfill`, POST → achtergrondtaak, 1 tegelijk):
  `run_absence_backfill` scant alle prod-tekstkanalen **terug in de tijd tot ~800 dagen**,
  neemt per lid het **recentste bericht** als `last_seen`; wie niks postte binnen de scan
  valt terug op zijn **join-datum**. Knop + laatste-run-status op de pagina.
  - `discord_rest`: `get_messages` (paginatie via `before`, 429-retry), `list_members_joined`
    (met `joined_at`), helpers `snowflake_secs` (msg-id → aanmaaktijd, geen ISO-parsing) en
    `iso8601_to_secs` (`joined_at`).
  - `db`: vrije `kv`-tabel + `kv_get/kv_set` (backfill-status + laatste-run-tijd).
- **Prod-run geverifieerd**: *"Absent-backfill klaar: 33/33 leden met bericht gevonden"* (~30s).
  Lijst nu reëel: Varun Sariv 358 d, King_Leopold 331 … tot recent-actieven op 0,3 d. Backfill
  getriggerd via tijdelijke admin-sessie (nadien verwijderd).
- **⚠️ Gedeeltelijke dekking**: kanalen zonder bot-leesrecht (o.a. #moderator-only, diverse
  categorie-kanalen) gaven **403** en zijn overgeslagen — toch 33/33 leden gevonden (iedereen
  postte o.a. in #general). Wil je 100%? Geef de bot **View Channel + Read Message History**
  op die kanalen en her-run de backfill. **Log-cosmetica**: de 403-tekst zegt "lacks 'Manage
  Roles'" (generieke `explain()`-tekst) — misleidend, het gaat om leesrecht; log-only.
- **NB**: nog niemand ≥1 jaar (top = 358 d). De ≥1-jaar-vlag werkt zodra iemand de drempel haalt.
- **Polish** (`d092709`, gedeployd): "Laatst actief"-kolom toont enkel **dagen** ("358 dagen
  geleden"), niet meer dagen+uren (user-verzoek).

### 📌 Open follow-ups (Absent/verdeel-kist)
- **Verdeel-kist-mechaniek zelf** — nog te bepalen met user (opgeven-flow + 24u-chest + saldo
  onder deelnemers verdelen).
- **Bot-leesrecht** op de 403-kanalen (View Channel + Read Message History) → dan backfill
  her-runnen voor 100% dekking. Nu 33/33 via #general e.a.
- **Cosmetica**: 403-log zegt misleidend "lacks 'Manage Roles'" (generieke `explain()`) — het is
  leesrecht; log-only, laag prioritair.

---

## ⏭️ Sessie (2026-07-18g) — Manage → Inactives + last_seen-tracking; Horseshoe-tekst; Favor-spelling

**Alles LIVE op prod + gecommit + gepusht + gedeployd** (commits `051149a`, `7c53368`;
subtree `market-gh` → `4260feb`).

### Manage → Inactives + activiteits-tracking (voorbereiding "verdeel-kist")
Idee (user): leden die ~1 jaar niks deden worden opgegeven → speciale 24u-chest waarbij
deelnemers hun coins verdelen. **Mechaniek zelf nog te bepalen**; deze sessie = de fundering.
- **Belangrijke realiteit** (aan user uitgelegd): Discord geeft géén retro "laatst getypt",
  en `earn_log` wordt na 8 dagen gewist → er is **geen jaar-historiek**. Enige weg = vanaf nu
  **vooruit** meten. Backfill van kanaalgeschiedenis bewust NIET gedaan (user-keuze).
- **Nieuwe tabel** `member_activity(user_id, name, last_seen)` + helpers `touch_activity`
  (upsert, naam-behoudend), `seed_activity` (INSERT OR IGNORE), `list_inactives` (aflopend
  op afwezigheid + saldo). db.rs.
- **Bot**: `GUILD_MESSAGE_REACTIONS`-intent erbij (niet-privileged). Élk niet-bot-**bericht**
  in de prod-guild ververst `last_seen` (ongeacht kanaal/commando/cooldown — activiteit ≠ coins);
  nieuwe **ReactionAdd**-handler doet hetzelfde voor reacties. Bij **CacheReady** krijgt elk
  huidig prod-lid `last_seen = nu` (seed, klok start; bestaande metingen blijven). **Geverifieerd
  op prod: 33 Magic-Meadow-leden geseed op nu; dev-guild bewust niet.**
- **Web**: Manage → **💤 Inactives**-tab (`/admin/inactives`) — tabel aflopend op dagen
  inactief, ≥1-jaar-kandidaten met ⚑-vlag, uitleg-note dat de teller vooruit opbouwt. Admin-UI
  is Nederlands (zoals de andere Manage-pagina's).
- **NB startdatum-klok = 2026-07-18.** Iedereen staat nu op 0 dagen; pas ~2027-07-18 kan iemand
  "365" halen. De verdeel-kist-feature is dus pas na een echt jaar zinvol tenzij de user de
  drempel verlaagt.

### Lucky Horseshoe-omschrijving + Favor-spelling (afronding 18f-parkeerpunt)
- **Omschrijving** = *"You will have twice as much chance to open Fortuna's Favor."* (user-tekst;
  "Foruna"-typo → "Fortuna"). Seed-default in db.rs + **live prod-DB-rij** direct bijgewerkt
  (seed is idempotent). Geen binary-redeploy nodig geweest voor die tekst (DB-driven).
- **Chest-naam overal US "Favor"** — de 2 resterende "Favour"-comments (db.rs, web.rs)
  gelijkgetrokken; user bevestigde: "favour" was zijn Britse typo, US aanhouden.

---

## ⏭️ Sessie (2026-07-18f) — inventory-polish: Trinkets, gem-image2, gems 6-per-rij

**Alles LIVE op prod + gecommit + gepusht** (commit `0a72f92`; subtree `market-gh` → `572a7a1`).
Drie geparkeerde inventory-klussen (1, 2, 4 uit 18e) zelfstandig afgewerkt:

- **"Boosts/Boosters" → "Trinkets"** — sub-tab-knop (`🍀 Trinkets`) + schap-titel in de
  fancy Spicy-Sale-font (spiegelt de "Basic Gems"-titel); ook in de admin-preview. Interne
  ids (`data-t="boosts"`, `#p-boosts`) ongewijzigd gelaten (niet speler-zichtbaar). "Trinkets"
  = user-woord; 🍀 hergebruikt uit de bestaande Lucky-Horseshoe-context (geen nieuwe emoji verzonnen).
- **2e afbeelding (image2) op gems** — `gem_slot` toont nu de optionele `image2` onder de titel
  (`.thumb2`), net als de shop-kaart en `booster_slot` al deden.
- **Basic Gems + Trinkets in een vast 6-koloms grid** (`.shelf.gems6`) i.p.v. de flex-wrap-strip;
  kaders rekken in hoogte mee zodat alle tekst past. Responsive terugval: **3 kol ≤820px, 2 ≤480px**
  (anders onleesbaar smal op telefoon).

Verificatie: `cargo build` schoon (enkel bestaande warnings), lokaal `MARKET_WEB_ONLY=1` → CSS +
markup uitgeleverd, prod-curl bevestigt `repeat(6,minmax(0,1fr))` live.

**Nog geparkeerd (blijft open):** ① Lucky-Horseshoe-omschrijving (user-tekst met "Foruna"-typo →
eerst navragen), ② "jaar niet actief member" (nog te specificeren).

---

## ⏭️ Sessie (2026-07-18e) — weekly-cadeau-feature, affordability-knop, inventory-preview + polish

**Alles LIVE op prod + gecommit + gepusht** (`6163a87` → `7a28ecb`; subtree t/m `3ebfc34`).

### ‼️ TWEE HARDE HUISREGELS (user, met nadruk — vanaf nu altijd)
1. **Geen eigen tekst/labels.** Verzin NOOIT speler-/klant-zichtbare tekst, labels of uitleg.
   Gebruik enkel de tekst die de user aanlevert; **ontbreekt die → vraag ze**. (Zie [[geen-eigen-publieke-teksten]].)
2. **Geen ephemerals**, als regel. Enige toegestane uitzondering die de user expliciet vroeg:
   de weekly-knop bij een verkeerde klikker → ephemeral **"This is not your prize."**.

### Weekly leaderboard — cadeau-feature (de grote brok, veel iteraties)
- **Post-kanaal = prod #coins** (`PROD_COINS_CHANNEL_ID`; user-keuze), zaterdag 15:00 Brussel.
  `WEEKLY_LEADERBOARD_ENABLED` (bot.rs) = **true** → loop hervat volgende zaterdag.
- **Embed**: titel "🏆 Weekly leaderboard"; ranglijst met plaats-iconen — **1-3 = 👑🥈🥉**,
  **4-9 = 4️⃣…9️⃣**, **vanaf 10 = `**N.**`** (vet, met punt). Onderaan: `🎉 **Top Three claim your
  prize below!** <:MM_party:1522596802874835014>` (MM_party = emoji in de **Magic Meadow**-guild).
- **Top 3 getagd in de message-CONTENT** (niet in de embed — mentions in een embed **pingen niet**,
  in de content wél).
- **Eén groene "🎁 Claim your reward"-knop** (niet 3). custom_id = `wg:g1,g2,g3` bevat de 3
  cadeau-rijen (`level_gifts` kind='weekly'). Handler `handle_weekly_claim` kiest bij een klik het
  cadeau van de klikker → plaats **1/2/3 → 300/200/100** (`credit_earned`, telt mee) + publiek
  `**Naam** won **X** coins with the weekly leaderboard.` in #coins. **Plaats 4+ → ephemeral**
  "This is not your prize."; al geclaimd → stil.
- **Knop grijst uit zodra alle 3 geclaimd** (bericht-brede edit → disabled; per-gebruiker grijzen
  kan Discord NIET). Helper `db::gift_claimed`.
- **NB huidige stand**: er staat nog een **handmatige** weekly-post in #coins (vorige week,
  gereconstrueerd uit `earn_log`): FayBelle 300 (**al geclaimd**), TimmyThumb 200 + Yâ-Ôd 100 (open).
  Dit was een eenmalige recovery na een accidentele 15:00-fire (opgeruimd). Data-recovery: het
  weekly-venster is `[fire−7d, fire)` uit `earn_log` (pruning ~8 dagen).

### Shop / inventory
- **Buy-knop grijs bij te weinig coins** (`shop_slot` kent nu het saldo) → de "not enough
  coins"-banner verschijnt niet meer in de normale flow; server-side rem blijft als vangnet.
- **Inventory-vakken breder (136→170px)** + **gem-omschrijving niet meer afgekapt** (clamp weg) +
  **Use-knop `margin-top:auto`** (knoppen uitgelijnd bij ongelijke omschrijvingen).
- **Preview inventory** (admin, `/admin/inventory`): volledige inventory met alle items als
  owned/unlocked. **Vervangt** de oude "Admin shop items"-pagina (`admin_shop` weg).

### 🖼 Emoji-blokker
De bot heeft **geen "Manage Emojis"-rechten** in de guild → ik kon `artwork/toeter.png` niet zelf
als emoji uploaden. De user maakte zelf **`<:MM_party:1522596802874835014>`** (Magic Meadow). Bot
zit in 2 guilds: **WaldsteinDevZone** (`652452615879262220`, waar dev-coins zit) + **Magic Meadow**
(`1296469405651435592`, prod). Emoji uit Magic Meadow rendert overal waar de bot post.

### 📌 Nog geparkeerd (uit de grote batch die de weekly onderbrak)
- ✅ ~~"Boosts" → "Trinkets"~~ — **gedaan in 18f**.
- ✅ ~~image2 (2e afbeelding) tonen in de inventory~~ — **gedaan in 18f**.
- ✅ ~~Gems 6-per-rij in de inventory~~ — **gedaan in 18f**.
- ✅ ~~Lucky Horseshoe-omschrijving~~ — **gedaan in 18g** ("...open Fortuna's Favor.").
- ✅ ~~"jaar niet actief member"~~ — **fundering gedaan in 18g** (Manage → Inactives +
  last_seen-tracking live). **Nog open: de verdeel-kist-mechaniek zelf** (24u-chest die het
  saldo van een opgegeven inactief lid onder de deelnemers verdeelt — details uit te werken).

---

## ⏭️ Sessie (2026-07-18d) — shop live (gems-only), aankoop-tekst, weekly top-20, feedback-mockups

**Alles LIVE op prod + gecommit + gepusht** (`1b1fc72` → `cddda98`; subtree t/m `ed12171`).

- **Dagpicks LIVE** (`bf80d01`) — `SHOP_DAILY_PICKS_LIVE = true`: de 4 dagpicks tonen echte
  **gems** (Ruby/Topaz/Cinnabar/Amber e.d.), **boosters blijven eruit** (`horseshoe_shop_odds_days=0`).
  `SHOP_PERMA_PASS_LIVE` blijft `false` → permanente pas nog grijze teaser. Dagpas koopbaar.
- **Reroll-knop weg van de publieke shop** (`3b8f0b1`) — verscheen toen de picks live gingen;
  hoort enkel in de **admin-secties** (Admin shop preview houdt zijn reroll). `market()` toont
  enkel nog de afteller.
- **Aankoopmelding: platte naam, NIET getagd** (`cddda98`) — `**Naam** bought a/an **X** (gem).`
  (kort even een `<@mention>` geweest → teruggedraaid op user-verzoek; naam als tekst, geen ping).
- **Weekly leaderboard → top 20** (`cddda98`) — `leaderboard_week` limiet 10→20 (filtert al op
  **≥1 coin**, sorteert **DESC** op coins). Basis = **week-earnings** uit `earn_log` sinds de vorige
  zaterdag (chat+daily+chest+geclaimde gifts), **niet** het saldo/all-time — bewust, want all-time
  zou vertekenen door de toegekende coins uit het oude economysysteem. Post **zaterdag 15:00**
  Brussel in **#general** (niet #coins; user vroeg of het 14:00 was — het is 15:00).
- **Uur-overzicht ("⏳ Earners of the last hour")**: weergave was al **alfabetisch** (niet op coins) —
  bewust géén wedstrijd-gevoel; ongewijzigd gelaten.
- **Teaser-hoogte**: de grijze teaser mag iets korter zijn dan de echte kaart (user-akkoord); een
  flex `align-items:stretch` rekt de teaser tot ~16px kort van de dagpas door een aspect-ratio/
  min-content-flexquirk — niet verder achtervolgd. Teaser = uniform grijs vak met gecentreerd 🔒.
- **Feedback-mockups in dev-coins**: alle prod-#coins-berichten (daily, aankoop gem/pas, level-up
  embed + **uitgeschakelde** 🎁-knop, claim-regel, uur-overzicht, weekly) als mockup gepost in het
  dev "coins"-kanaal (`1525189157104648343`) ter tekstcontrole — via een los script met de
  bot-token op de server (geen commando). **Regel bevestigd: alle buitenwereld-tekst = Engels.**

---

## ⏭️ Sessie (2026-07-18c) — levelfix-commando's weg, shop-dag-afteller, boosters uit de dagrotatie

**Alles LIVE op prod + gecommit + gepusht** (`7f0b29d` → `b13ab9e`; subtree t/m `18bb886`).

- **Eenmalige `!levelfix`-commando's verwijderd** (`7f0b29d`) — de correctie was uitgevoerd
  (18b): `!levelfix_preview`/`!levelfix_commit` + `handle_level_preview` + de `lgprev:`-route +
  `correction_for` + `DEV_COINS_CHANNEL_ID` (bot.rs) weg, plus de verweesde db-helpers
  `level_floor`/`gifted_levels`/`has_correction`/`all_earners`. **De echte claim (`lg:` →
  `handle_level_claim` → `claim_level_gift`) blijft ongemoeid** → de 19 al-geposte
  correctie-embeds in prod #coins blijven claimbaar. `level_gifts`-data blijft staan.
- **Shop-dag-afteller** (`d4c7660`) — live-tickende `⏳ New picks in H:MM:SS` naast
  "✨ Today's picks", voor iedereen (client rekent met `Date.now()` → tijdzone-proof). Admin
  houdt zijn ↻ reroll ernaast. **Refresh-moment = 00:00 UTC = 02:00 Brussel (CEST) / 01:00
  (CET)**, want `shop_day() = floor(now/86400)` in UTC.
- **Boosters uit de dagrotatie — gems-only** (`d4c7660`) — `horseshoe_shop_odds_days`: `min`
  1 → 0, met **0 = UIT** (read/write klemmen op de spec-grenzen, dus 0 vergt de min-wijziging).
  **Live op prod op 0 gezet** + dag-cache gewist. Omkeerbaar via ⚙ Settings (terug op N).
- **Daily picks in trekkingsvolgorde** (`d5e64de`) — `daily_shop` heeft PK `(day,item_id)`; de
  lees-query zonder ORDER BY gaf de gems gesorteerd op item_id terug → bij een reroll leek de
  auto-refresh ze te "herordenen". Fix: `ORDER BY rowid` = insertie- = trekkingsvolgorde.
- **Publieke shop = volledig ontwerp met grijze teasers** (`f959683`, `b13ab9e`) — de
  day-pass-only-test (`SHOP_TEST_DAY_PASS_ONLY`) is **vervangen** door het volle ontwerp
  (dagrotatie + Hytale-passen, zoals de Admin shop preview) met twee feature-flags in `web.rs`:
  - `SHOP_DAILY_PICKS_LIVE = false` → 4 grijze textloze 🔒-teasers i.p.v. gems. **Op groen licht
    → `true`** → echte gem-rotatie (+ admin-reroll).
  - `SHOP_PERMA_PASS_LIVE = false` → permanente pas = grijze 🔒-teaser (géén naam/prijs). **Later,
    als de server-mod af is → `true`**. De **dagpas blijft koopbaar**.
  - Afteller staat inline naast de titel (waar de reroll stond). Pas-vakjes gelijke hoogte
    (`.shelf align-items:stretch` + `.slot.soon justify-content:center`). **Dagpicks-rij
    horizontaal gecentreerd** (`.shelf.picks{justify-content:safe center}`); de passen-rij niet.
- **Alle klant/GUI-tekst in het Engels** (`b13ab9e`) — klant-facing was al Engels; resterende
  gerenderde NL → Engels (chestrescue-Discord-replies, log-details deelnemer→participant, admin
  Settings-units, ∞-tooltip, "Nobody has bought anything yet"). Server-logs (`tracing`), panics
  en code-comments blijven NL (niet speler-zichtbaar). **Regel: alle buitenwereld-tekst = Engels.**

### 📌 Open TODO — horseshoe-testprotocol (voor later; verfijnd 2026-07-18d)
De Lucky Horseshoe (permanente booster, bezit = **2 loten** i.p.v. 1 bij een chest-trekking →
`db::chest_weight` = 2 via `owns_horseshoe`) moet eerst getest worden vóór hij terug in de
dagrotatie mag (`horseshoe_shop_odds_days` weer op N). **Geparkeerd op verzoek.** Twee valkuilen
die het naïeve "één live-chest in dev" plan ondermijnen:
- **Dev-guild draait op de prod-`coins.db`** (zelfde bot-instance). Een echte dev-chest schrijft
  dus in prod (join-records, uitbetaling, Faybelle's horseshoe-inventory-rij). "Niks verstoren"
  vergt dus terugdraaien achteraf.
- **Eén trekking bewíjst de 2× niet** (met 2 spelers = 33/67; één sample).

**Afgesproken protocol (klaar om uit te voeren):**
1. **Odds-bewijs = simulatie** (nul risico, al één keer gedraaid 2026-07-18d): replica van de
   draw-logica (`total=Σweights; roll∈[0,total); walk`) over 500k trekkingen → houder wint in élk
   scenario **exact 2×** een niet-houder (2-,3-,5-speler getest). Dít is de echte 2×-toets.
2. **Hands-on e2e (optioneel, voor de flow + de "🍀 Their Lucky Horseshoe doubled the odds!"-regel):**
   `!chest` (bestaat, **dev-guild-only**, spawnt meteen in het huidige kanaal). Reversibel doen:
   vooraf Faybelle de horseshoe geven (DB-insert), Waldstein+Faybelle joinen, ná de trekking
   **alles terugdraaien** (horseshoe weg, uitbetaalde coins + `total_earned` terug, test-logrijen
   wissen) → prod exact ongewijzigd. Per trekking de exacte kansen rapporteren.
3. Daarna balans-oordeel (2× / prijs 120 / zeldzaamheid) → dan pas `horseshoe_shop_odds_days` weer aan.

### 📌 Open TODO — verjaardagen + birthday-chest (geparkeerd 2026-07-18c)
- **Verjaardagen overnemen van MEE6**: de birthday-data die de MEE6-bot bijhoudt opvragen en
  importeren in market (mechaniek nog uit te zoeken — MEE6 API/dashboard-export?).
- **Cadeau = 500 coins** op de verjaardag.
- **Nieuwe chest ontwerpen** speciaal hiervoor (birthday-chest). Ontwerp nog te bespreken.

---

## ⏭️ Sessie (2026-07-18b) — ALLE coins tellen mee voor leveling + gift in uur-overzicht + dubbel-pay-fix

**Alles LIVE op prod + gecommit + gepusht** (commit `fd56e39`; subtree t/m `baeb094`).

**Aanleiding — audit van TimmyThumb.** User meldde: Timmy levelt om 04:08 (claimt 20), krijgt
tegelijk +49 checkin, maar het 05:01-uuroverzicht toont enkel 49 — "klopt niet". Bevinding: Timmy
had **al zijn coins** (`coins 1354 = total_earned 1334 + gift 20`); het overzicht toonde 49 omdat
de level-up-gift **bewust niet** meetelde als verdienste (`earn_log`/`total_earned`). Volledige
reconciliatie over alle 21 leden: **0 mismatches** (`coins = total_earned + geclaimde gifts −
uitgegeven`). Dus geen boekhoudfout — wél een beleidsmismatch.

**User-beslissing:** *alle* coins moeten meetellen voor de level-up (all-time saldo, ongeacht bron)
én het uur-bericht moet **alle** verdiensten tonen. Correctie mag levels ontgrendelen (**optie A**),
maar de dubbele betaling eruit.

**Doorgevoerd (commit `fd56e39`):**
- **`db::credit_earned`** (nieuw): boekt coins **als verdienste** (`coins` + `total_earned` +
  `earn_log`) **zonder** `last_award` aan te raken (dat is enkel de chat-cooldown — een cadeau mag
  die niet resetten; daarom géén hergebruik van `award`). `claim_level_gift` gebruikt dit i.p.v.
  `admin_add_coins`. Gevolg: gift telt mee voor leveling **en** verschijnt in **"⏳ Earners of the
  last hour"**.
- **`handle_level_claim`** draait `maybe_levelup` ná de claim → een gift die een nieuw level
  ontgrendelt wordt meteen opgepikt. **Geen cascade:** een gift = 1,5 % van het saldo, een levelgat
  is altijd ~30-40 % → een cadeau kan nooit zélf een volgend level openen (behalve de grotere
  correctie-gift, zie onder).
- **`correction_for`** slaat levels over die al een echte gift-rij hebben (`db::gifted_levels`,
  nieuw) → **geen dubbele betaling** meer. Timmy: **26** i.p.v. 46 (zijn al-uitgekeerde level-6-gift
  wordt niet nog eens betaald). Totaal correctie blijft **860 coins / 19 leden**.
- Verweesde `admin_add_coins` + `current_coins` verwijderd.

**Data (prod, eenmalig):** TimmyThumb's al-geclaimde gift (20) alsnog in `total_earned` geboekt
(1334 → **1354**), idempotent-veilig. Reconciliatie ná afloop: **0 mismatches / 21**; invariant is
nu `coins == total_earned − spent` (gifts zitten voortaan ín `total_earned`).

**⚠️ Impact op `!levelfix_commit` (nog te draaien):** onder het nieuwe beleid triggert de correctie
precies **één** nieuwe level-up — **Waldstein 9 → 10** (215 duwt hem over drempel 10 = 9078) → krijgt
daar bovenop een verse level-10-gift (~138). Bewust toegelaten (optie A). Alle anderen blijven op hun
level. **Handmatige admin-grants** (Manage → Coins, was `admin_add_coins`) tellen **bewust niet** mee
— aparte, opzettelijke admin-tool.

---

## ⏭️ Sessie (2026-07-18a) — level-up-cadeaus (embed+claim), correctie-inhaalslag, aankoopmeldingen, cosmetica

**Alles LIVE op prod + gecommit + gepusht** (`f1f69ac` → `b205e6e`; subtree t/m `128a253`).

### 1. Level-up-cadeaus — nieuw embed+claim-systeem (`15a5d2f` e.v.)
**Vervangt** het oude auto-uitgekeerde 1%-bonusje (dat enkel in het chat-award-pad vuurde en
door de recente deploy nooit was getriggerd — 0 level-events in `server_log`, terwijl 19 leden
al ≥ level 1 stonden).

- **Voortaan:** bij een level-up post de bot een embed **"🎉 LEVEL UP! 🎉"** in **prod #coins**
  met tekst `<@tag>, you are now level **N**, <variant>` + een **🎁 Claim reward**-knop.
  - Variant = willekeurig uit **5 user-teksten** (`LEVELUP_VARIANTS` in `bot.rs`): *super
    inspiring!* · *terrifically done!* · *be proud of you!* · *you did amazing!* · *lots of
    praise to you!* (allemaal met `!`). **Niet zelf uitbreiden** — user-beslissing.
  - **Enkel de gelevelde** kan claimen (custom_id `lg:{id}`, atomische claim). Andere klikker →
    ephemeral **"Uh-oh! This is not your reward!"**. Bij claim → publiek **"<naam> got X coins
    for the level up."** (de náám, géén ping — bewust anders dan de tag in de embed).
  - **Cadeau = 1,5 % van je saldo** op het moment van de level-up (half naar boven). *(Achterhaald
    op 2026-07-18b: de claim liep via `admin_add_coins` en telde toen niet mee voor `total_earned`;
    nu via `credit_earned` → telt **wél** mee. Zie het 18b-blok bovenaan.)*
- **Gecentraliseerd + zelfhelend:** `maybe_levelup(http, pool, uid, name)` draait na élke
  coin-bron (bericht `bot.rs:~193`, daily `~365`, chest-pop `~1015`, chest-rescue `~562`).
  Marker `coins.gifted_level` = hoogst gepost level → geen dubbele/gemiste embeds; een level-up
  via admin-grant wordt bij de volgende verdienste alsnog opgepikt.
- **Baseline-migratie (eenmalig, `db.rs` init, flag `settings.levelgift_baseline_v1`):** zet bij
  deploy `gifted_level = huidig level` voor bestaande leden, zodat de embed **niet met
  terugwerkende kracht** de hele backlog naar #coins spamt. ✅ Geverifieerd: 0 mismatches.
- **DB:** tabel `level_gifts(id,uid,amount,level,kind,claimed,ts)` + kolom `coins.gifted_level`.
  Helpers in `db.rs`: `level_floor`, `get/set_gifted_level`, `create_level_gift`,
  `claim_level_gift`→`GiftClaim`, `has_correction`, `all_earners`.

### 2. Eenmalige correctie-inhaalslag — ⚠️ WACHT OP GOEDKEURING (nog NIET uitgevoerd)
Beslissing user: **géén** stille bulk-uitkering; wél de gemiste level-ups **replayen** als
één gebundeld cadeau per lid (**optie A**). Basis = **1,5 % van elke bereikte leveldrempel**
(`level_floor(1..cur)`, som). Twee admin-`!`-commando's (`admin_only`, geregistreerd):
- **`!levelfix_preview`** → post **1 sample-embed** naar het **dev "coins"**-kanaal
  (`DEV_COINS_CHANNEL_ID = 1525189157104648343`) met een **preview-knop** die niets uitkeert
  (custom_id `lgprev:{uid}:{amount}`). Verandert niets aan de DB.
- **`!levelfix_commit`** → post de **19 gebundelde correctie-embeds** naar **prod #coins** met
  echte claim-knoppen, zet `gifted_level`, logt `level/correction`. **Idempotent** via
  `level_gifts.kind='correction'` (herhalen doet niets; resumable).

**Dry-run (prod, 2026-07-18):** 19 leden, **860 coins** totaal. Waldstein+FayBelle 215 (lvl 9),
DinDin 130 (8), thatladtag 78 (7), Yâ-Ôd 46 (6), 6×26 (lvl 5), Rivi+Ezuldor 7 (3), 6×1 (lvl 1).

**→ Volgende sessie / user-actie:** user typt `!levelfix_preview` in dev-coins → keurt de look →
`!levelfix_commit` voor prod. Pas dán is de correctie uitgekeerd.

### 3. Aankoopmeldingen in #coins — ALLE items (`2e335d2`, `b205e6e`)
Web-aankoop postte niets naar Discord. Nu wel, via **nieuwe** `discord_rest::send_channel_message`
(REST-POST, los van de gateway-bot) + helper `announce_purchase`/`purchase_announce` in `web.rs`,
in **beide** koop-branches (pas + gem/booster). Async gespawnd (blokkeert de redirect niet, een
Discord-hapering breekt de aankoop niet). Omgevingsbewust (`coins_channel`: dev→dev-coins,
prod→prod #coins). Teksten: gems **"<naam> bought a/an <Gem> gem."**, boosters/passen
**"<naam> bought a/an <Naam>."** (a/an op de eerste letter).

### 4. Cosmetica (`f1f69ac`, `7bd1cf5`)
- **Duizendtal-punt** in de shop-prijzen (`dots()` in `web.rs`): 7.777, 20.000.
- **"Basic Gems"-titel + kaarten gecentreerd**; titel in de sier-font **"Spicy Sale"**
  (1001fonts, gratis comm.) — ingebakken via `include_bytes!`, geserveerd op
  `/fonts/spicy-sale.ttf`, `@font-face` + class `shelf-title.fancy`.
- **Permanente pas als grey-out verzamelvakje** op de Boosts-tab (naast de boosters). Blijft
  `category='boost'` → **NOOIT** in de rnd-korf, altijd gewoon in de shop; onthult per koper
  (`has_perma_access`).
- **Boosts-tab toont geen Hytale-naam meer** (naam + "No Hytale pass yet."-placeholder geschrapt).
- **`shop_offers`-docstring veralgemeend**: de 1/N-booster-loting geldt voor álle toekomstige
  boosters (category='booster') — automatisch 1 slot per N dagen, willekeurig één gekozen. De
  rnd is zuiver bevonden via simulatie (2M dagen → 7,117 % ≈ 1/14; grillige gaps, mediaan 10).

### Open / nog te beslissen
- **`!levelfix_commit` nog te draaien** na preview-goedkeuring (zie §2).
- Level-up embed **knop-label**: nu "Claim reward" — user mag nog kiezen voor enkel 🎁.
- Multi-level-catch-up going-forward geeft 1 embed **per** gepasseerd level (zeldzaam).

---

## ⏭️ Sessie (2026-07-17f) — polish: purse-loop, booster-image2, Basic Gems

**Alles LIVE + gecommit + gepusht** (`133e6d9`, `39a2210`; subtree t/m `4e34bfc`).

- **Purse-afteller herhaalde zich** (`133e6d9`). De shop auto-refresht elke 5s met
  `location.reload()` → herlaadt `/market?…&from=X` → server rendert weer `data-from=X`
  (oud saldo) → afteller springt terug en telt opnieuw af. **Fix**: zodra de animatie
  `from` gelezen heeft, strippen we `from` uit de URL (`history.replaceState`), zodat de
  reload een schone `/market` opvraagt → `from==saldo` → geen herhaling. `ok`/`err` blijven.
- **Booster toonde image2 niet** (`133e6d9`). `booster_slot` op de Boosts-tab kreeg de
  tweede afbeelding (kleinere `thumb2` onder de titel), zoals de shop-kaart.
- **Gems → één compacte set "Basic Gems"** (`39a2210`). De 3 schap-rijen
  (primary/secondary/prism) zijn samengevoegd tot één `.shelf wrap`-set met titel
  **"Basic Gems"** (werktitel): rij vult zich en wrapt pas als hij vol is → zo weinig
  mogelijk rijen. Schap-volgorde behouden; schappen/shop-indeling ongemoeid.

## ⏭️ Sessie (2026-07-17e) — Lucky Horseshoe = permanent verzamel-item

**LIVE op prod + gecommit** (`2aa2005`, deploy 21:47). Nog te pushen via
`git subtree push --prefix=market market-gh main`.

**Herontwerp (user):** de horseshoe was een verbruikbare booster (Use → dubbele kans bij de
*volgende* chest → op). Nu een **PERMANENT verzamel-item**: koop 1×, daarna **altijd** dubbele
kans op de treasure chest. Getoond als **grey-out-slot op de Boosts-tab** zoals de gems
(vergrendeld "???" → onthuld na koop). **Geen Use-knop.**

- **Effect = eigendom.** `db::chest_weight` leest nu `owns_horseshoe` (inventory ⋈ items
  category='booster') i.p.v. de verbruikbare `chest_luck`-vlag → weight 2. Enkel **Fortuna's
  Favour** (de enige chest); een later ander chest-type dat dit niet wil, roept `chest_weight`
  gewoon niet aan. `bot.rs` verbruikt niets meer na een chest.
- **Koop 1×.** `purchase()` behandelt 'booster' als 'inventory' (max één). Prijs **7777**
  (seed-default + prod-item id 58 gezet). Shop toont een gekochte booster als **Owned**.
- **Zeldzaam in de shop.** `shop_offers` is nu **gems-only** + een aparte booster-roll: de
  horseshoe pakt met kans **1/N per dag** één slot. `N` = nieuwe setting
  **`horseshoe_shop_odds_days`** (default **14**, groep **Shop** in ⚙ Settings) → live tunbaar.
  ✅ Handelt de open todo's "waarschijnlijkheid instellen" **en** "testen" af.
- **Opgeruimd:** `/use/booster`-route + handler, `has_chest_luck`/`clear_chest_luck`/
  `activate_horseshoe`/`owned_booster_items`, de active-banner. `chest_luck`-kolom blijft
  (harmloos, refund/reset schrijven er nog een 0 in).
- **Dry-run-tests** (`horseshoe_dryrun`): eigendom→dubbel gewicht, koop-1×, shop-loterij bij N=1.
  7 tests groen. **Prod:** niemand bezat er al één (geen gratis perk), geen `chest_luck` actief.

⚠️ **Nog te bekijken:** de horseshoe is echte-koop-getest? (koop op prod voor 7777 → verschijnt
ungreyed op Boosts-tab → chest-kans verdubbelt). En de daily-shop-rotatie van vandaag kan de
horseshoe nog uit de oude uniforme trekking bevatten (id 58 zat in enkele gecachete dagen).

## ⏭️ Sessie (2026-07-17d) — gem-swap self-healing + Hytale-naam éénmalig

**Beide LIVE op prod + gecommit** (`e5413aa`, `e27528b`; deploys 21:03 + 21:17). Nog te pushen
via `git subtree push --prefix=market market-gh main`.

**(A) Gem-Use verwijderde de vorige kleurrol niet.** `use_gem` trok enkel de gem in die in
`coins.equipped_gem` stond; bij een lege/stale tracking bleef een oude kleurrol (bv. Ruby) op het
lid staan en toonde Discord díe kleur (hoogste gekleurde rol wint) i.p.v. de nieuwe. **Fix**: de
bot leest nu de **échte rollen op het lid** (`Discord::all_roles` + `member_role_ids`) en haalt
**élke** kleur-gem-rol behalve de nieuwe weg — self-healing, ook voor oude test-/handmatige
toekenningen. Niet-gem-rollen (Flowerborn, Hytaler) blijven. Het gekochte item blijft in de
inventory. Selectie zit in de pure `other_gem_role_ids` (web.rs) met dry-run-tests
(`gem_swap_dryrun`) + `gem_color_dryrun` (db.rs) — draaien mee in `cargo test`. **Faybelle-test
bevestigd werkend.** ⚠️ Als een revoke ooit faalt: bot-rol *Fortuna* moet **boven** de kleurrollen
staan in de hiërarchie.

**(B) Hytale-naam is nu éénmalig (anti-pas-doorgeven).** Een lid gaf zijn naam door → whitelist;
kon die naam nadien **wijzigen** en zo z'n pas aan iemand anders doorgeven. Dichtgezet op drie
plekken: (1) boosts-sectie toont de naam **read-only** i.p.v. een Update-formulier; (2) de
`/hytale/name` update-route + handler + `HytaleNameForm` **verwijderd** (POST → 404, lokaal
geverifieerd); (3) `purchase()` zet de meegestuurde naam **enkel als er nog géén is**
(first-set-only, ook tegen een zelf-gemaakte POST bij een herkoop). Eerste keer instellen gebeurt
onveranderd via het pas-**koopformulier**. Naam corrigeren (typfout) = enkel nog admin/DB.

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

- **⚠️ Day Pass staat nog op TEST** *(hoogste prioriteit van deze lijst)*: op de **prod-DB** heeft
  de **Meadowland Day Pass** (id 21) `duration = 7200`s (2u) en `price = 1`. Moet naar **86400s (24u)**
  + een echte prijs. **Niet acuut** — de shop is gate-d (enkel admins raken in `/market`) — maar
  **moet rechtstaan vóór de gate opengaat**. Zetbaar via **Manage Shop**.
  ✅ **Permanent Pass** (id 22) prijs staat sinds **2026-07-17f** op **20000** (user).
- **🚫 Dubbele pas voorkomen — Twitch-kant** *(user 2026-07-17f)*: een lid mag géén dag- of
  permanente pas kunnen kopen als het er al één heeft. **Via de site is dat geregeld** (`db::purchase`:
  perma blokkeert een tweede perma én een dagpas; een lopende dagpas blokkeert een tweede). **Maar
  Twitch omzeilt dit**: `twitch.rs::on_redeem` roept rechtstreeks `db::grant_day_whitelist` aan
  (geen `purchase`, geen pas-check) én draait onder een **aparte Twitch-pseudo-id**, los van de
  Discord-uid — dus een Twitch-redeem stapelt tijd bovenop, ook als het lid al (permanent) toegang
  heeft. Nog te doen: in de Twitch-flow een bestaande (site-)pas herkennen en dan weigeren/refunden.
  ⚠️ Complicatie: Twitch-identiteit ≠ Discord-identiteit; koppelen kan wellicht via de **Hytale-naam**
  (die staat in beide grants).
- **Permanent Pass `role_id` is leeg** op prod → `Use` zet enkel `perma_access`, kent **geen
  Discord-rol** toe. Invullen via Manage Shop.
- **Shop-graphics**: de shop toont nu álle schappen; gems/boosters zonder afbeelding renderen als
  gekleurde bol. Echte item-graphics maken vóór de shop **members-zichtbaar** wordt (site-gate weg).
- ~~**Prijzen/economie balanceren**~~ → **AFGEHANDELD op 2026-07-17** (user): het ⚙ Settings-panel
  werkt en we sturen live bij indien nodig. De coin-instroom ging weliswaar +53% t.o.v. de oude
  prijs-ijking, maar dat is nu een **live tuning-kwestie** (gewicht +4/+5 of msg-cooldown via
  ⚙ Settings), geen openstaand bouwwerk meer. Gem-prijzen 1000–11000 (2026-07-16), Lucky Horseshoe 120.
- ~~**Lucky Horseshoe — waarschijnlijkheid instellen**~~ → **KLAAR 2026-07-17e**: `horseshoe_shop_odds_days`
  (⚙ Settings → Shop, default 14) regelt hoe zeldzaam hij in de dagshop verschijnt (1/N per dag).
- ~~**Lucky Horseshoe — testen**~~ → dry-run-tests groen (`horseshoe_dryrun`); **echte-koop op prod
  nog te doen** (koop 7777 → ungreyed op Boosts-tab → chest-kans ×2). Zie sessie 2026-07-17e.
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
