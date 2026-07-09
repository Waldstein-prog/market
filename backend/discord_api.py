"""Dunne wrapper rond de Discord REST-API voor rol-toekenning.

Gebruikt enkel een Bot Token (geen gateway/intents nodig). De bot moet:
  - lid zijn van de guild,
  - de permissie "Manage Roles" hebben,
  - een rol hebben die HOGER staat dan de rol die hij toekent (hiërarchie).
"""
import requests

API = "https://discord.com/api/v10"


class DiscordError(Exception):
    """Nette foutmelding met HTTP-status + reden voor de UI."""

    def __init__(self, status, message):
        self.status = status
        super().__init__(message)


def _headers(token):
    return {
        "Authorization": f"Bot {token}",
        "Content-Type": "application/json",
    }


def _explain(resp):
    """Vertaal een niet-OK Discord-respons naar een begrijpelijke reden."""
    try:
        body = resp.json()
        msg = body.get("message", resp.text)
    except ValueError:
        msg = resp.text
    if resp.status_code == 401:
        return "Bot Token ongeldig (401)."
    if resp.status_code == 403:
        return ("Geen toestemming (403): bot mist 'Manage Roles' of zijn rol "
                "staat niet hoger dan de doelrol.")
    if resp.status_code == 404:
        return "Niet gevonden (404): guild, gebruiker of rol bestaat niet, of user is geen lid."
    if resp.status_code == 429:
        return "Rate limited (429): even wachten."
    return f"Discord-fout ({resp.status_code}): {msg}"


def get_member(token, guild_id, user_id):
    """Haal het guild-lid op. Returnt None als de user geen lid is (404)."""
    resp = requests.get(
        f"{API}/guilds/{guild_id}/members/{user_id}",
        headers=_headers(token), timeout=10,
    )
    if resp.status_code == 404:
        return None
    if not resp.ok:
        raise DiscordError(resp.status_code, _explain(resp))
    return resp.json()


def has_role(token, guild_id, user_id, role_id):
    member = get_member(token, guild_id, user_id)
    if member is None:
        return None  # geen lid
    return str(role_id) in [str(r) for r in member.get("roles", [])]


def add_role(token, guild_id, user_id, role_id):
    resp = requests.put(
        f"{API}/guilds/{guild_id}/members/{user_id}/roles/{role_id}",
        headers=_headers(token), timeout=10,
    )
    # 204 = toegevoegd, 200/no-op als hij 'm al had.
    if not resp.ok:
        raise DiscordError(resp.status_code, _explain(resp))


def remove_role(token, guild_id, user_id, role_id):
    resp = requests.delete(
        f"{API}/guilds/{guild_id}/members/{user_id}/roles/{role_id}",
        headers=_headers(token), timeout=10,
    )
    if not resp.ok:
        raise DiscordError(resp.status_code, _explain(resp))
