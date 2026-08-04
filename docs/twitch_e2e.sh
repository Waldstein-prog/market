#!/usr/bin/env bash
# E2e mock-test voor het Twitch-luik, zonder echt Twitch-account (Twitch-CLI EventSub-mock).
# Bewijst de vijf paden van `on_redeem` sinds de streamer de reward zelf bezit:
#   1. titel matcht NIET             → niets (geen grant) — alle andere beloningen van het kanaal
#   2. titel matcht, naam ongeldig   → geen grant, wél een 'twitch/rejected'-regel (manueel terugbetalen)
#   3. titel matcht, zelfde naam     → grant van N uur uit de SETTINGS + whisper met die tekst
#   4. perma-titel matcht            → permanente grant (expires = NULL)
#   5. titel matcht, ANDERE naam dan de vastgezette → geen tijd, 'twitch/name_mismatch' + whisper
#
# ⚠️ De Twitch-CLI kan `user_input` niet zetten: elke redeem stuurt letterlijk
# "Test Input From CLI". Daarom dragen de kijkers die wél tijd moeten krijgen (111/333) die
# tekst als vastgezette naam — enkel zo is het getypte veld gelijk aan wat vastligt. Kijker
# 555 staat geregistreerd op een gewone naam en botst dus met diezelfde invoer: precies het
# nieuwe pad dat we willen bewijzen.
#
# Vereist: de Twitch CLI (`twitch`) en een gebouwde debug-binary (`cargo build`).
# Draai vanuit de market-projectroot:  bash docs/twitch_e2e.sh
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$ROOT/target/debug/market"
[ -x "$BIN" ] || { echo "bouw eerst: cargo build ($BIN ontbreekt)"; exit 1; }
WORK=$(mktemp -d); cd "$WORK" || exit 1
DB="$WORK/coins.db"
echo "== workdir: $WORK =="

DAY_TITLE="Hytale pass (test)"
PERMA_TITLE="Hytale forever (test)"

# 1. Mock EventSub-ws-server op 8080
twitch event websocket start-server >"$WORK/ws.log" 2>&1 &
WSPID=$!; sleep 2

# 2. Market in mock + web-only (geen Discord-gateway), tegen de mock-ws.
#    De reward-titels/duur/whisper staan NIET meer in env of secrets.json maar in de
#    settings-tabel — die vullen we hieronder, ná het aanmaken van de DB.
#    MARKET_PORT: een eigen poort, zodat dit naast een draaiende lokale market kan.
DISCORD_BOT_TOKEN=dummy MARKET_WEB_ONLY=1 MARKET_TWITCH_MOCK=1 MARKET_PORT=8701 \
TWITCH_ENABLED=1 TWITCH_APP_ID=x TWITCH_APP_SECRET=x \
TWITCH_EVENTSUB_URL=ws://127.0.0.1:8080/ws RUST_LOG=info,serenity=warn \
"$BIN" >"$WORK/market.log" 2>&1 &
MKPID=$!; sleep 3

# 3. Settings zetten (live gelezen) + één naam vooraf vastzetten. De CLI stuurt altijd
#    user_input = "Test Input From CLI" (met spaties) — die naam is ongeldig, wat precies
#    het weiger-pad test; voor de geslaagde redeem zetten we de naam vooraf vast.
python3 - "$DB" "$DAY_TITLE" "$PERMA_TITLE" <<'PY'
import sqlite3, sys, time
db, day, perma = sys.argv[1], sys.argv[2], sys.argv[3]
c = sqlite3.connect(db)
for k, v in [("twitch_reward_title", day), ("twitch_perma_reward_title", perma),
             ("twitch_pass_hours", "2"),
             ("twitch_whisper_text", "Je mag {uren} uur mee op de server als {naam} — 1.2.3.4:5520"),
             ("twitch_perma_whisper_text", "Permanent binnen als {naam} — 1.2.3.4:5520"),
             ("twitch_mismatch_whisper_text", "Andere naam dan {naam} — geen tijd toegekend.")]:
    c.execute("INSERT INTO settings(key,value) VALUES(?,?) "
              "ON CONFLICT(key) DO UPDATE SET value=excluded.value", (k, v))
# 333 (dagpas) en 111 (perma) hebben hun naam al vastgezet op wat de CLI stuurt, zodat hun
# redeem doorgaat; 555 staat op een àndere naam (→ botsing); 222 heeft er nog geen.
for uid, name in [("twitch:111", "Test Input From CLI"),
                  ("twitch:333", "Test Input From CLI"),
                  ("twitch:555", "DayGuy")]:
    c.execute("INSERT OR REPLACE INTO hytale_whitelist(user_id,hytale_name,expires) VALUES(?,?,?)",
              (uid, name, time.time() + 3600))
c.commit(); c.close(); print("settings + pre-seed ok")
PY

# 4. De vier redemptions injecteren (-n zet de reward-titel, -f de kijker).
trig() { twitch event trigger channel.channel_points_custom_reward_redemption.add \
           -T websocket -n "$1" -f "$2" -I "$3" >"$WORK/$3.log" 2>&1; }
trig "Song request"   444 evt-other      # 1. niet van ons
trig "$DAY_TITLE"     222 evt-badname    # 2. naam ongeldig, geen refund
trig "$DAY_TITLE"     333 evt-day        # 3. 2 uur + whisper
trig "$PERMA_TITLE"   111 evt-perma      # 4. permanent
trig "$DAY_TITLE"     555 evt-mismatch   # 5. andere naam dan vastgezet → niets toekennen
sleep 2

# 5. Resultaat
echo "== market twitch-log =="
grep -i "twitch\|whisper\|genegeerd\|geweigerd" "$WORK/market.log" | tail -20
echo "== DB-eindstand =="
python3 - "$DB" <<'PY'
import sqlite3, sys, time
c = sqlite3.connect(sys.argv[1]); now = time.time()
print("  whitelist:")
for uid, name, exp in c.execute("SELECT user_id,hytale_name,expires FROM hytale_whitelist ORDER BY user_id"):
    when = "PERMANENT" if exp is None else f"+{(exp-now)/3600:.2f}u"
    print(f"    {uid:12} {name:10} -> {when}")
print("  twitch-logboek:")
for kind, act, det in c.execute(
        "SELECT event, actor_name, detail FROM server_log WHERE category='twitch' ORDER BY ts"):
    print(f"    {kind:16} {act or '':14} {det or ''}")
print("  444 mag NIET in de whitelist staan (titel matchte niet);")
print("  222 mag er niet bij komen (ongeldige naam); 333 moet ~3u hebben (1u seed + 2u).")
print("  555 moet op ~1u BLIJVEN staan (andere naam getypt) + een name_mismatch-regel hebben.")
c.close()
PY

kill $MKPID $WSPID 2>/dev/null; wait 2>/dev/null
echo "== klaar =="
