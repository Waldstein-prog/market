# Handover — market (Meadow Market) — 2026-07-09

## Wat dit is
Project **`market`** (lab-poort **8700**): een **Discord-economysysteem** met gekoppelde
website. Deployt op **PythonAnywhere** (EU-regio) volgens het **pod-deploymodel**
(Flask + Jinja + SQLite, één proces, geen build-stap).

GitHub: `github.com/Waldstein-prog/market` (privé, onder `Waldstein-prog`). Push vanuit
de lab-monorepo via `git subtree push --prefix=market <auth-url> main`.

## Deploystatus (LIVE)
- **Fase I draait live** op `https://meadowmarket.eu.pythonanywhere.com` — pagina toont
  "Meadow Market" (met config-waarschuwing zolang `secrets.json` ontbreekt = normaal).
- PA-account: **`meadowmarket`** (NIEUW account, los van pod/magicmeadow).
- Repo gecloned in `~/market` (schoon, geen stray clones), venv `market-venv`,
  `~/.pa_api_token` gezet → `setup.sh` reload automatisch.
- Updaten na een push van mij: `cd ~/market && bash deploy/setup.sh`.

## Fasering
- **Fase I — technische PoC (NU).** Site met een toggle die in de **dev-guild** een
  Discord-rol **aan/uit** zet voor een gegeven user. Doel: bewijzen dat de technologie
  (Bot Token → Discord REST-API → rol-toekenning) werkt. **Geen OAuth2, geen DB.**
- **Fase II — businesslogic (LATER).** De user heeft de economy-specs *gedicteerd en
  bewust nog onsamenhangend* aangeleverd; die worden **in Fase II toegelicht**. Niet nu
  invullen. Verwacht: OAuth2-login om de bezoeker te identificeren, en rol/saldo-logica.

## Fase I — PoC (gebouwd)
- `backend/app.py` — routes: `/` (toggle-UI), `GET /api/status?user_id=`,
  `POST /api/toggle` ({user_id, enable}), `/healthz`.
- `backend/discord_api.py` — REST-wrapper: `get_member`, `has_role`, `add_role`,
  `remove_role`. Nette foutvertaling (401 token, 403 perms/hiërarchie, 404 geen lid).
- `backend/config.py` — laadt `bot_token`, `guild_id`, `role_id` uit env-vars
  (`DISCORD_BOT_TOKEN`, `DISCORD_GUILD_ID`, `DISCORD_ROLE_ID`) of `backend/secrets.json`.
- `backend/secrets.example.json` — sjabloon; kopieer → `secrets.json` (gitignored).
- UI: veld voor user-ID → status opvragen → toggle-knop.

## VOLGENDE STAP — Discord-bot + secrets.json (nog te doen)
De toggle werkt pas als dit ingevuld is. Nog niet gedaan.
1. **Bot-applicatie** aanmaken (Discord Developer Portal) → **Bot Token**.
2. Bot **inviten** in de dev-guild met permissie **Manage Roles** (OAuth2 → URL Generator,
   scope `bot`).
3. Bot-rol in de serverinstellingen **hoger** slepen dan de te togglen rol (hiërarchie!,
   anders 403).
4. IDs verzamelen (Developer Mode aan → rechtsklik → Copy ID): **guild-ID**, **rol-ID**,
   en een **test-user-ID**.
5. Op PA: `cp ~/market/backend/secrets.example.json ~/market/backend/secrets.json` en
   bot_token/guild_id/role_id invullen (`nano`). Staat NIET in git → blijft op PA.
   REST-fetch van leden vereist **geen** gateway-intent.
6. Testen: pagina openen → user-ID intypen → *Status opvragen* → *Rol aanzetten/afzetten*.

## Fase II — businesslogic (LATER, wacht op user)
User leverde de economy-specs *gedicteerd en bewust onsamenhangend*; worden pas in Fase II
toegelicht. Niet nu invullen. Verwacht: OAuth2-login + rol/saldo-logica.

## Lokaal draaien
`./run.sh` → venv + deps + start op poort 8700 → http://localhost:8700

## Deploy op PythonAnywhere
- Eerste keer: Bash-console → `git clone https://Waldstein-prog:<TOKEN>@github.com/Waldstein-prog/market.git ~/market`
- Web-tab → Add a new web app → Manual configuration (Python 3.x).
- `cd ~/market && bash deploy/setup.sh` (git pull + WSGI koppelen + reload).
- `secrets.json` staat NIET in git → op PA apart aanmaken (of env-vars via WSGI).
- Live-URL: `https://<naam>.eu.pythonanywhere.com`.
