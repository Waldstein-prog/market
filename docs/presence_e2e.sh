#!/usr/bin/env bash
# End-to-end: telt een pas enkel af terwijl de speler ín het spel is?
#
# Draait market (web-only, eigen poort + eigen DB in een tijdelijke map) tegen een
# NAGEMAAKT chat_mirror.log en schuift daar join/leave-regels in, precies zoals de
# Hytale-server ze schrijft. Bewijst dat de echte binary — niet enkel de unit-tests —
# de pas stilzet bij een leave en weer laat lopen bij een join.
#
# Gebruik:  bash docs/presence_e2e.sh
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT=$PWD

WORK=$(mktemp -d)
trap 'kill "${MARKET_PID:-0}" 2>/dev/null || true; rm -rf "$WORK"' EXIT
echo "== workdir: $WORK =="

cargo build --release >/dev/null 2>&1
cp "$ROOT/secrets.json" "$WORK/secrets.json" 2>/dev/null || echo '{"bot_token":"x"}' > "$WORK/secrets.json"
LOG="$WORK/chat_mirror.log"
: > "$LOG"

py() { python3 - "$@"; }

# Pas van 2 uur voor 'TestSpeler', gekoppeld aan Discord-lid 'disc1'.
py <<PY
import sqlite3, time
con = sqlite3.connect("$WORK/coins.db")
con.executescript("""
CREATE TABLE IF NOT EXISTS hytale_whitelist(user_id TEXT PRIMARY KEY, hytale_name TEXT NOT NULL, expires REAL);
""")
con.execute("INSERT INTO hytale_whitelist(user_id,hytale_name,expires) VALUES('twitch:42','TestSpeler',?)",
            (time.time() + 2*3600,))
con.commit()
print("pas gezet: 2u voor TestSpeler")
PY

cd "$WORK"
MARKET_WEB_ONLY=1 MARKET_PRESENCE_LOG="$LOG" MARKET_PORT=8702 \
  RUST_LOG=market=info "$ROOT/target/release/market" > "$WORK/market.log" 2>&1 &
MARKET_PID=$!
sleep 4

rest() {
  py <<PY
import sqlite3
r = sqlite3.connect("file:$WORK/coins.db?mode=ro", uri=True).execute(
    "SELECT expires, remaining FROM hytale_whitelist WHERE user_id='twitch:42'").fetchone()
print(f"{r[1]}" if r[1] is not None else "loopt")
PY
}

echo "== 1. speler logt uit → pas moet stilvallen =="
printf '%s\tleave\tTestSpeler left the game\n' "$(date +%s)" >> "$LOG"
sleep 8
PAUSED=$(rest)
echo "   remaining = $PAUSED"

echo "== 2. tien seconden niets doen → er mag niets af gaan =="
sleep 10
STILL=$(rest)
echo "   remaining = $STILL"

echo "== 3. speler logt in → pas moet weer lopen =="
printf '%s\tjoin\tTestSpeler joined the game\n' "$(date +%s)" >> "$LOG"
sleep 8
RUNNING=$(rest)
echo "   remaining = $RUNNING"

echo "== market-log =="
grep -iE "aanwezigheid|pauze|hervat" "$WORK/market.log" || true

echo "== oordeel =="
py <<PY
paused, still, running = "$PAUSED", "$STILL", "$RUNNING"
ok = True
if paused == "loopt":
    print("  ✗ leave zette de pas niet op pauze"); ok = False
elif abs(float(paused) - 2*3600) > 30:
    print(f"  ✗ verkeerde resterende tijd bij pauze: {paused}"); ok = False
else:
    print(f"  ✓ leave → pauze op {float(paused)/3600:.2f}u")
if still != "loopt" and paused != "loopt" and abs(float(still) - float(paused)) > 1:
    print(f"  ✗ er ging tijd af tijdens de pauze: {paused} → {still}"); ok = False
else:
    print("  ✓ tijdens de pauze ging er geen speeltijd af")
if running != "loopt":
    print(f"  ✗ join hervatte de pas niet (remaining={running})"); ok = False
else:
    print("  ✓ join → pas loopt weer")
print("== KLAAR ==" if ok else "== GEZAKT ==")
raise SystemExit(0 if ok else 1)
PY
