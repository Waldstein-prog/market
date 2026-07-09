# Handover — 2026-07-09 (market — opstart)

## Wat dit is
Nieuw project **`market`** (lab-poort nog toe te wijzen): een **Discord-economysysteem**
met een **gekoppelde website**. De website deployt op **PythonAnywhere**, met exact hetzelfde
deploymentmodel als project **pod** (nu in `lab/Archief/pod`, live op
`https://magicmeadow.eu.pythonanywhere.com`, EU-regio).

Status: **nog niets gebouwd.** Deze sessie was verkenning in afwachting van de specs.
`lab/market/` bevat enkel dit HANDOVER.

## Wat deze sessie deed
- Het **pod-deploymentmodel** volledig bestudeerd (zie blauwdruk hieronder).
- Architectuur uitgeklaard via een reeks verduidelijkingen van de user:
  - Geen gateway-bot, geen always-on proces nodig → alles past op PA-gratis.
  - **Geen slash-commands.** De flow is **web-gedreven**: embed met links → OAuth2-login → rol.
  - **Geen herbruikbare template** — één concrete server.
- Nog géén specs voor de inhoud (wat de economy precies doet) — die komen **later**.
- Nog niets gebouwd; enkel dit HANDOVER in `lab/market/`.

## Pod-blauwdruk om te hergebruiken (geverifieerd, bestanden gelezen)
Stack: **Flask + Jinja + SQLite**, één Python-proces, geen build-stap.
- `backend/app.py` (`create_app()`), `backend/db.py` (SQLite + auto-migraties in `_migrate`),
  `schema.sql`, `seed.py`, `set_password.py`.
- `requirements.txt`: Flask, Werkzeug, requests, pytest — meer niet.
- Lokaal: `./run.sh` → maakt venv, installeert deps, seedt, start op vaste poort.
- Tests: `cd backend && python3 -m pytest` (pod had 16 groen, incl. Playwright-e2e).

PythonAnywhere-deploy (het slimme deel — letterlijk klonen):
- `deploy/pa_wsgi.py` — generiek WSGI, **geen aanpassing nodig**: vindt zelf `~/<proj>/backend`,
  genereert eenmalig persistente `secret_key.txt` (gitignored, overleeft `git pull`).
- `deploy/setup.sh` — one-shot: `git pull` → seed → admin-wachtwoord (enkel 1e keer) →
  WSGI koppelen in `/var/www/*_wsgi.py` → `pip install` → **auto-reload via PA API-token**
  uit `~/.pa_api_token` (regio-bewust via `PYTHONANYWHERE_SITE`/`_DOMAIN`, werkt EU én US).
- Updaten na push = `cd ~/market && bash deploy/setup.sh`. `*.db` gitignored → data blijft.
- **EU-regio**: live-URL = `https://<naam>.eu.pythonanywhere.com` (zonder `.eu.` = PA-placeholder).

GitHub: per-project **private repo** onder `Waldstein-prog` (zoals `pod.git`); token in
`lab/github creds.txt`. Voor dit project → `github.com/Waldstein-prog/market.git` (nog aan te maken).

## Architectuur — huidige stand (2026-07-09)
**Geen herbruikbare template** — één concrete server. **Geen slash-commands / gateway-bot.**
De flow is **web-gedreven**:
1. In Discord staat een **embed met links** → bezoeker klikt → landt op de PA-site.
2. Site identificeert de bezoeker via **Discord OAuth2-login** (nodig om te weten aan wie een
   rol toegekend moet worden).
3. Site **kent een Discord-rol toe** via de Discord REST-API met een **Bot Token**.

Alles in **één Flask-proces + één SQLite-DB** op PA (always-on = gewoon de web-app, zoals pod).

Wat dit technisch vergt:
- Echte **bot-applicatie** als *credential* (niet als draaiend proces): bot in de server,
  permissie **Manage Roles**, bot-rol **hoger** dan de uit te delen rol (Discord-hiërarchie).
- Interactions-endpoint / PyNaCl uit vorige ronde: **NIET nodig** (geen slash-commands) — geschrapt.
- Secrets: **Bot Token** + **OAuth2 Client ID/Secret**. Buiten git, zoals pod's `secret_key.txt`.

## Nog open (wacht op specs)
- **Wat is de "economy"?** Kent de site altijd dezelfde rol toe (pure gate: klik → rol), of hangt
  de rol af van saldo/aankoop/actie? Bepaalt het hele datamodel.
- Wordt de **embed met links** één keer handmatig gepost, of moet de site/bot 'm posten?
- User geeft **later concrete specs** — nu niet verder bouwen.

## Volgende stappen (na de specs)
1. Lab-poort toewijzen in `lab/CLAUDE.md` (pod=8500, tracker=8600 → market wellicht **8700**).
2. Repo aanmaken onder `Waldstein-prog`, pod's `deploy/`-scaffold + `run.sh` overnemen.
3. Datamodel + schema op basis van de specs.
4. Discord OAuth2-login-flow + rol-toekenning via Bot Token (Manage Roles + hiërarchie).

## Context / valkuilen
- Deze machine mist soms `python3-venv`; pod viel terug op systeem-`python3`.
- Smoke-test/deploy altijd tegen het **`.eu.`-domein**.
- Referentiebestanden: `lab/Archief/pod/deploy/`, `lab/Archief/pod/backend/`, `lab/Archief/pod/README.md`.
