#!/usr/bin/env python3
"""Dagelijkse snapshot van market's coins.db.

Waarom niet gewoon `cp`: market schrijft dóór tijdens de backup, en een kale kopie
kan dan een half geschreven transactie vangen — een bestand dat er is maar stuk is.
SQLite's **online-backup-API** maakt wél een consistente kopie zonder de app te
blokkeren. Daarna doen we een `integrity_check`: een backup die je niet gecontroleerd
hebt is een aanname, geen backup.

Alles in coins.db is onvervangbaar: saldo's, aankopen, passen, shop-instellingen en
het logboek. Er stond hier tot 2026-07-15 niets tegenover.

Config via env (defaults hieronder):
  BACKUP_SRC=/opt/market/coins.db
  BACKUP_DIR=/opt/backups/market
  BACKUP_KEEP_DAYS=30
"""
import gzip
import os
import shutil
import sqlite3
import sys
import time
from datetime import datetime, timezone

SRC = os.environ.get("BACKUP_SRC", "/opt/market/coins.db")
DEST_DIR = os.environ.get("BACKUP_DIR", "/opt/backups/market")
KEEP_DAYS = int(os.environ.get("BACKUP_KEEP_DAYS", "30"))


def log(msg):
    print(msg, flush=True)


def main():
    if not os.path.exists(SRC):
        log(f"FOUT: {SRC} bestaat niet"); return 1
    os.makedirs(DEST_DIR, exist_ok=True)

    stamp = datetime.now(timezone.utc).strftime("%Y-%m-%d")
    tmp = os.path.join(DEST_DIR, f".coins-{stamp}.tmp")
    final = os.path.join(DEST_DIR, f"coins-{stamp}.db.gz")

    # 1) Consistente snapshot (read-only bron; ruime timeout voor een lopende schrijf).
    src = sqlite3.connect(f"file:{SRC}?mode=ro", uri=True, timeout=30)
    dst = sqlite3.connect(tmp)
    try:
        with dst:
            src.backup(dst)
    finally:
        dst.close()
        src.close()

    # 2) Controleren vóór we hem als backup beschouwen.
    chk = sqlite3.connect(tmp)
    try:
        res = chk.execute("PRAGMA integrity_check").fetchone()[0]
        rows = chk.execute("SELECT COUNT(*) FROM coins").fetchone()[0]
    finally:
        chk.close()
    if res != "ok":
        os.remove(tmp)
        log(f"FOUT: integrity_check zegt {res!r} — snapshot weggegooid")
        return 1

    # 3) Inpakken; pas op het einde hernoemen, zodat er nooit een halve .gz blijft staan.
    part = final + ".part"
    with open(tmp, "rb") as f_in, gzip.open(part, "wb", compresslevel=6) as f_out:
        shutil.copyfileobj(f_in, f_out)
    os.replace(part, final)
    os.remove(tmp)
    log(f"ok: {final} ({os.path.getsize(final)} bytes, {rows} leden, integriteit ok)")

    # 4) Opruimen op leeftijd.
    grens = time.time() - KEEP_DAYS * 86400
    weg = 0
    for f in os.listdir(DEST_DIR):
        if not (f.startswith("coins-") and f.endswith(".db.gz")):
            continue
        p = os.path.join(DEST_DIR, f)
        if os.path.getmtime(p) < grens:
            os.remove(p)
            weg += 1
    bewaard = len([f for f in os.listdir(DEST_DIR) if f.endswith(".db.gz")])
    log(f"opruiming: {weg} verwijderd, {bewaard} snapshots bewaard (max {KEEP_DAYS} dagen)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
