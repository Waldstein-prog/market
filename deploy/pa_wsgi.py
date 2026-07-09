"""Generiek WSGI-bestand voor PythonAnywhere — vereist GEEN aanpassingen.

Kopieer dit naar je PythonAnywhere WSGI-locatie, bv.:
    cp ~/market/deploy/pa_wsgi.py /var/www/${USER}_pythonanywhere_com_wsgi.py

Het vindt zelf het app-pad (~/market/backend) en maakt eenmalig een vaste,
willekeurige SECRET_KEY aan in backend/secret_key.txt (gitignored, blijft
bewaard over git pulls heen).
"""
import os
import sys

BASE = os.path.expanduser("~/market/backend")
if BASE not in sys.path:
    sys.path.insert(0, BASE)

# Persistente SECRET_KEY naast de app — eenmalig gegenereerd.
_secret_path = os.path.join(BASE, "secret_key.txt")
if os.path.exists(_secret_path):
    with open(_secret_path) as fh:
        os.environ["SECRET_KEY"] = fh.read().strip()
else:
    import secrets
    _key = secrets.token_hex(32)
    with open(_secret_path, "w") as fh:
        fh.write(_key)
    os.environ["SECRET_KEY"] = _key

os.environ.setdefault("SESSION_COOKIE_SECURE", "1")

from app import app as application
