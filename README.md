# Meadow Market

Discord-economysysteem met gekoppelde website. Flask + Jinja + SQLite, één proces,
geen build-stap. Deployt op **PythonAnywhere** (EU-regio) volgens het pod-deploymodel.

- Lab-poort: **8700**
- Live-URL (na PA-setup): `https://<naam>.eu.pythonanywhere.com`

## Lokaal draaien

```bash
./run.sh          # maakt venv, installeert deps, start op poort 8700
```

Open http://localhost:8700 — nu enkel een "Meadow Market"-landingspagina.

## Deployen op PythonAnywhere

1. Maak een web-app aan (Web-tab → Add a new web app → Manual configuration, Python 3.x).
2. Clone deze repo naar `~/market`.
3. `cd ~/market && bash deploy/setup.sh`

`deploy/pa_wsgi.py` vindt zelf `~/market/backend` en genereert eenmalig een
persistente `secret_key.txt` (gitignored, overleeft `git pull`). Auto-reload werkt
via een PA API-token in `~/.pa_api_token`; anders volstaat één klik op Reload.

## Status

Scaffold: landingspagina + deployscript. Het economy-datamodel (Discord OAuth2-login
+ rol-toekenning via Bot Token) volgt zodra de specs er zijn — zie `HANDOVER.md`.
