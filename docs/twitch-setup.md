# Twitch-pas opzetten (channel-points-redeem → Hytale-whitelist)

Een kijker doet een channel-points-redeem op het kanaal van de streamer en typt zijn
exacte Hytale-naam in de invoerprompt. Market schrijft een grant in
`coins.db.hytale_whitelist` (`user_id = "twitch:<id>"`, stapelend) en stuurt de kijker
een **whisper** (Twitch-DM) met de duur en het serveradres. De tale-bot whitelistet die
naam read-only op de server. Dit luik raakt de Hytale-FIFO **niet** aan.

> **De streamer bezit de reward, market niet** (omslag 2026-08-03). Market maakt of
> beheert geen rewards meer; het herkent de juiste redeem aan de **titel**. Gevolg:
> **terugbetalen gebeurt manueel** in de Twitch-wachtrij — Helix laat een app enkel
> redemptions annuleren van rewards die ze zélf maakte. Het logboek (Manage → 📜 Log,
> 🟣 twitch) zegt wanneer dat nodig is.

---

## 1. De reward aanmaken (streamer, in Twitch)
Creator Dashboard → *Viewer Rewards → Channel Points → Manage Rewards & Challenges* →
**New Custom Reward**:
- **Titel** en **kost**: vrij te kiezen — de titel moet later letterlijk in market staan.
- **Require Viewer to Enter Text**: **AAN** (zonder invoerveld kent market geen Hytale-naam).
- De prompt-tekst is van de streamer; laat er duidelijk in staan dat er enkel de exacte
  Hytale-naam in moet.

Randvoorwaarde: het account moet **Twitch Affiliate** of Partner zijn, anders bestaan er
geen channel points.

## 2. Market instellen (Manage → ⚙ Settings, groep *Twitch-redeem → Hytale-pas*)
Alles hier werkt **live** — geen deploy, geen herstart.

| Veld | Betekenis |
|---|---|
| **Reward-titel (tijdelijke pas)** | Exact de titel uit stap 1 (hoofdletters/spaties eromheen maken niet uit). **Leeg = market negeert alle redeems.** |
| **Duur van de pas** | Uren toegang per redeem. Twee keer redeemen stapelt op. |
| **Whisper naar de kijker** | Het privébericht na een geslaagde redeem. Plaatshouders `{uren}` en `{naam}`. **Zet hier het serveradres in** — zonder adres geraakt de kijker er niet op. Leeg = geen bericht. |
| **Reward-titel (permanente pas)** | Optionele tweede reward die permanente toegang geeft. Leeg = die redeem bestaat niet. |
| **Whisper (permanente pas)** | Idem, met `{naam}` (geen `{uren}`). |

Klopt de titel niet exact, dan gebeurt er niets en staat in de log:
`Twitch-redeem '<titel>' … genegeerd — komt niet overeen met de ingestelde reward-titel(s)`.
Bij de start logt market ook alle reward-titels die het kanaal heeft.

## 3. Twitch-app registreren (eenmalig, ontwikkelaar)
1. <https://dev.twitch.tv/console/apps> → **Register Your Application**.
2. **OAuth Redirect URLs** → exact: `http://localhost:17563`
3. Noteer de **Client ID** en genereer een **Client Secret**.

## 4. Creds in `secrets.json` (naast de binary)
```json
{
  "twitch_enabled": true,
  "twitch_app_id": "CLIENT_ID",
  "twitch_app_secret": "CLIENT_SECRET"
}
```
Meer staat er niet meer in: titels, duur en teksten wonen in de Settings-pagina.

## 5. OAuth-token van de streamer (eenmalig)
⚠️ **De whisper vereist een nieuwe scope** (`user:manage:whispers`). Een token van vóór
2026-08-03 heeft die niet — dan lukt de whisper niet en zie je een 401 in de log. De
stappen hieronder moeten dus één keer opnieuw, ingelogd op het **streamer**-account.

**Stap A — autoriseren (browser).** Vul de Client ID in en open:
```
https://id.twitch.tv/oauth2/authorize?client_id=CLIENT_ID&redirect_uri=http://localhost:17563&response_type=code&scope=channel%3Aread%3Aredemptions+user%3Amanage%3Awhispers
```
→ **Authorize**. De browser springt naar `http://localhost:17563/?code=XXXX&scope=…` (die
pagina laadt niet — normaal). Kopieer de waarde **`code=XXXX`**.
> De `code` vervalt na enkele minuten en is eenmalig. Foutmelding? Herhaal stap A.

**Stap B — code inwisselen (curl).**
```bash
curl -s -X POST https://id.twitch.tv/oauth2/token \
  -d client_id=CLIENT_ID -d client_secret=CLIENT_SECRET \
  -d code=XXXX -d grant_type=authorization_code \
  -d redirect_uri=http://localhost:17563
```

**Stap C — tokens-bestand** naast de binary (`market/twitch_tokens.json`):
```json
{"access_token":"…","refresh_token":"…"}
```
Market ververst dit daarna zelf. Enkel `refresh_token` is strikt nodig om te starten.

### ⚠️ Twitch-eisen voor whispers
- Het **zendende account** (de streamer) moet een **geverifieerd telefoonnummer** hebben,
  anders geeft Twitch **401** en komt er geen bericht aan.
- De **ontvanger** moet whispers van niet-gevolgden toelaten, anders **403**. Daar valt van
  onze kant niets aan te doen.
- Max **500 tekens** voor iemand die je nog nooit geschreven heeft; market kapt af op 500.
- Mislukt de whisper, dan is de **toegang toch toegekend** — enkel het bericht ontbreekt.

## 6. Starten & verifiëren
```bash
cd market
MARKET_WEB_ONLY=1 cargo run          # web_only = geen Discord-gateway (geen dubbele coins)
```
Verwacht in de log:
```
Twitch-luik actief — kanaal=<login>, reward-titel='…', perma-titel=(uit), pas=2u
Twitch-rewards op het kanaal: '…', '…'
Twitch EventSub: geabonneerd op alle reward-redemptions van het kanaal
```

## 7. Testen met een 2e account
1. Redeem de reward met een **tweede Twitch-account** en typ een Hytale-naam (`TestSpeler`).
2. Verwacht: een whisper met de ingestelde tekst, en in de DB:
   ```bash
   sqlite3 market/coins.db "SELECT user_id, hytale_name, expires FROM hytale_whitelist;"
   ```
   → rij `twitch:<id> | TestSpeler | <epoch ~now + N uur>`.
3. Redeem nogmaals → `expires` stapelt; **de getypte naam wordt genegeerd** (de naam ligt na
   de eerste keer vast op dat Twitch-account, tegen doorgeven aan derden). Een foute naam
   ruim je zelf op: `DELETE FROM hytale_whitelist WHERE user_id='twitch:<id>'`, of de pas
   intrekken via het Hytale-panel.
4. Redeem met een lege/rare naam → **geen** grant, wel een `🚫 twitch reject`-regel in
   Manage → 📜 Log. **Betaal die redeem manueel terug** in de Twitch-wachtrij.

### Mock-test zonder Twitch-account
`bash docs/twitch_e2e.sh` — start de Twitch-CLI EventSub-mock + market in mock/web-only
(op poort 8701, dus naast een draaiende market) en injecteert vier redemptions. Bewijst
end-to-end: een vreemde titel wordt genegeerd, een ongeldige naam geeft geen grant, een
geldige geeft de duur uit de settings + de whisper-tekst, en de perma-titel geeft
`expires = NULL`.
