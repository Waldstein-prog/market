#!/bin/bash
# PythonAnywhere one-shot setup voor Meadow Market.
#
#   cd ~/market && bash deploy/setup.sh
#
# Doet ALLES in één keer: laatste code ophalen (git pull), WSGI-bestand op z'n
# plek zetten, dependencies installeren en de web-app herladen.
# Zonder API-token hoef je daarna enkel nog op de Web-tab op Reload te klikken.
set -e

REPO="$(cd "$(dirname "$0")/.." && pwd)"
BACKEND="$REPO/backend"

echo "==> 1/4  Laatste code ophalen (git pull)"
git -C "$REPO" pull --ff-only

echo "==> 2/4  WSGI-bestand koppelen"
WROTE=0
for f in /var/www/*_wsgi.py; do
    [ -e "$f" ] || continue
    # --remove-destination vervangt ook een bestaande symlink door een echt
    # bestand (PythonAnywhere volgt symlinks niet altijd).
    cp --remove-destination "$REPO/deploy/pa_wsgi.py" "$f"
    echo "    geschreven naar $f"
    WROTE=1
done
if [ "$WROTE" = 0 ]; then
    echo "    !! Geen web-app gevonden in /var/www/."
    echo "       Maak eerst op de Web-tab een web-app aan (Add a new web app ->"
    echo "       Manual configuration), en draai dit script daarna opnieuw."
    exit 1
fi

echo "==> 3/4  Dependencies"
if [ -n "$VIRTUAL_ENV" ]; then
    pip install -q -r "$BACKEND/requirements.txt"
    echo "    geïnstalleerd in virtualenv: $VIRTUAL_ENV"
else
    echo "    Geen actieve virtualenv. Activeer 'm en installeer eenmalig:"
    echo "       workon market-venv && pip install -r $BACKEND/requirements.txt"
fi

echo "==> 4/4  Web-app herladen"
RELOADED=0
# Domein en API-host bepalen — werkt op zowel de US- als de EU-site.
PA_BASE_DOMAIN="${PYTHONANYWHERE_DOMAIN:-pythonanywhere.com}"
DOMAIN="${WEBAPP_DOMAIN:-$(echo "$USER" | tr 'A-Z' 'a-z').$PA_BASE_DOMAIN}"
API_BASE="${PYTHONANYWHERE_SITE:-https://www.pythonanywhere.com}"

# Token uit env, of uit ~/.pa_api_token (zet 'm daar één keer neer).
TOKEN="${API_TOKEN:-}"
if [ -z "$TOKEN" ] && [ -f "$HOME/.pa_api_token" ]; then
    TOKEN="$(tr -d '[:space:]' < "$HOME/.pa_api_token")"
fi

if [ -n "$TOKEN" ]; then
    if curl -sf -X POST \
        "$API_BASE/api/v0/user/$USER/webapps/$DOMAIN/reload/" \
        -H "Authorization: Token $TOKEN" >/dev/null 2>&1; then
        echo "    automatisch herladen gelukt ($DOMAIN)"
        RELOADED=1
    else
        echo "    !! reload-API faalde. Controleer token en domein ($DOMAIN via $API_BASE)."
    fi
fi

echo ""
echo "============================================================"
if [ "$RELOADED" = 1 ]; then
    echo " KLAAR. Open https://$DOMAIN — Meadow Market draait."
else
    echo " BIJNA KLAAR. Eén klik nog: Web-tab -> groene Reload-knop."
    echo " (auto-reload aanzetten? Zet je API-token één keer in een bestand:"
    echo "    echo 'JOUW_TOKEN' > ~/.pa_api_token"
    echo "  Daarna herlaadt dit script voortaan vanzelf.)"
fi
echo "============================================================"
