# Twitch-pas testen (channel-points-redeem → Hytale-whitelist)

Het Twitch-luik zit nu in **market** (Rust, `src/twitch.rs`). Een kijker doet de
channel-points-redeem **"Hytale-ticket (24u)"** en typt (de 1e keer) zijn exacte
Hytale-naam → market schrijft een grant in `coins.db.hytale_whitelist`
(`user_id = "twitch:<id>"`, +24u stapelend). De tale-bot whitelistet die naam al
read-only op de server. Dit luik raakt de Hytale-FIFO **niet** aan.

We testen eerst op **jouw eigen Twitch-account als dev** (`environment=dev` → reward
kost **0** channel points = gratis redeemen).

---

## 0. Randvoorwaarde: Affiliate
Je account moet **Twitch Affiliate** (of Partner) zijn, anders bestaan er geen channel
points en kun je niets redeemen. Check: Twitch → Creator Dashboard → *Viewer Rewards →
Channel Points* moet aan te zetten zijn.

## 1. Twitch-app registreren (eenmalig)
1. <https://dev.twitch.tv/console/apps> → **Register Your Application** (ingelogd op je
   dev-account).
2. **OAuth Redirect URLs** → exact toevoegen: `http://localhost:17563`
3. Category maakt niet uit. → **Create**.
4. Noteer de **Client ID** en genereer een **Client Secret**.

## 2. Creds in `secrets.json` (naast de binary)
Voeg deze top-level velden toe aan `market/secrets.json`:
```json
{
  "twitch_enabled": true,
  "twitch_app_id": "JOUW_CLIENT_ID",
  "twitch_app_secret": "JOUW_CLIENT_SECRET",
  "environment": "dev"
}
```
(De bestaande Discord-velden laat je staan.) `environment: "dev"` → reward-kost 0.

### Optioneel: permanente-pas-reward (naast de dagpas)
De bot maakt standaard één **dagpas**-reward (`twitch_reward_title`, default "Hytale-ticket
(24u)"). Wil je óók een **permanente-pas**-redeem, voeg dan een titel toe — **zonder titel
bestaat die tweede reward niet** (we verzinnen bewust geen speler-zichtbare tekst):
```json
{
  "twitch_perma_reward_title": "JOUW EXACTE TITEL",   // bv. "Hytale-pas (permanent)"
  "twitch_perma_reward_cost": 5000                     // channel points; leeg ⇒ dev 0 / prod 5000
}
```
Een perma-redeem geeft dezelfde naam-vastzet-flow, maar de whitelist-grant is **permanent**
(`expires = NULL`, geen afteller). Redeem-je later een dagpas terwijl je al permanent bent,
dan blijft permanent staan (geen downgrade).

## 3. OAuth-token aanmaken (eenmalig, browser + één curl)
Ingelogd op **jouw** Twitch-account. Doe dit zelf in een terminal zodat je secret
nergens gedeeld wordt.

**Stap A — autoriseren (browser).** Vul je Client ID in en open deze URL:
```
https://id.twitch.tv/oauth2/authorize?client_id=CLIENT_ID&redirect_uri=http://localhost:17563&response_type=code&scope=channel%3Amanage%3Aredemptions+user%3Awrite%3Achat
```
→ **Authorize**. De browser springt naar `http://localhost:17563/?code=XXXX&scope=…`
(die pagina laadt niet — normaal). Kopieer de waarde **`code=XXXX`** uit de adresbalk.
> De `code` vervalt na enkele minuten en is eenmalig. Foutmelding? Herhaal stap A.

**Stap B — code inwisselen voor tokens (curl).**
```bash
curl -s -X POST https://id.twitch.tv/oauth2/token \
  -d client_id=CLIENT_ID \
  -d client_secret=CLIENT_SECRET \
  -d code=XXXX \
  -d grant_type=authorization_code \
  -d redirect_uri=http://localhost:17563
```
De JSON-respons bevat `access_token` en `refresh_token`.

**Stap C — tokens-bestand schrijven** naast de market-binary (`market/twitch_tokens.json`):
```json
{"access_token":"…","refresh_token":"…"}
```
De bot ververst dit daarna zelf. Enkel `refresh_token` is strikt nodig om te starten.

## 4. Starten & verifiëren
```bash
cd market
MARKET_WEB_ONLY=1 cargo run          # web_only = geen Discord-gateway (geen dubbele coins)
```
Verwacht in de log:
```
Twitch-luik actief — kanaal=<jouw login>, reward='Hytale-ticket (24u)', pas=24u, chat=aan
Twitch EventSub: geabonneerd op reward-redemptions
```
De reward **"Hytale-ticket (24u)"** verschijnt nu in je kanaalpunten-menu (kost 0 in dev).

## 5. Testen met een 2e account
1. Log met een **tweede Twitch-account** in op je kanaal, redeem de reward, typ een
   Hytale-naam (bv. `TestSpeler`).
2. Verwacht: chatbevestiging `✅ 24u toegang voor Hytale-naam 'TestSpeler' (naam nu vastgezet)`.
3. Controleer de grant:
   ```bash
   sqlite3 market/coins.db "SELECT user_id, hytale_name, expires FROM hytale_whitelist;"
   ```
   → rij `twitch:<id> | TestSpeler | <epoch ~now+86400>`.
4. Redeem nogmaals → `expires` stapelt (+24u); getypte naam wordt genegeerd (blijft vast).
5. Redeem met een lege/rare naam → redemption **Canceled** in Twitch (punten terug) + uitleg
   in de chat.

### Permanente-pas testen (indien `twitch_perma_reward_title` gezet)
Naast de dagpas verschijnt de tweede reward. Redeem ze (met je vastgezette naam):
- Verwacht: chatbevestiging `✅ permanente toegang voor Hytale-naam '<naam>'`.
- In de DB staat `expires = NULL` voor `twitch:<id>` (permanent, geen afteller).

Snelle mock-check zonder Affiliate (dev): `bash docs/perma_e2e.sh` — start de Twitch-CLI
EventSub-mock + market in mock-modus en injecteert een dag- én perma-redeem (`-i mock_reward`
resp. `-i mock_perma_reward`). Bewijst de routing dag→24u vs. perma→NULL end-to-end.

## Wat verandert er voor prod later
Op de VPS: `secrets.json` met de **prod-streamer**-creds + `environment: "prod"`
(reward-kost 1500), `twitch_tokens.json` van de streamer, en één keer `systemctl restart market`.
Zet `twitch_perma_reward_title` (+ `twitch_perma_reward_cost`) als je de permanente-pas-redeem
live wil. De tale-bot pikt de grants read-only op (staat al klaar).
