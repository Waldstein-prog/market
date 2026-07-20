#!/usr/bin/env bash
# E2e mock-test voor het Twitch-luik: bewijst de reward-routing
#   dagpas-reward   → grant_day_whitelist   (expires = now+24u)
#   perma-reward    → grant_perma_whitelist (expires = NULL, permanent)
# zonder een echt Twitch-account (gebruikt de Twitch-CLI EventSub-mock).
#
# Vereist: de Twitch CLI (`twitch`) en een gebouwde debug-binary (`cargo build`).
# Draai vanuit de market-projectroot:  bash docs/perma_e2e.sh
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$ROOT/target/debug/market"
[ -x "$BIN" ] || { echo "bouw eerst: cargo build ($BIN ontbreekt)"; exit 1; }
WORK=$(mktemp -d); cd "$WORK" || exit 1
DB="$WORK/coins.db"
echo "== workdir: $WORK =="

# 1. Mock EventSub-ws-server op 8080
twitch event websocket start-server >"$WORK/ws.log" 2>&1 &
WSPID=$!; sleep 2

# 2. Market in mock + web-only (geen Discord-gateway), tegen de mock-ws
DISCORD_BOT_TOKEN=dummy MARKET_WEB_ONLY=1 MARKET_TWITCH_MOCK=1 \
TWITCH_ENABLED=1 TWITCH_APP_ID=x TWITCH_APP_SECRET=x \
TWITCH_REWARD_TITLE="test-day" TWITCH_PERMA_REWARD_TITLE="test-perma" \
TWITCH_EVENTSUB_URL=ws://127.0.0.1:8080/ws RUST_LOG=info,serenity=warn \
"$BIN" >"$WORK/market.log" 2>&1 &
MKPID=$!; sleep 3

# 3. Namen vooraf vastzetten (de mock-CLI kan user_input niet meesturen): 1u dagpas voor 2 kijkers.
python3 - "$DB" <<'PY'
import sqlite3, sys, time
c=sqlite3.connect(sys.argv[1]); now=time.time()
for uid,name in [("twitch:111","PermaGuy"),("twitch:222","DayGuy")]:
    c.execute("INSERT OR REPLACE INTO hytale_whitelist(user_id,hytale_name,expires) VALUES(?,?,?)",
              (uid,name, now+3600))
c.commit(); c.close(); print("pre-seed ok")
PY

# 4. Redemptions injecteren: perma voor 111, dag voor 222.
twitch event trigger channel.channel_points_custom_reward_redemption.add \
  -T websocket -i mock_perma_reward -f 111 -I evt-perma-1 >"$WORK/t1.log" 2>&1
twitch event trigger channel.channel_points_custom_reward_redemption.add \
  -T websocket -i mock_reward -f 222 -I evt-day-1 >"$WORK/t2.log" 2>&1
sleep 2

# 5. Resultaat
echo "== market twitch-log =="
grep -i "twitch\|perma\|whitelist" "$WORK/market.log" | tail -20
echo "== DB-eindstand (verwacht: 111 PERMANENT, 222 dagpas) =="
python3 - "$DB" <<'PY'
import sqlite3, sys
c=sqlite3.connect(sys.argv[1])
for uid,name,exp in c.execute("SELECT user_id,hytale_name,expires FROM hytale_whitelist ORDER BY user_id"):
    print(f"  {uid:12} {name:10} -> {'PERMANENT' if exp is None else f'dagpas (expires={exp:.0f})'}")
c.close()
PY

kill $MKPID $WSPID 2>/dev/null; wait 2>/dev/null
echo "== klaar =="
