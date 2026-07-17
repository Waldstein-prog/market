#!/usr/bin/env bash
# Deploy Meadow Market naar de Hetzner-VPS (ssh-config-alias 'hytale').
#
# Bouwt de release-binary LOKAAL (niet op de RAM-krappe VPS) en kopieert enkel
# de self-contained binary (templates/CSS zitten er via include_str! in) +
# de systemd-unit. secrets.json wordt NIET meegekopieerd — die staat apart op
# de server (bevat de bot-token, hoort niet in git of in transit-scripts).
#
# Gebruik:  ./deploy/deploy.sh
set -euo pipefail

REMOTE="${MARKET_REMOTE:-hytale}"
DEST="/opt/market"
cd "$(dirname "$0")/.."

# Volgorde is bewust: ALLES wat traag is (build, scp, daemon-reload) gebeurt
# terwijl de oude bot gewoon doordraait. Pas als alle bytes al op de server
# staan, stoppen we — en dan is de onderbreking enkel de restart zelf (~2s)
# i.p.v. de hele netwerkoverdracht. `install` op een draaiende binary kan niet
# (ETXTBSY), vandaar de losse /tmp-stage en de swap ná de stop.
echo "== 1/5 build (release) =="
cargo build --release

echo "== 2/5 binary + unit klaarzetten (bot draait nog) =="
scp target/release/market "$REMOTE:/tmp/market.new"
scp deploy/market.service "$REMOTE:/tmp/market.service"

echo "== 3/5 unit plaatsen + daemon-reload (bot draait nog) =="
ssh "$REMOTE" 'install -m 644 /tmp/market.service /etc/systemd/system/market.service \
  && rm -f /tmp/market.service && systemctl daemon-reload'

# Stop → swap → start in ÉÉN ssh-sessie: geen extra round-trips (elk ~60ms naar
# Hetzner) in het venster waarin de bot plat ligt.
echo "== 4/5 swap + herstart (korte onderbreking) =="
ssh "$REMOTE" "systemctl stop market 2>/dev/null || true; \
  install -o market -g market -m 755 /tmp/market.new $DEST/market && rm -f /tmp/market.new; \
  systemctl enable --now market"

echo "== 5/5 status =="
ssh "$REMOTE" 'sleep 3 && systemctl --no-pager --lines=0 status market | head -6'
echo "== klaar — logs: ssh $REMOTE 'journalctl -u market -f' =="
