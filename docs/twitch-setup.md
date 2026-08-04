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
| **Whisper bij een afwijkende naam** | Bericht als de kijker bij een **volgende** redeem een **andere** Hytale-naam invult dan de naam die al aan zijn Twitch-account vastzit. Er wordt dan **géén tijd toegekend** — betaal de punten manueel terug. `{naam}` = de vastgezette naam. Leeg = geen bericht (de weigering blijft). |
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

> **Stand op prod (2026-08-04):** de app-registratie bestaat al — dezelfde als die van de oude
> tale-bot (`app_id` `f70589odg5k0v76e1o0qrbzmbs8xw9`). ID + secret staan in
> `/opt/market/secrets.json`, met `twitch_enabled: false` tot er een tokens-bestand is.
> Geldigheid bevestigd via een `client_credentials`-token. Enige blokker is nu stap 5.

## 5. OAuth-token van de streamer (eenmalig)
⚠️ **De whisper vereist een nieuwe scope** (`user:manage:whispers`). Een token van vóór
2026-08-03 heeft die niet — dan lukt de whisper niet en zie je een 401 in de log. De
stappen hieronder moeten dus één keer opnieuw, ingelogd op het **streamer**-account.

**Stap A — autoriseren (browser).** Ingelogd op het **streamer**-account, open:
```
https://id.twitch.tv/oauth2/authorize?client_id=f70589odg5k0v76e1o0qrbzmbs8xw9&redirect_uri=http://localhost:17563&response_type=code&scope=channel%3Aread%3Aredemptions+user%3Amanage%3Awhispers&force_verify=true
```
`force_verify=true` toont altijd het toestemmingsscherm mét accountnaam — zo autoriseer je niet
per ongeluk met een verkeerd ingelogd account. (Voor een andere app-registratie: vervang de
`client_id`.)
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
3. Redeem nogmaals met **dezelfde** naam (hoofdletters maken niet uit, leeg laten mag ook) →
   `expires` stapelt. De naam ligt na de eerste keer vast op dat Twitch-account, tegen
   doorgeven aan derden.
   Redeem je met een **andere** naam, dan wordt er **niets** toegekend: de kijker krijgt de
   whisper "afwijkende naam" en er komt een `🚫 twitch name_mismatch`-regel in het logboek.
   **Betaal die redeem manueel terug.** Wil je de vastgezette naam vrijgeven (typefout bij de
   eerste keer), trek de pas dan in via het Hytale-panel, of
   `DELETE FROM hytale_whitelist WHERE user_id='twitch:<id>'`.
4. Redeem met een lege/rare naam → **geen** grant, wel een `🚫 twitch reject`-regel in
   Manage → 📜 Log. **Betaal die redeem manueel terug** in de Twitch-wachtrij.

### Mock-test zonder Twitch-account
`bash docs/twitch_e2e.sh` — start de Twitch-CLI EventSub-mock + market in mock/web-only
(op poort 8701, dus naast een draaiende market) en injecteert vijf redemptions. Bewijst
end-to-end: een vreemde titel wordt genegeerd, een ongeldige naam geeft geen grant, een
geldige geeft de duur uit de settings + de whisper-tekst, de perma-titel geeft
`expires = NULL`, en een afwijkende naam laat de bestaande tijd ongemoeid + logt
`name_mismatch`.
