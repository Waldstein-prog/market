# Meadow Market — Economy-ontwerp (Fase II)

> Werkdocument. Gestructureerd uit de brainstorm van Waldstein (Discord, 2026-07-09).
> **Nog niet definitief** — punten met `[?]` moeten bijgestuurd worden.
> Geen implementatie zolang dit niet vastligt.

---

## 1. Rollen & toegang

- **Flowerborn** = de leden-rol. Alleen Flowerborns doen mee aan de economy:
  verdienen coins, hebben een account, kunnen hun inventory/market bekijken.
- **Niet-Flowerborns**: website toont enkel een bericht ("follow the rules").
  Geen account, geen coins.
- `[?]` "Flowerborn" vervangt de huidige doelrol **Hytaler** — of staat het los?

---

## 2. Coins verdienen (bronnen)

| Bron | Regel | Status |
|------|-------|--------|
| **Chatten** | 1–3 coins/bericht, cooldown 30s per lid | ✅ GEBOUWD & LIVE |
| **Daily-knop** | Geeft daily coins; random bedrag, **hoger bij streak** | te bouwen |
| **Treasure chests** | Event in general-chat, coins voor "lucky winner(s)" | te bouwen (zie §7) |

- Munt-naam: **Meadow Coins**.

---

## 3. De winkel — "Meadow Market"

Overzicht van alle koopbare items. Elk item geeft (meestal) een **Discord-rol**.

- **Dagelijkse rotatie**: elke dag andere dingen te koop. **4 slots**, met (deels)
  random inhoud en **verschillende prijzen**.
- **Eenmalig koopbaar**: kristallen en lucky-spullen kunnen maar **één keer** gekocht
  worden → daarna **greyed-out** ("je hebt dit al").
- **Magic slot**: af en toe verschijnt hier iets speciaals.
- **Item-gedrag na aankoop** — twee soorten:
  1. **Direct actief** ("begint meteen te draaien") — bv. een booster met timer.
  2. **Naar inventory** — bv. collectibles/kristallen; blijven bestaan, later te
     activeren.
- **Settings per item moeten adjustable zijn** (prijs, type, effect, duur…).

### Item-categorieën (uit de brainstorm)

- **Rollen / passen** — permanent. Bv. permanente Hytale-pass.
- **Boosters**:
  - *Extra lucky voor treasures* — "horseshoe", werkt **random / af en toe**.
  - *Perm Hytale* (permanent) en *Temp Hytale* (tijdelijk, **met countdown-timer**).
  - *Summon-a-chest pickaxe* — tijdelijk werkend item dat chests in de chat spawnt
    (zie §7).
- **Collectibles / kristallen** — gele, rode… → gaan naar inventory, per kleur/soort
  gebundeld op "schappen".
- `[?]` **Plushies-schap** = expliciet **voor later**.

---

## 4. Inventory & collectibles

- **Schappen i.p.v. losse vakjes**: gelijksoortige items groeperen — alle gele
  kristallen samen, alle rode samen, enz.
- Visueel: **vakjes met afgeronde hoekjes**, stijl à la de `pod`-winkel.
- **Item klikken in inventory → geeft die Discord-rol; het item blijft bestaan**
  (activeren "verbruikt" het niet).
- `[?]` Kunnen sommige items wél verbruikt worden (bv. de chest-pickaxe), of blijft
  alles permanent staan?

---

## 5. Website-account (per Flowerborn)

- **Login = Discord OAuth2** (BESLIST 2026-07-10). Flow:
  1. Embed-**link-knop** in vast kanaal → site → redirect naar Discord's login.
  2. User autoriseert → Discord geeft eenmalige code terug → server wisselt om voor
     de **echte Discord-ID** (niet te vervalsen, komt van Discord zelf).
  3. Server checkt via de bot of die ID de **Flowerborn-rol** heeft → coins-overzicht;
     zo niet → regels-pagina.
  4. Server zet een **sessie-cookie** (`HttpOnly`/`Secure`/`SameSite`), lang geldig
     (30–90 dagen) → voelt als "één keer inloggen".
- **Data-isolatie**: server toont enkel data van de ingelogde ID; niemand ziet
  andermans coins, niets persoonlijks zichtbaar zonder OAuth te doorlopen.
- **Nodig**: Client ID + Secret (de bot-app heeft die al), `identify`-scope, een
  redirect-URI, en een **HTTPS-adres** (zie afhankelijkheid hieronder).
- **Coin-page**: hoeveel coins **verzameld** + hoeveel al **uitgegeven** (knop).
  - **Leaderboard** op de website.
  - **Kroontje-icoon**: "max ooit" en "max nu" op de coin-page.
- **Tabs**:
  1. **Inventory** (§4) — je schappen/collectibles, klik = rol.
  2. **Meadow Market** (§3) — de winkel, koopbare items.
- `[?]` **Later**: elkaars profiel kunnen bekijken — zichtbaar voor andere
  Flowerborns via een **toggle**.

---

## 6. Discord-embed & kanalen

- Een **embed in een kanaal** met **knoppen** als toegangspoort tot de economy.
  Knoppen o.a.: **Daily**, **Inventory**, **Market**.
- Alleen **Flowerborns** kunnen via de embed hun account openen.
- **Command** om in te stellen **naar welk kanaal** de output gaat.
- **Daily-knop** → post een bericht in het **loot-/daily-kanaal** met de daily coins.
- **Vast embed-kanaal**: de uitkomst van de daily "komt ergens" terecht (vast kanaal).
- **BESLIST 2026-07-10**: de embed is een **launcher** — één vaste, altijd-zichtbare
  embed met een **link-knop** naar de website. De website is het echte scherm
  (coins/inventory/market); de embed is ingang + (later) meldingen.

---

## 7. Events — treasure chests

Twee mechanismen genoemd (mogelijk twee aparte features):

- **A. Via booster-item** ("Summon-a-chest pickaxe", tijdelijk):
  spawnt **3 treasure chests** in de chat met Meadow Coins → **2 winnaars** (niet 1).
- **B. Activiteits-trigger**: als **3 personen binnen 10 min** in **general** chatten
  → spawnt een chest. Enkel in general → **1 lucky winner**.
- **Kansverhoging**: een **Lucky-rol** of **sub tier 2/3** geeft **meer kans**, en is
  **stackable**.
- `[?]` Zijn A en B echt twee losse systemen, of één mechaniek met varianten?

---

## 8. Koppelingen (Twitch / Discord)

- "Koppeling bestaat" — er is al een **rollenkanaal** dat Twitch ↔ Discord linkt.
- FayBelle-reminder: **uitleg rollenkanaal + link Twitch-Discord** nog te documenteren.
- Sub-tiers (Twitch) → **Lucky-kans** in de chest-events (§7).
- `[?]` Raakvlak met het bestaande **Twitch-ticket-idee** (Twitch-redeem → tijdelijke
  Hytale-whitelist) uit het tale-project?

---

## 9. Branding

- Winkel: **Meadow Market**. Bot: **MeadowMarketBot** (bestaat al).

---

## 10. Belangrijkste open vragen (bij te sturen)

1. **Flowerborn vs Hytaler** — nieuwe naam of aparte rol? (§1)
2. ~~**Login**~~ — ✅ BESLIST: Discord OAuth2, embed-launcher + sessie-cookie. (§5, §6)
3. ~~**Website vs embed**~~ — ✅ BESLIST: website = scherm, embed = launcher. (§6)
4. **Treasure chests** — A en B apart of samen? (§7)
5. **Item-verbruik** — blijft alles permanent, of zijn er consumables? (§4)
6. **Daily-streak** — hoe telt een streak, en wat is het bereik van het random bedrag?
7. **Prioriteit/volgorde** — wat bouwen we eerst na de chat-coins? (Daily? Market?
   Inventory? Embed?)

## 11. Technische afhankelijkheid — domein + TLS

OAuth2 vereist een **HTTPS redirect-URI** in prod. De site draait nu op kaal
`http://167.235.142.113:8700` ("URL later"). Vóór de login-flow live kan:
- **domein** koppelen (A-record naar de VPS) + **TLS** via **Caddy** (auto-cert).
- Redirect-URI registreren in de Discord Developer Portal.
Dit blokkeert OAuth in prod, maar niet het lokaal bouwen/testen (Discord staat
`http://localhost` toe als redirect voor dev).
