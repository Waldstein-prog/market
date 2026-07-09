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

echo "== 1/5 build (release) =="
cargo build --release

echo "== 2/5 service stoppen (indien actief) =="
ssh "$REMOTE" 'systemctl stop market 2>/dev/null || true'

echo "== 3/5 binary kopiëren =="
scp target/release/market "$REMOTE:/tmp/market.new"
ssh "$REMOTE" "install -o market -g market -m 755 /tmp/market.new $DEST/market && rm -f /tmp/market.new"

echo "== 4/5 systemd-unit plaatsen =="
scp deploy/market.service "$REMOTE:/tmp/market.service"
ssh "$REMOTE" 'install -m 644 /tmp/market.service /etc/systemd/system/market.service && rm -f /tmp/market.service && systemctl daemon-reload'

echo "== 5/5 starten + status =="
ssh "$REMOTE" 'systemctl enable --now market && sleep 3 && systemctl --no-pager --lines=0 status market | head -6'
echo "== klaar — logs: ssh $REMOTE 'journalctl -u market -f' =="
