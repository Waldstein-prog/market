"""Laad Discord-config uit env-vars, met fallback naar backend/secrets.json.

secrets.json staat in .gitignore (net als secret_key.txt). Kopieer
secrets.example.json → secrets.json en vul je eigen waarden in.
"""
import json
import os

_HERE = os.path.dirname(__file__)
_SECRETS_PATH = os.path.join(_HERE, "secrets.json")

_KEYS = ("bot_token", "guild_id", "role_id")
# Optioneel: pin een vaste test-user + rol-label zodat de UI één simpele
# aan/uit-schakelaar wordt (geen user-ID-veld). Niet vereist voor de config-check.
_OPTIONAL = ("user_id", "role_label")


def _load_file():
    if os.path.exists(_SECRETS_PATH):
        with open(_SECRETS_PATH) as fh:
            return json.load(fh)
    return {}


def load_config():
    """Env-var wint van bestand. Env-namen: DISCORD_BOT_TOKEN, DISCORD_GUILD_ID,
    DISCORD_ROLE_ID, DISCORD_USER_ID, DISCORD_ROLE_LABEL."""
    fromfile = _load_file()
    cfg = {}
    for key in _KEYS + _OPTIONAL:
        env = os.environ.get("DISCORD_" + key.upper())
        cfg[key] = env if env else str(fromfile.get(key, "")).strip()
    return cfg


def missing_keys(cfg):
    return [k for k in _KEYS if not cfg.get(k)]
