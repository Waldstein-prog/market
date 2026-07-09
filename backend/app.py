import os
import secrets

from flask import Flask, render_template, request, jsonify

import config
import discord_api


def create_app():
    app = Flask(__name__)
    app.secret_key = os.environ.get("SECRET_KEY", secrets.token_hex(32))
    app.config["SESSION_COOKIE_HTTPONLY"] = True
    app.config["SESSION_COOKIE_SAMESITE"] = "Lax"
    app.config["SESSION_COOKIE_SECURE"] = os.environ.get("SESSION_COOKIE_SECURE") == "1"

    @app.route("/")
    def index():
        cfg = config.load_config()
        return render_template("index.html", configured=not config.missing_keys(cfg))

    @app.route("/api/status")
    def api_status():
        cfg = config.load_config()
        missing = config.missing_keys(cfg)
        if missing:
            return jsonify(ok=False, error=f"Config ontbreekt: {', '.join(missing)}"), 400
        user_id = (request.args.get("user_id") or "").strip()
        if not user_id.isdigit():
            return jsonify(ok=False, error="Geef een geldig Discord user-ID (cijfers)."), 400
        try:
            state = discord_api.has_role(cfg["bot_token"], cfg["guild_id"], user_id, cfg["role_id"])
        except discord_api.DiscordError as e:
            return jsonify(ok=False, error=str(e)), 400
        if state is None:
            return jsonify(ok=False, error="Die gebruiker is geen lid van de guild."), 404
        return jsonify(ok=True, has_role=state)

    @app.route("/api/toggle", methods=["POST"])
    def api_toggle():
        cfg = config.load_config()
        missing = config.missing_keys(cfg)
        if missing:
            return jsonify(ok=False, error=f"Config ontbreekt: {', '.join(missing)}"), 400
        data = request.get_json(silent=True) or {}
        user_id = str(data.get("user_id", "")).strip()
        want = bool(data.get("enable"))
        if not user_id.isdigit():
            return jsonify(ok=False, error="Geef een geldig Discord user-ID (cijfers)."), 400
        try:
            if want:
                discord_api.add_role(cfg["bot_token"], cfg["guild_id"], user_id, cfg["role_id"])
            else:
                discord_api.remove_role(cfg["bot_token"], cfg["guild_id"], user_id, cfg["role_id"])
            state = discord_api.has_role(cfg["bot_token"], cfg["guild_id"], user_id, cfg["role_id"])
        except discord_api.DiscordError as e:
            return jsonify(ok=False, error=str(e)), 400
        return jsonify(ok=True, has_role=state)

    @app.route("/healthz")
    def healthz():
        return {"status": "ok"}

    return app


app = create_app()


if __name__ == "__main__":
    port = int(os.environ.get("PORT", "8700"))
    app.run(host="0.0.0.0", port=port, debug=True)
