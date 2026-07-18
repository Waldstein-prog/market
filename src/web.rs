//! Axum-site.
//! - `/`            persoonlijke pagina: login-knop, of (na Discord-login) je
//!                  coin-overzicht als Flowerborn, anders de regels-pagina;
//! - `/login`       start de Discord OAuth2-flow;
//! - `/auth/callback` wisselt de code om, haalt de identiteit op, zet een sessie;
//! - `/logout`      sessie wissen;
//! - `/admin`       de Fase-I rol-toggle (intern beheertool, ongewijzigd).
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Form, Multipart, Path, Query, State},
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{COOKIE, SET_COOKIE},
    },
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use rand::Rng;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::Config;
use crate::db::{self, DbPool};
use crate::discord_rest::Discord;
use crate::settings;

const SESSION_MAX_AGE: i64 = 90 * 24 * 3600; // ~90 dagen: voelt als "één keer inloggen"
const MEADOW: &str = "#6b9b52";

/// Hoeveel items er dagelijks in de publieke shop liggen. De rest van de catalogus is
/// enkel via de Admin shop te koop — zo blijft alles verzamelen een werk van dagen.
const SHOP_DAILY_N: i64 = 4;

/// De publieke shop toont het volledige ontwerp (dagrotatie + Hytale-passen, zoals de Admin
/// shop preview), maar twee onderdelen zijn nog niet vrijgegeven en staan grijs:
///
/// * `SHOP_DAILY_PICKS_LIVE = false` → de 4 dagvakjes zijn grijze 🔒-placeholders i.p.v. echte
///   gems. Op **groen licht** → `true` → de echte gem-rotatie verschijnt (+ admin-reroll).
/// * `SHOP_PERMA_PASS_LIVE = false` → de permanente Hytale-pas staat grijs (nog niet te koop,
///   wacht op de server-mod). De **dagpas blijft gewoon koopbaar**. Later → `true`.
const SHOP_DAILY_PICKS_LIVE: bool = true;
const SHOP_PERMA_PASS_LIVE: bool = false;

/// De dag waarop de shop-rotatie draait (hele dagen sinds epoch, UTC → rolt om
/// middernacht UTC, 01:00/02:00 Brusselse tijd).
fn shop_day() -> i64 {
    (now_secs() / 86400.0).floor() as i64
}
// Meadowcoins-emoji als inline afbeelding (Discord-CDN); schaalt mee met font-size (1em).
const MC: &str = "<img class=\"mc\" src=\"https://cdn.discordapp.com/emojis/1526188363110023308.png?size=48\" alt=\"coins\">";
// Ticket-afbeelding voor de 24h-pas, ingebakken in de binary (geserveerd op /img/ticket.png).
const TICKET_IMG: &[u8] = include_bytes!("../artwork/24hHytale.png");
const CHEST_PNG: &[u8] = include_bytes!("../artwork/treasure chest.png"); // chest-embed image via URL (/img/chest.png)
/// Ronde Hytale-knop; draagt de aflopende pas-timer op de Coins-tab (/img/hytalepass.png).
const HYTALE_PASS_PNG: &[u8] = include_bytes!("../artwork/HytalePass_Button.png");
/// "Spicy Sale"-display-font (1001fonts.com, gratis personal+commercial), ingebakken in de
/// binary en geserveerd op /fonts/spicy-sale.ttf — gebruikt voor de "Basic Gems"-titel.
const SPICY_SALE_TTF: &[u8] = include_bytes!("../artwork/fonts/spicy-sale.ttf");
// Prod-guild (Magic Meadow): de coins-beheerpagina + kanalen-picklist lezen hiervan.
const COINS_GUILD_ID: &str = "1296469405651435592";
// Coins-aankondigingskanaal per omgeving (aankoopmeldingen). Prod = 🪙meadowcoins, dev = coins.
const PROD_COINS_CHANNEL_ID: &str = "1403044480218824794";
const DEV_COINS_CHANNEL_ID: &str = "1525189157104648343";
// Auto-refresh voor admin-pagina's: herlaad elke 20s, tenzij je in een veld typt/kiest.
/// Herlaadt de pagina periodiek, maar **niet** terwijl je in een veld staat — anders
/// verdwijnt een half ingetypte waarde onder je handen. Scrollpositie: zie KEEP_SCROLL_JS.
fn auto_refresh_js(ms: u32) -> String {
    format!(
        "<script>setInterval(function(){{var a=document.activeElement;\
           if(a&&(a.tagName==='INPUT'||a.tagName==='SELECT'))return;location.reload();}},{ms});</script>"
    )
}
/// Log/Coins/Channels: daar zit je te lézen, een herlaadflits om de paar seconden helpt niemand.
const AUTO_REFRESH_MS: u32 = 20_000;
/// Shop: een admin die voorraad bijvult wil dat meteen zien landen (user-wens 2026-07-15).
const AUTO_REFRESH_SHOP_MS: u32 = 5_000;
// Bewaart de scrollpositie vóór een form-submit en herstelt ze na de POST→redirect
// reload, zodat CRUD-acties (delete/update/upload/add) de pagina niet naar boven
// laten springen. Per-pad gesleuteld in sessionStorage.
const KEEP_SCROLL_JS: &str = "<script>(function(){var K='mmScroll:'+location.pathname;\
var y=sessionStorage.getItem(K);if(y!==null){sessionStorage.removeItem(K);\
window.scrollTo(0,+y);}document.addEventListener('submit',function(){\
sessionStorage.setItem(K,window.scrollY);},true);})();</script>";
/// Na een bewaaractie: strip de ?saved-parameter uit de URL (zodat een refresh
/// niet opnieuw flitst) en laat de "✓ Saved"-badge na 2,5s wegfaden.
const SAVED_FLASH_JS: &str = "<script>(function(){\
if(location.search.indexOf('saved=')>-1){history.replaceState({},'',location.pathname);}\
setTimeout(function(){document.querySelectorAll('.savedflash').forEach(function(e){e.style.opacity='0';});},2500);\
})();</script>";
/// Elk item-update-formulier persisteert zich AUTOMATISCH bij een veldwijziging
/// (op 'change', via sendBeacon zodat het ook een navigatie overleeft). Zo gaat een
/// getypte prijs nooit meer verloren als je daarna op Upload/◀▶/Move klikt (aparte
/// formulieren die het prijsveld niet meesturen). Toont een korte "✓ Saved"-flits.
const AUTOSAVE_JS: &str = "<script>(function(){\
document.querySelectorAll('form[action=\"/admin/item/update\"]').forEach(function(f){\
f.addEventListener('change',function(){\
var q=new URLSearchParams(new FormData(f)).toString();\
try{var ok=navigator.sendBeacon&&navigator.sendBeacon(f.action,new Blob([q],{type:'application/x-www-form-urlencoded'}));\
if(!ok)fetch(f.action,{method:'POST',headers:{'Content-Type':'application/x-www-form-urlencoded'},body:q,keepalive:true});}catch(e){}\
var c=f.closest('.aitem');if(!c)return;var fl=c.querySelector('.autoflash');\
if(!fl){fl=document.createElement('div');fl.className='autoflash';c.appendChild(fl);}\
fl.textContent='\\u2713 Saved';fl.style.opacity='1';\
clearTimeout(fl._t);fl._t=setTimeout(function(){fl.style.opacity='0';},1500);\
});});})();</script>";
/// Drag-&-drop op de afbeeldingskaders in Manage: sleep een afbeelding op een frame
/// → het bestand gaat in de bijhorende upload-form en die submit meteen (dezelfde
/// multipart-route als de knop). Puur client-side, geen serverlast.
const DND_JS: &str = "<script>(function(){\
document.querySelectorAll('.imgblock,.img2box').forEach(function(box){\
var form=box.querySelector('form.iupload');if(!form)return;\
var input=form.querySelector('input[type=file]');if(!input)return;\
['dragenter','dragover'].forEach(function(ev){box.addEventListener(ev,function(e){e.preventDefault();box.classList.add('dragover');});});\
['dragleave','dragend','drop'].forEach(function(ev){box.addEventListener(ev,function(e){box.classList.remove('dragover');});});\
box.addEventListener('drop',function(e){e.preventDefault();\
var f=e.dataTransfer&&e.dataTransfer.files&&e.dataTransfer.files[0];if(!f)return;\
try{var dt=new DataTransfer();dt.items.add(f);input.files=dt.files;}catch(err){return;}\
try{sessionStorage.setItem('mmScroll:'+location.pathname,window.scrollY);}catch(e2){}\
form.submit();});});})();</script>";
/// Klik op een (bezeten) gem-kaart → toon je naam live in díe gem-kleur op de
/// preview-swatches (dark/light/Discord-achtergrond). Louter een voorbeeld; Use zet de
/// kleur echt vast. Markeert de gekozen kaart.
const GEM_PREVIEW_JS: &str = "<script>(function(){\
var sw=document.querySelectorAll('.nameshow .swatch');\
document.querySelectorAll('.gemcard.previewable').forEach(function(card){\
card.addEventListener('click',function(e){\
if(e.target.closest('form'))return;\
var col=card.dataset.color;if(!col)return;\
sw.forEach(function(s){s.style.color=col;});\
document.querySelectorAll('.gemcard.previewsel').forEach(function(x){x.classList.remove('previewsel');});\
card.classList.add('previewsel');});});})();</script>";
const UPLOAD_DIR: &str = "uploads"; // in WorkingDirectory (/opt/market/uploads op prod)
/// Standaardduur van een Hytale-dagpas, enkel nog gebruikt bij het **seeden** van een
/// nieuwe pas. De echte duur is sinds 2026-07-15 weer **per item instelbaar** in
/// Manage → Shop (veld "Access (minutes)" → `items.duration`), en `buy()` leest dát.
/// Was tijdelijk hardcoded; op vraag van de user terug instelbaar gemaakt zodat verval
/// te testen valt zonder een dag te wachten. `duration == 0` blijft "permanente pas".
#[allow(dead_code)]
const DAY_PASS_SECS: i64 = 24 * 3600;

#[derive(Clone)]
struct AppState {
    cfg: Config,
    dc: Arc<Discord>,
    pool: DbPool,
    http: reqwest::Client,
}

type JsonResp = (StatusCode, Json<Value>);

/// De guild waaruit de gem-kleuren (Discord-rollen) gelezen worden: dev-guild in de
/// dev-omgeving, prod-guild (Magic Meadow) in prod.
fn color_guild(cfg: &Config) -> String {
    if cfg.environment.eq_ignore_ascii_case("dev") {
        cfg.guild_id.clone()
    } else {
        COINS_GUILD_ID.to_string()
    }
}

/// Het coins-kanaal voor aankoopmeldingen: dev-kanaal in dev, prod #coins in prod.
fn coins_channel(cfg: &Config) -> &'static str {
    if cfg.environment.eq_ignore_ascii_case("dev") {
        DEV_COINS_CHANNEL_ID
    } else {
        PROD_COINS_CHANNEL_ID
    }
}

/// Tekst van de publieke aankoopmelding. Gems krijgen het achtervoegsel "gem"; passen en
/// boosters worden bij hun naam genoemd. `a`/`an` volgt de eerste letter van de naam.
fn purchase_announce(name: &str, item: &db::Item) -> String {
    let article = match item.name.chars().next().map(|c| c.to_ascii_lowercase()) {
        Some('a' | 'e' | 'i' | 'o' | 'u') => "an",
        _ => "a",
    };
    // De naam als platte tekst (bewust géén mention/ping) — user-keuze 2026-07-18.
    if item.category == "inventory" {
        format!("**{name}** bought {article} **{}** gem.", item.name)
    } else {
        format!("**{name}** bought {article} **{}**.", item.name)
    }
}

/// Post de aankoopmelding in #coins (async, mag de redirect niet ophouden en een
/// Discord-hapering mag de aankoop niet breken).
fn announce_purchase(st: &AppState, name: &str, item: &db::Item) {
    let dc = st.dc.clone();
    let chan = coins_channel(&st.cfg).to_string();
    let msg = purchase_announce(name, item);
    tokio::spawn(async move {
        let _ = dc.send_channel_message(&chan, &msg).await;
    });
}

pub async fn serve(cfg: Config, pool: DbPool) {
    let dc = Arc::new(Discord::new(cfg.bot_token.clone(), cfg.guild_id.clone()));
    let state = AppState {
        cfg,
        dc,
        pool,
        http: reqwest::Client::new(),
    };

    let _ = std::fs::create_dir_all(UPLOAD_DIR);

    // Gem-kleuren synchroniseren uit de Discord-rollen (van de omgeving-guild) bij opstart:
    // gem-naam = rolnaam → kleur van die rol wordt de kleur van de gem.
    {
        let cg = color_guild(&state.cfg);
        match state.dc.list_roles(&cg).await {
            Ok(roles) => {
                let n = db::sync_gem_colors(&state.pool, &roles);
                tracing::info!("gem-kleuren gesynct: {n} items ({} rollen, guild {cg})", roles.len());
            }
            Err(e) => tracing::warn!("gem-kleur-sync bij opstart overgeslagen: {e}"),
        }
    }

    let app = Router::new()
        .route("/", get(index))
        .route("/market", get(market))
        .route("/inventory", get(inventory))
        .route("/leaderboard", get(leaderboard_page))
        .route("/public", post(set_public_route))
        .route("/buy", post(buy))
        .route("/use/gem", post(use_gem))
        .route("/use/gem/unequip", post(unequip_gem))
        .route("/admin/market", get(admin_market))
        .route("/admin/inventory", get(admin_inventory_preview))
        .route("/admin/shop/preview", get(admin_shop_preview))
        .route("/admin/shop/reroll", post(admin_shop_reroll))
        .route("/admin/shelf/add", post(admin_shelf_add))
        .route("/admin/shelf/rename", post(admin_shelf_rename))
        .route("/admin/shelf/delete", post(admin_shelf_delete))
        .route("/admin/item/add", post(admin_item_add))
        .route("/admin/item/update", post(admin_item_update))
        .route("/admin/item/delete", post(admin_item_delete))
        .route("/admin/item/stock", post(admin_item_stock))
        .route("/admin/accounts", get(admin_accounts))
        .route("/admin/inactives", get(admin_inactives))
        .route("/admin/item/move", post(admin_item_move))
        .route("/admin/item/shelf", post(admin_item_shelf))
        .route("/admin/item/image/clear", post(admin_item_image_clear))
        .route("/admin/item/image2/clear", post(admin_item_image2_clear))
        .route("/admin/reset-collection", post(admin_reset_collection))
        .route("/admin/sync-gem-colors", post(admin_sync_gem_colors))
        .route("/admin/coins", get(admin_coins))
        .route("/admin/coins/add", post(admin_coins_add))
        .route("/admin/coins/set", post(admin_coins_set))
        .route("/admin/coins/undo", post(admin_coins_undo))
        .route("/admin/coins/restore", post(admin_coins_restore))
        .route("/admin/coins/discard", post(admin_coins_discard))
        .route("/admin/log", get(admin_log))
        .route("/admin/refund", post(admin_refund))
        .route("/admin/channels", get(admin_channels))
        .route("/admin/channels/add", post(admin_channels_add))
        .route("/admin/channels/remove", post(admin_channels_remove))
        .route("/admin/settings", get(admin_settings))
        .route("/admin/settings/save", post(admin_settings_save))
        .route("/admin/settings/weight/set", post(admin_settings_weight_set))
        .route("/admin/settings/weight/delete", post(admin_settings_weight_delete))
        .route("/admin/settings/tier/add", post(admin_settings_tier_add))
        .route("/admin/settings/tier/update", post(admin_settings_tier_update))
        .route("/admin/settings/tier/delete", post(admin_settings_tier_delete))
        .route(
            "/admin/item/image",
            post(admin_item_image).layer(DefaultBodyLimit::max(8 * 1024 * 1024)),
        )
        .route("/uploads/{name}", get(serve_upload))
        .route("/img/ticket.png", get(serve_ticket))
        .route("/img/chest.png", get(serve_chest))
        .route("/img/hytalepass.png", get(serve_hytale_pass))
        .route("/fonts/spicy-sale.ttf", get(serve_spicy_sale_font))
        .route("/info", get(info_page))
        .route("/login", get(login))
        .route("/auth/callback", get(callback))
        .route("/logout", get(logout))
        .route("/admin", get(admin))
        .route("/api/status", get(api_status))
        .route("/api/toggle", post(api_toggle))
        .route("/api/balance", get(api_balance))
        .route("/healthz", get(|| async { "ok" }))
        // Dienst-tot-dienst, niet voor browsers: Caddy blokkeert /internal/* van buitenaf.
        .route("/internal/pass/revoke", post(internal_revoke_pass))
        .with_state(state);

    // Poort: default 8700, overschrijfbaar met MARKET_PORT (bv. voor lokale tests naast
    // een andere instance).
    let port: u16 = std::env::var("MARKET_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8700);
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|e| panic!("kan poort {port} niet binden: {e}"));
    tracing::info!("Web-server luistert op http://0.0.0.0:{port}");
    axum::serve(listener, app).await.expect("web-server crashte");
}

// De site-brede `gate`-middleware is weg (2026-07-15, go-live): die stuurde élke niet-admin
// naar /info, waardoor de embed-knop voor gewone leden doodliep. Toegang loopt nu per route
// via het eigen slot: `require_flowerborn` op de ledenpagina's (shop/inventory/leaderboard/
// buy/use), `require_admin` op alles onder /admin én op de oude toggle-UI + /api/status +
// /api/toggle. Die laatste drie leunden vroeger volledig op de gate — vandaar dat ze bij het
// weghalen ervan hun eigen check kregen (zonder dat kon eender wie zichzelf Flowerborn maken).

// --- helpers ------------------------------------------------------------

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

/// Willekeurige hex-token (sessie-id / CSRF-state).
fn rand_token() -> String {
    let mut rng = rand::thread_rng();
    (0..32)
        .map(|_| char::from_digit(rng.gen_range(0..16), 16).unwrap())
        .collect()
}

/// Percent-encode voor query-waarden (RFC 3986 unreserved blijft staan).
fn pct(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Waarde van één cookie uit de `Cookie`-header.
fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers.get(COOKIE)?.to_str().ok()?.split(';').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k.trim() == name).then(|| v.trim().to_string())
    })
}

fn set_cookie(pair: &str) -> HeaderMap {
    let mut h = HeaderMap::new();
    if let Ok(v) = HeaderValue::from_str(pair) {
        h.insert(SET_COOKIE, v);
    }
    h
}

/// Meerdere `Set-Cookie`-headers in één response. Belangrijk: `append`, niet
/// `insert` — anders overschrijft de tweede cookie de eerste (zelfde header-naam).
fn set_cookies(pairs: &[&str]) -> HeaderMap {
    let mut h = HeaderMap::new();
    for pair in pairs {
        if let Ok(v) = HeaderValue::from_str(pair) {
            h.append(SET_COOKIE, v);
        }
    }
    h
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn shell(title: &str, nav: &str, wide: bool, body: &str) -> String {
    let card_cls = if wide { "card wide" } else { "card" };
    // Logout hoort bij een ingelogde sessie; de nav is leeg op de login-pagina.
    let logout = if nav.is_empty() {
        String::new()
    } else {
        "<a class=\"tb-logout\" href=\"/logout\">↪ Log out</a>".to_string()
    };
    // Live-refresh: pol elke 5s /api/balance en werk de getagde elementen bij
    // (saldo, all-time, level). Enkel op ingelogde pagina's met zo'n element.
    let poller = if nav.is_empty() {
        String::new()
    } else {
        "<script>(function(){\
           if(!document.querySelector('[data-bal],[data-earned]'))return;\
           function set(s,v){document.querySelectorAll(s).forEach(function(e){e.textContent=v;});}\
           function upd(){fetch('/api/balance',{cache:'no-store'})\
             .then(function(r){return r.ok?r.json():null;})\
             .then(function(d){if(!d||!d.ok)return;\
               set('[data-bal]',d.coins);set('[data-earned]',d.earned);\
               var b=document.querySelector('[data-lvl]');if(b)b.textContent='Lv '+d.lvl;\
               var f=document.querySelector('[data-fill]');if(f)f.style.width=d.pct+'%';\
               var n=document.querySelector('[data-lvlnm]');if(n)n.textContent=d.nm;})\
             .catch(function(){});}\
           setInterval(upd,5000);})();</script>"
            .to_string()
    };
    format!(
        r#"<!doctype html><html lang="nl"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title}</title><style>
:root{{color-scheme:light dark}}
*{{box-sizing:border-box}}
body{{margin:0;min-height:100vh;display:flex;flex-direction:column;
  font:16px/1.5 system-ui,sans-serif;background:#0e1510;color:#e8f0e4}}
.topbar{{background:{MEADOW};color:#0e1510;padding:.6rem 1.1rem;
  box-shadow:0 2px 10px rgba(0,0,0,.35);
  display:flex;align-items:center;justify-content:space-between;gap:1rem}}
.brand{{font-weight:700;font-size:1.1rem;letter-spacing:.02em}}
.tb-logout{{color:#0e1510;text-decoration:none;font-weight:700;font-size:.9rem;
  background:rgba(14,21,16,.12);padding:.35rem .8rem;border-radius:999px;white-space:nowrap}}
.tb-logout:hover{{background:rgba(14,21,16,.22)}}
.uname{{font-weight:800;font-size:1.25rem;letter-spacing:.01em;margin:0 0 .7rem}}
.nav{{display:flex;gap:.3rem;margin:0 0 1.3rem;flex-wrap:wrap}}
.nav a{{flex:1 1 auto;text-align:center;padding:.5rem .7rem;border-radius:11px;
  text-decoration:none;color:#cfe0c8;font-weight:600;font-size:.92rem;
  background:#141d14;white-space:nowrap}}
.nav a.active{{background:{MEADOW};color:#0e1510}}
.nav a:hover:not(.active){{background:#20301e}}
.bigname{{font-size:2rem;font-weight:800;text-align:center;letter-spacing:.01em;
  margin:.2rem 0 1.1rem}}
.subtabs{{display:flex;gap:.3rem;margin:0 0 1.2rem;flex-wrap:wrap}}
.subtabs.center{{justify-content:center}}
.subtab{{padding:.4rem .85rem;border-radius:9px;cursor:pointer;font-weight:600;
  font-size:.92rem;color:#9db095;background:transparent;border:1px solid #2c3d2a}}
.subtab.on{{background:#141d14;color:#e8f0e4;border-color:#3a4d38}}
a.subtab{{text-decoration:none;display:inline-block}}
.panel{{display:none}}
.panel.on{{display:block}}
.mc{{height:1em;width:auto;vertical-align:-0.15em}}
details.acc{{background:#141d14;border:1px solid #2c3d2a;border-radius:11px;margin:.55rem 0}}
details.acc>summary{{cursor:pointer;padding:.75rem 1rem;font-weight:700;color:#e8f0e4;list-style:none;display:flex;align-items:center;gap:.55rem}}
details.acc>summary::-webkit-details-marker{{display:none}}
details.acc>summary::after{{content:'▸';margin-left:auto;color:#8fb37a;transition:transform .15s}}
details.acc[open]>summary::after{{transform:rotate(90deg)}}
details.acc>p{{margin:0;padding:0 1rem .85rem;color:#c8d6c0;line-height:1.45}}
details.acc .mc{{height:1.1em;vertical-align:-.2em}}
.earned{{font-size:2.6rem;font-weight:800;color:{MEADOW};text-align:center;margin:.2rem 0 0;line-height:1}}
.levelrow{{display:flex;align-items:center;gap:.6rem;margin:1.1rem 0}}
.lvlbadge{{background:{MEADOW};color:#0e1510;font-weight:800;border-radius:9px;
  padding:.3rem .6rem;font-size:.9rem;white-space:nowrap}}
.bar{{flex:1;height:14px;background:#0e1510;border:1px solid #2c3d2a;border-radius:999px;overflow:hidden}}
.fill{{height:100%;background:linear-gradient(90deg,#3f6a2c,{MEADOW});border-radius:999px;transition:width .4s}}
.lvlnm{{font-variant-numeric:tabular-nums;font-weight:700;color:#cfe0c8;font-size:.85rem;white-space:nowrap}}
.content{{flex:1;display:grid;justify-items:center;align-items:start;
  row-gap:1.1rem;padding:1.2rem 1rem}}
.card{{background:#182319;border:1px solid #2c3d2a;border-radius:18px;
  padding:2rem 2.25rem;max-width:28rem;width:calc(100% - 2rem);
  box-shadow:0 10px 30px rgba(0,0,0,.35)}}
.card.wide{{max-width:64rem}}
.navcard{{padding:1rem 1.25rem}}
.navcard .uname{{margin:0 0 .7rem}}
.navcard .nav{{margin:0}}
h1{{margin:.2rem 0 1rem;font-size:1.35rem}}
.coins{{font-size:2.4rem;font-weight:700;color:{MEADOW};margin:.4rem 0}}
.muted{{color:#9db095;font-size:.9rem}}
/* Ja/Nee-badges in de accounts-tabel: groen = actief, gedempt rood = niet. */
.yes{{color:#8fb37a;font-weight:700}}
.no{{color:#9a6b62}}
/* Knoppen voelen als echte knoppen: een 'rand' eronder (box-shadow) die bij het
   indrukken wegvalt terwijl de knop zakt. Zelfde beleving als de Buy-knop.
   De schaduwkleur is per variant een donkerdere versie van de knop zelf.
   (Klik-geluid volgt later — de user bezorgt het sample.) */
a.btn,button.btn{{display:inline-block;margin-top:1rem;padding:.7rem 1.15rem;
  border:0;border-radius:12px;background:{MEADOW};color:#0e1510;font-weight:600;
  text-decoration:none;cursor:pointer;font-size:1rem;
  box-shadow:0 3px 0 #3a5a28;transition:transform .05s,box-shadow .05s,filter .1s}}
a.btn:hover,button.btn:hover{{filter:brightness(1.06)}}
a.btn:active,button.btn:active{{transform:translateY(3px);box-shadow:0 0 0 #3a5a28}}
button.btn:disabled{{box-shadow:none;transform:none;filter:none}}
a.link{{color:{MEADOW}}}
.statrow{{display:flex;justify-content:space-between;align-items:center;
  padding:.7rem .1rem;border-top:1px solid #22301f}}
.statrow .k{{color:#9db095;font-size:.92rem}}
.pill{{display:inline-block;padding:.25rem .65rem;border-radius:999px;
  font-size:.8rem;font-weight:700}}
.pill.on{{background:{MEADOW};color:#0e1510}}
.pill.off{{background:#2c3d2a;color:#9db095}}
.switch{{position:relative;display:inline-block;width:48px;height:28px;flex:none}}
.switch input{{opacity:0;width:0;height:0}}
.slider{{position:absolute;inset:0;background:#2c3d2a;border-radius:999px;
  cursor:pointer;transition:background .2s}}
.slider::before{{content:"";position:absolute;height:22px;width:22px;left:3px;top:3px;
  background:#f4f8f1;border-radius:50%;transition:transform .2s;
  box-shadow:0 1px 3px rgba(0,0,0,.45)}}
.switch input:checked + .slider{{background:{MEADOW}}}
.switch input:checked + .slider::before{{transform:translateX(20px)}}
.grants{{display:flex;flex-direction:column;gap:.4rem}}
.grant{{display:flex;justify-content:space-between;align-items:center;
  padding:.5rem .7rem;border-radius:10px;background:#141d14;border:1px solid #2c3d2a}}
.glabel{{font-weight:600;font-size:.9rem}}
.gtime{{font-variant-numeric:tabular-nums;font-weight:700;color:{MEADOW}}}
.grant.expired{{opacity:.55}}
.grant.expired .gtime{{color:#9db095}}
.soon{{margin-top:1rem;padding:.8rem 1rem;border:1px dashed #3a4d38;
  border-radius:12px;color:#9db095;font-size:.92rem}}
.slots{{display:grid;grid-template-columns:repeat(auto-fit,minmax(130px,1fr));gap:.7rem;margin:.6rem 0 0}}
.slot{{background:#141d14;border:1px solid #2c3d2a;border-radius:12px;padding:.7rem;
  display:flex;flex-direction:column;gap:.5rem;text-align:center}}
.slot .thumb{{aspect-ratio:1;border-radius:9px;background:#0e1510;
  border:1px solid #26331f;display:grid;place-items:center}}
.slot .gem{{width:66%;aspect-ratio:1;border-radius:50%;
  box-shadow:inset 0 -4px 8px rgba(0,0,0,.35),0 2px 6px rgba(0,0,0,.3)}}
.slot .name{{font-size:.82rem;font-weight:600;color:#e8f0e4;
  overflow:hidden;text-overflow:ellipsis;white-space:nowrap}}
.slot .sdesc{{font-size:.72rem;color:#bccdb3;line-height:1.35;font-style:italic}}
/* Prijs + Buy (+ Hytale-naamveld) als groep onderaan de kaart: alle Buy-knoppen lijnen
   uit, ongeacht hoeveel tekst/omschrijving er boven staat. */
.slot .price{{font-weight:700;color:{MEADOW};font-size:.95rem;margin-top:auto}}
.slot .buy{{width:100%;padding:.45rem;border:0;border-radius:9px;
  background:#2c3d2a;color:#6f8268;font-weight:600;font-size:.9rem;
  cursor:not-allowed}}
.buyform{{margin:0;width:100%}}
.buy.on{{background:{MEADOW};color:#0e1510;cursor:pointer;
  box-shadow:0 3px 0 #3a5a28;transition:transform .05s,box-shadow .05s,filter .1s}}
.buy.on:hover{{filter:brightness(1.06)}}
.buy.on:active{{transform:translateY(3px);box-shadow:0 0 0 #3a5a28}}
.buy.owned{{width:100%;padding:.45rem;border:0;border-radius:9px;
  background:#2c3d2a;color:#8aa07f;font-weight:600;font-size:.9rem;cursor:default}}
.shophead{{display:flex;justify-content:space-between;align-items:center;
  gap:1rem;flex-wrap:wrap}}
.shophead h1{{margin:.2rem 0}}
/* Zwevende Purse: rechts uitgelijnd block-kind van de content-card, blijft tijdens het
   scrollen bovenaan vastgeklikt zichtbaar. */
.purse-box{{font-size:1.6rem;font-weight:800;color:#cfe0c8;background:#141d14;
  border:1px solid #2c3d2a;padding:.45rem 1.1rem;border-radius:14px;white-space:nowrap;
  position:sticky;top:.6rem;z-index:30;box-shadow:0 4px 16px rgba(0,0,0,.45);
  width:fit-content;margin-left:auto}}
.shoptitle{{margin:.1rem 0 .6rem}}
.purse-box .purse-n{{color:{MEADOW};font-variant-numeric:tabular-nums}}
.notice{{padding:.6rem .9rem;border-radius:10px;margin:.2rem 0 1rem;font-size:.92rem}}
.notice.ok{{background:#1f3320;color:#bfe3b0;border:1px solid #2f5a2c}}
.notice.err{{background:#3a201c;color:#f0c9c0;border:1px solid #6e352c}}
.shelf{{display:flex;gap:.6rem;overflow-x:auto;padding:.2rem 0 .5rem;align-items:stretch}}
/* Inventory: gems mogen niet in een zijwaartse schuifstrip verdwijnen — laat ze
   gewoon doorlopen en afbreken over een paar rijen, zodat je alles in één blik ziet. */
.shelf.wrap{{flex-wrap:wrap;overflow-x:visible;justify-content:center}}
/* Inventory-gems (en Trinkets): vast 6-per-rij grid i.p.v. de flex-wrap-strip, zodat de
   rijen netjes uitlijnen. minmax(0,1fr) laat de kaarten samen de rij delen; kaders rekken
   in hoogte mee zodat alle tekst past. Op smallere schermen minder kolommen (leesbaarheid). */
.shelf.gems6{{display:grid;grid-template-columns:repeat(6,minmax(0,1fr));gap:.6rem;
  overflow-x:visible;justify-content:initial;align-items:stretch}}
.shelf.gems6 .slot{{width:auto;flex:initial}}
@media (max-width:820px){{.shelf.gems6{{grid-template-columns:repeat(3,minmax(0,1fr))}}}}
@media (max-width:480px){{.shelf.gems6{{grid-template-columns:repeat(2,minmax(0,1fr))}}}}
.shelf .slot{{flex:0 0 auto;width:170px}}
.shelf .slot .thumb{{font-size:1.2rem}}
.shelf .slot .name{{white-space:normal;overflow:visible}}
.shelf.shop .slot{{width:210px}}
/* Enkel de dagpicks horizontaal centreren (de passen-rij blijft links). `safe` valt terug
   op links als de rij te breed wordt, zodat er op smalle schermen niks wegvalt. */
.shelf.picks{{justify-content:safe center}}
.shelf.shop .slot .name{{white-space:nowrap;overflow:hidden;text-overflow:ellipsis}}
.shelf-title{{margin:1.3rem 0 .2rem;font-size:1rem;color:#cfe0c8;font-weight:700;
  display:flex;align-items:center;gap:.5rem}}
@font-face{{font-family:'Spicy Sale';src:url('/fonts/spicy-sale.ttf') format('truetype');font-display:swap}}
.shelf-title.center{{justify-content:center}}
/* Basic Gems-titel in de sier-font 'Spicy Sale' — iets groter zodat het karakter opvalt. */
.shelf-title.fancy{{font-family:'Spicy Sale',cursive;font-size:1.9rem;font-weight:400;letter-spacing:.02em}}
.reroll-f{{display:inline;margin:0}}
.reroll{{background:transparent;border:1px solid #2c3d2a;color:#9db095;border-radius:999px;
  width:1.6rem;height:1.6rem;padding:0;font-size:.9rem;line-height:1;cursor:pointer;
  display:inline-flex;align-items:center;justify-content:center}}
.reroll:hover{{background:#20301e;color:#e8f0e4;border-color:#3a4d38}}
/* Afteller naar de volgende dag-refresh (middernacht UTC). Inline naast de titel, op
   dezelfde plek waar de admin-reroll staat (niet naar rechts geduwd). */
.shop-countdown{{font-size:.78rem;font-weight:600;color:#9db095;
  background:#141d14;border:1px solid #2c3d2a;border-radius:999px;padding:.22rem .6rem;
  white-space:nowrap;font-variant-numeric:tabular-nums}}
.shop-countdown b{{color:#cfe0c8}}
/* Nog niet vrijgegeven: grijs, niet-klikbaar placeholder-vakje met een slotje. */
/* Teaser = één strak, uniform grijs vak met het slotje gecentreerd. De thumb HOUDT
   aspect-ratio:1 (nodig: dat laat `align-items:stretch` het vak wél naar de volle
   pashoogte rekken — een `flex:1`-thumb blokkeert die stretch), maar krijgt géén eigen
   rand/achtergrond, zodat het geen doosje-in-een-doos is. `justify-content:center` zet
   het slotje in het midden van het (uitgerekte) vak. */
.slot.soon{{opacity:.5;filter:grayscale(.75);justify-content:center}}
.slot.soon .thumb{{border:0;background:transparent;font-size:2.2rem}}
/* Ronde Hytale-knop onderaan de Coins-tab, met de pas-timer eróver. De H in de
   afbeelding is druk, dus de tijd krijgt een donker pilletje — anders leest hij niet. */
.passbtn{{position:relative;width:125px;margin:1.4rem auto .2rem;line-height:0}}
.passbtn img{{width:100%;height:auto;border-radius:50%;display:block;
  box-shadow:0 6px 18px rgba(0,0,0,.45)}}
.passtime{{position:absolute;left:50%;top:50%;transform:translate(-50%,-50%);
  background:rgba(14,21,16,.82);color:#e8f0e4;font:800 1.9rem/1.2 system-ui,sans-serif;
  padding:.1rem .5rem;border-radius:999px;white-space:nowrap;
  font-variant-numeric:tabular-nums;border:1px solid rgba(232,240,228,.18)}}
.passtime.out{{background:rgba(192,86,47,.9);color:#fff}}
/* Zwevende naam-preview: blijft bovenaan zichtbaar terwijl je door de gems scrolt. */
.nameshow{{display:flex;gap:.6rem;margin:.2rem 0 1rem;flex-wrap:wrap;
  position:sticky;top:.6rem;z-index:20;background:#182319;padding:.5rem;
  border-radius:14px;box-shadow:0 4px 16px rgba(0,0,0,.45)}}
/* Discord-achtig lettertype voor een realistische naam-preview. */
.swatch{{flex:1 1 140px;text-align:center;padding:.7rem;border-radius:11px;
  font-weight:700;font-size:1.25rem;border:1px solid #2c3d2a;
  font-family:'gg sans','Noto Sans','Helvetica Neue',Arial,sans-serif;transition:color .15s}}
.swatch.dark{{background:#141414}}
.swatch.light{{background:#ffffff}}
.preview-hint{{text-align:center;font-size:.72rem;margin:-.4rem 0 .8rem}}
.gemcard.previewable{{cursor:pointer}}
.gemcard.previewsel{{outline:2px solid {MEADOW};outline-offset:2px}}
.gemcard .gdesc{{font-size:.7rem;color:#9db095;line-height:1.25}}
/* Duw de Use-knop naar de onderkant van de (uitgerekte) kaart, zodat de knoppen op één rij
   uitlijnen ook al is de ene omschrijving langer dan de andere. */
.gemcard .buyform{{margin-top:auto}}
.booster-active{{font-size:.72rem;color:#c9a227;font-weight:700;text-align:center;margin:.15rem 0 .3rem}}
.booster-banner{{background:rgba(201,162,39,.12);border:1px solid rgba(201,162,39,.5);
  color:#e0c56a;border-radius:10px;padding:.55rem .8rem;margin:.6rem 0;font-size:.85rem;font-weight:600}}
.slot.locked{{opacity:.5}}
.slot.locked .thumb{{display:grid;place-items:center}}
.slot .lock{{font-size:1.5rem;filter:grayscale(1)}}
/* Vergrendelde verzamelkaart: grijs vak met een groot vraagteken. */
.slot .qmark{{font-size:2.4rem;font-weight:800;color:#4c5c46}}
.slot.locked .name.muted{{color:#5a6b52}}
/* Reeds gekocht in de shop: kaart grijs + groene checkmark, geen Buy-knop. */
.slot.bought{{opacity:.55;position:relative}}
.slot.bought .boughtmark{{position:absolute;top:.5rem;right:.5rem;z-index:2;
  width:1.5rem;height:1.5rem;border-radius:50%;background:{MEADOW};color:#08240f;
  display:grid;place-items:center;font-weight:900;font-size:1rem;
  box-shadow:0 1px 4px rgba(0,0,0,.5)}}
.buy.on.eq{{background:#2c3d2a;color:#cfe0c8;cursor:default}}
.slot .thumb img{{width:100%;height:100%;object-fit:contain;border-radius:9px}}
/* Tweede afbeelding onder de titel in de shop: kleiner, gecentreerd. */
.slot .thumb2{{margin:.25rem auto .1rem;text-align:center;line-height:0}}
.slot .thumb2 img{{max-width:62%;max-height:64px;object-fit:contain;border-radius:7px}}
.gem-empty{{background:#22301f}}
/* Kleine knop: ondiepere 'rand', anders oogt hij log. */
.btn.small{{margin-top:0;padding:.35rem .55rem;font-size:.82rem;border-radius:8px;
  box-shadow:0 2px 0 #3a5a28}}
.btn.small:active{{transform:translateY(2px);box-shadow:0 0 0 #3a5a28}}
/* Elke variant z'n eigen donkerdere onderrand, anders steekt er groen onder rood uit. */
.btn.ghost{{background:#2c3d2a;color:#cfe0c8;box-shadow:0 2px 0 #1c2a1b}}
.btn.ghost:active{{box-shadow:0 0 0 #1c2a1b}}
.btn.danger{{background:#7a2f28;color:#f3d9d4;box-shadow:0 2px 0 #4d1d18}}
.btn.danger:active{{box-shadow:0 0 0 #4d1d18}}
.aitems{{display:flex;flex-wrap:wrap;gap:.7rem;align-items:flex-start;margin-top:.5rem}}
/* 168px was te smal om de omschrijving/naam deftig te lezen. Breder, en de kaart mag
   op smalle schermen krimpen i.p.v. buiten beeld te lopen. */
.aitem{{position:relative;width:240px;max-width:100%;background:#141d14;border:1px solid #2c3d2a;
  border-radius:12px;padding:.6rem;display:flex;flex-direction:column;gap:.4rem}}
.savedflash{{position:absolute;top:.45rem;right:.45rem;z-index:2;background:#2f7a3a;
  color:#eafff0;font-size:.66rem;font-weight:800;padding:.15rem .45rem;border-radius:6px;
  box-shadow:0 1px 4px rgba(0,0,0,.4);transition:opacity .6s}}
/* Auto-save-flits (op veldwijziging), onderaan de kaart zodat hij de ✓ Saved-badge niet overlapt. */
.autoflash{{position:absolute;bottom:.45rem;right:.45rem;z-index:2;background:#2f7a3a;
  color:#eafff0;font-size:.62rem;font-weight:800;padding:.1rem .4rem;border-radius:6px;
  opacity:0;transition:opacity .3s;pointer-events:none}}
/* Beheerblok voor de tweede afbeelding (plain items). */
.img2box{{margin-top:.35rem;border-top:1px dashed #2c3d2a;padding-top:.4rem;
  display:flex;flex-direction:column;gap:.35rem}}
.img2box .thumb2{{aspect-ratio:1;border:1px solid #26331f;border-radius:9px;background:#0e1510;
  display:grid;place-items:center;overflow:hidden;max-height:88px}}
.img2box .thumb2 img{{max-width:100%;max-height:100%;object-fit:contain;border-radius:7px}}
.img2box .thumb2.empty{{font-size:.62rem;color:#6b7d63}}
/* Hoofdafbeelding: frame + browse/upload gegroepeerd bovenaan de kaart. */
.aitem .imgblock{{display:flex;flex-direction:column;gap:.35rem;
  border-bottom:1px dashed #2c3d2a;padding-bottom:.5rem;margin-bottom:.1rem}}
/* Veldlabels: kleine kop boven elk invoerveld, hint in lichter/dun font. */
.aitem .lbl{{font-size:.62rem;color:#9db095;font-weight:700}}
.aitem .fld{{display:flex;flex-direction:column;gap:.12rem;text-align:left;
  font-size:.62rem;color:#9db095;font-weight:700}}
.aitem .hint{{font-weight:400;color:#6b7d63}}
/* Voorraad in de shop: klein regeltje onder de prijs. */
.slot .stock{{font-size:.72rem;color:#9db095;font-weight:600;margin:.1rem 0 .2rem}}
.slot .stock.none{{color:#c0562f}}
/* Voorraadvak op de Manage-kaart: eigen formulier, dus visueel afgezet. */
.aitem .stockbox{{border-top:1px solid #2c3d2a;padding-top:.45rem;margin:.1rem 0 0;
  display:flex;flex-direction:column;gap:.3rem}}
.aitem .stockbox .lbl{{font-size:.72rem;color:#9db095;text-align:left}}
.aitem .stockbox .num{{width:4rem;background:#0e1510;border:1px solid #2c3d2a;color:#e8f0e4;
  border-radius:8px;padding:.25rem .35rem;font:inherit;font-size:.78rem}}
.aitem .stockbox .arow{{display:flex;gap:.3rem;align-items:center}}
.soldout{{color:#c0562f}}
/* Out-of-stock-vinkje op de item-kaart: op één regel, klikbaar label. */
.aitem .chk{{display:flex;align-items:center;gap:.4rem;font-size:.74rem;color:#cfe0c8;
  cursor:pointer;text-align:left;flex-wrap:wrap}}
.aitem .chk input{{accent-color:{MEADOW};width:.9rem;height:.9rem;flex:none;cursor:pointer}}
.aitem .rdonly{{font-weight:600;color:#cfe0c8;font-size:.74rem;padding:.3rem .4rem;
  border:1px dashed #2c3d2a;border-radius:7px;background:#0e1510}}
/* Drag-&-drop-feedback op een afbeeldingskader. */
.imgblock.dragover .thumb,.img2box.dragover .thumb2{{outline:2px dashed #6bbf59;outline-offset:2px}}
.imgblock.dragover,.img2box.dragover{{background:rgba(107,191,89,.06);border-radius:9px}}
.aitem .save{{width:100%;margin-top:.1rem}}
.arow{{display:flex;gap:.3rem;align-items:center}}
.arow .iform{{margin:0}}
.arow form:last-child{{margin-left:auto}}
.mvshelf{{display:flex;gap:.3rem;align-items:center;margin-top:.1rem}}
.mvshelf select{{flex:1;min-width:0}}
.aitem .thumb{{aspect-ratio:1;border:1px solid #26331f;border-radius:9px;
  background:#0e1510;display:grid;place-items:center;overflow:hidden}}
.aitem .thumb img{{max-width:100%;max-height:100%;object-fit:contain;border-radius:9px}}
.aitem input,.ashelf-head input[name=title]{{width:100%;padding:.35rem;
  border:1px solid #2c3d2a;border-radius:7px;background:#0e1510;color:#e8f0e4;
  font:inherit;font-size:.82rem}}
.aitem input[type=file]{{font-size:.68rem;color:#9db095;padding:.15rem 0;border:0}}
.aitem select{{width:100%;padding:.32rem;border:1px solid #2c3d2a;border-radius:7px;
  background:#0e1510;color:#e8f0e4;font:inherit;font-size:.8rem}}
.ibadge{{font-size:.72rem;font-weight:700;color:#cdbb6a}}
.chead{{display:flex;align-items:center;gap:1rem;flex-wrap:wrap}}
.chead h1{{margin:.2rem 0}}
.ctable{{width:100%;border-collapse:collapse;margin-top:.6rem}}
.ctable th,.ctable td{{padding:.45rem .6rem;border-bottom:1px solid #26331f;text-align:left;vertical-align:middle}}
.ctable th{{color:#9db095;font-size:.78rem;font-weight:700}}
.ctable .cbal{{font-variant-numeric:tabular-nums;color:#cfe0c8;white-space:nowrap}}
.ctable .cbal .mc{{height:1em;vertical-align:-.15em}}
.coinform{{display:flex;gap:.4rem;align-items:center;margin:0;flex-wrap:wrap}}
.coinform .cbx{{font-size:.72rem;color:#9db095;display:inline-flex;align-items:center;gap:.2rem;white-space:nowrap;cursor:pointer}}
.coinform input[type=number]{{width:90px;padding:.32rem;border:1px solid #2c3d2a;border-radius:7px;background:#0e1510;color:#e8f0e4;font:inherit}}
.undoform{{margin:0}}
.undonote{{font-size:.8rem}}
.archline{{display:flex;gap:.4rem;align-items:center;flex-wrap:wrap;margin-top:.35rem;font-size:.8rem}}
.archline .amuted{{color:#c7a86a}}
.archline .mc,.sugline .mc{{height:1em;vertical-align:-.15em}}
.iform{{margin:0}}
.ctoolbar{{display:flex;gap:.5rem;flex-wrap:wrap;margin:.4rem 0 .2rem}}
.sugline{{display:flex;gap:.4rem;align-items:center;flex-wrap:wrap;margin-bottom:.35rem;font-size:.8rem}}
.sugline .smuted{{color:#8fb37a}}
.chlist{{list-style:none;padding:0;margin:.4rem 0;display:flex;flex-direction:column;gap:.35rem}}
.chrow{{display:flex;align-items:center;gap:.6rem;background:#141d14;border:1px solid #2c3d2a;border-radius:9px;padding:.4rem .7rem}}
.chrow .chname{{font-weight:600;color:#e8f0e4}}
.chrm{{margin-left:auto;background:#7a2f28;color:#f3d9d4;border:0;border-radius:7px;width:1.7rem;height:1.7rem;font-weight:800;cursor:pointer;line-height:1}}
.chrm:hover{{filter:brightness(1.1)}}
.prow{{display:flex;gap:.3rem}}
.iupload{{display:flex;gap:.3rem;align-items:center}}
.plus{{width:64px;height:64px;border-radius:12px;border:1px dashed #3a4d38;
  background:#141d14;color:{MEADOW};font-size:1.7rem;cursor:pointer;align-self:center}}
.plus:hover{{background:#1a271a}}
.ashelf{{border-top:1px solid #22301f;padding-top:1rem;margin-top:1.2rem}}
.ashelf-head{{display:flex;gap:.6rem;flex-wrap:wrap;align-items:center}}
.ashelf-head .rn{{display:flex;gap:.3rem;flex:1 1 240px}}
.addbar{{display:flex;gap:.4rem;margin-top:1.6rem;max-width:26rem}}
.addbar input{{flex:1;padding:.5rem;border:1px solid #2c3d2a;border-radius:9px;
  background:#0e1510;color:#e8f0e4;font:inherit}}
/* ⚙ Settings */
.sgroup{{border-top:1px solid #22301f;padding-top:.9rem;margin-top:1.4rem}}
.sgroup h2{{font-size:1rem;margin:0 0 .8rem;color:#e8f0e4}}
.sfield{{display:flex;align-items:baseline;gap:.7rem;padding:.5rem 0;
  border-bottom:1px solid #1a241a;flex-wrap:wrap}}
.sfield label{{flex:0 0 13rem;font-weight:600;color:#e8f0e4}}
.sfield input[type=number]{{width:7rem;padding:.4rem .5rem;border:1px solid #2c3d2a;
  border-radius:8px;background:#0e1510;color:#e8f0e4;font:inherit}}
.sfield .unit{{color:#8fb37a;font-size:.85rem;min-width:3.2rem}}
.sfield .shelp{{flex:1 1 100%;margin:.2rem 0 0 13.7rem;font-size:.8rem;color:#9db095}}
.wtable{{width:100%;border-collapse:collapse;margin:.4rem 0 .8rem}}
.wtable th{{text-align:left;font-size:.78rem;color:#9db095;font-weight:600;
  padding:.3rem .5rem;border-bottom:1px solid #2c3d2a}}
.wtable td{{padding:.3rem .5rem;border-bottom:1px solid #1a241a;vertical-align:middle}}
.wtable input{{width:5.5rem;padding:.35rem .45rem;border:1px solid #2c3d2a;
  border-radius:8px;background:#0e1510;color:#e8f0e4;font:inherit}}
.wtable .pct{{font-weight:700;color:{MEADOW};white-space:nowrap}}
.wtable .bar{{display:block;height:.4rem;border-radius:999px;background:{MEADOW};
  min-width:2px;margin-top:.2rem;opacity:.75}}
.wtable tfoot td{{color:#9db095;font-size:.8rem;padding-top:.5rem}}
.lb{{list-style:none;margin:.5rem 0 0;padding:0}}
.lb li{{display:flex;align-items:center;gap:.6rem;padding:.55rem .35rem;
  border-bottom:1px solid #1d281c}}
.lb .rk{{width:1.9rem;text-align:center;font-weight:700;color:#9db095}}
.lb .nm{{flex:1;font-weight:600;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}}
.lb .amt{{font-weight:700;color:{MEADOW};font-variant-numeric:tabular-nums;
  text-align:right;min-width:5.5rem}}
.lb li.me{{background:#16211590;border-radius:10px}}
</style></head><body><header class="topbar"><span class="brand">🌼 Meadow Market</span>{logout}</header>
{content}{poller}</body></html>"#,
        content = if nav.is_empty() {
            format!(r#"<div class="content"><div class="{card_cls}">{body}</div></div>"#)
        } else {
            format!(
                r#"<div class="content"><div class="{card_cls} navcard">{nav}</div><div class="{card_cls}">{body}</div></div>"#
            )
        }
    )
}

// --- pagina's -----------------------------------------------------------

/// Heeft dit lid de Flowerborn-rol (voorlopig de geconfigureerde `role_id`)?
async fn is_flowerborn(st: &AppState, uid: &str) -> bool {
    st.dc
        .has_role(uid, &st.cfg.role_id)
        .await
        .ok()
        .flatten()
        .unwrap_or(false)
}

/// (uid, naam) van de ingelogde Flowerborn, of None (niet ingelogd / geen rol).
async fn require_flowerborn(st: &AppState, headers: &HeaderMap) -> Option<(String, String)> {
    let (uid, name) = cookie(headers, "session").and_then(|t| db::get_session(&st.pool, &t))?;
    if is_flowerborn(st, &uid).await {
        Some((uid, name))
    } else {
        None
    }
}

/// Navigatiebalk voor de ingelogde secties. `admin` toont de Beheer-tab.
fn nav_html(active: &str, admin: bool) -> String {
    let item = |href: &str, key: &str, label: &str| {
        let cls = if key == active { " class=\"active\"" } else { "" };
        format!("<a{cls} href=\"{href}\">{label}</a>")
    };
    let admin_link = if admin {
        item("/admin/market", "admin", "⚙ Manage")
    } else {
        String::new()
    };
    format!(
        "<nav class=\"nav\">{}{}{}{}</nav>",
        item("/", "home", "🎒 Inventory"),
        item("/market", "market", "🛒 Shop"),
        item("/leaderboard", "leaderboard", "🏆 Leaderboard"),
        admin_link,
    )
}

/// Sub-tabbalk binnen de Manage-sectie: Shop / Coins / Channels / Log / Server.
/// `active` = "market" | "coins" | "channels" | "log".
///
/// De Server-tab verlaat market: `/panel` is het Hytale-panel (aparte service,
/// door Caddy geproxyd naar 127.0.0.1:8090) met een eigen wachtwoord-login. Het
/// heeft dus geen `active`-toestand en linkt zelf terug naar `/admin/market`.
fn admin_subtabs(active: &str) -> String {
    let item = |href: &str, key: &str, label: &str| {
        let on = if key == active { " on" } else { "" };
        format!("<a class=\"subtab{on}\" href=\"{href}\">{label}</a>")
    };
    format!(
        "<div class=\"subtabs\">{}{}{}{}{}{}{}{}{}{}</div>",
        item("/admin/market", "market", "🛒 Shop"),
        item("/admin/inventory", "inv_preview", "🎒 Preview inventory"),
        item("/admin/shop/preview", "shop_preview", "👁 Admin shop preview"),
        item("/admin/accounts", "accounts", "👥 Accounts"),
        item("/admin/inactives", "inactives", "💤 Inactives"),
        item("/admin/coins", "coins", "🪙 Coins"),
        item("/admin/channels", "channels", "📋 Channels"),
        item("/admin/settings", "settings", "⚙ Settings"),
        item("/admin/log", "log", "📜 Log"),
        item("/panel", "server", "🖥 Server"),
    )
}

/// De nav-tabs voor ingelogde pagina's. De naam staat NIET meer in de navbar
/// (enkel groot+gecentreerd op de Inventory-pagina zelf). `extra` komt eventueel
/// vóór de nav (bv. een losse knop); `_name` blijft in de signatuur voor de
/// bestaande call-sites.
fn chrome(_name: &str, active: &str, admin: bool, extra: &str) -> String {
    format!("{}{}", extra, nav_html(active, admin))
}

// --- levelsysteem (op basis van coins ooit verdiend) --------------------
// Beginner = Level 0; na genoeg coins naar Level 1, enz. GEEN cap: elk volgend
// level kost 1.6× het vorige, dus levels lopen oneindig door (formule, geen tabel).
const LEVEL_BASE: f64 = 50.0; // coins nodig van level 0 → 1
const LEVEL_GROWTH: f64 = 1.6; // exponentiële groei per level

/// Coins (verdiend) nodig om van level `l` naar `l+1` te gaan (l vanaf 0).
fn level_cost(l: i64) -> i64 {
    (LEVEL_BASE * LEVEL_GROWTH.powi(l as i32)).round() as i64
}

/// (level ≥0, coins in dit level, coins nodig voor dit level). Oneindig veel
/// levels: het niveau wordt dynamisch berekend uit `earned`, geen bovengrens.
fn level_info(earned: i64) -> (i64, i64, i64) {
    let mut level = 0i64;
    let mut floor = 0i64;
    loop {
        let cost = level_cost(level);
        // Vangnet tegen f64/i64-overflow bij absurde waarden (praktisch onbereikbaar).
        if cost <= 0 || floor.checked_add(cost).is_none() {
            return (level, earned - floor, cost.max(1));
        }
        if earned < floor + cost {
            return (level, earned - floor, cost);
        }
        floor += cost;
        level += 1;
    }
}

/// Adminlijst (Discord-ID's) die de shop mogen beheren.
const ADMINS: [&str; 2] = ["391337551543271433", "233179495094419456"];

pub(crate) fn is_admin(uid: &str) -> bool {
    ADMINS.contains(&uid)
}

/// (uid, naam) uit de sessie, zonder rolcheck.
fn session_user(st: &AppState, headers: &HeaderMap) -> Option<(String, String)> {
    cookie(headers, "session").and_then(|t| db::get_session(&st.pool, &t))
}

/// (uid, naam) als de ingelogde gebruiker admin is, anders None.
fn require_admin(st: &AppState, headers: &HeaderMap) -> Option<(String, String)> {
    let (uid, name) = session_user(st, headers)?;
    is_admin(&uid).then_some((uid, name))
}

#[derive(Deserialize)]
struct HomeQuery {
    tab: Option<String>,
    msg: Option<String>,
}

async fn index(
    State(st): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<HomeQuery>,
) -> Html<String> {
    let session = cookie(&headers, "session").and_then(|t| db::get_session(&st.pool, &t));

    let (nav, body, wide) = match session {
        None => (String::new(), login_body(&st.cfg), false),
        Some((uid, name)) => {
            if is_flowerborn(&st, &uid).await {
                let (coins, _max, _pub, total_earned) = db::get_stats(&st.pool, &uid);
                let grants = db::active_grants(&st.pool, &uid, now_secs());
                let tab = q.tab.as_deref().unwrap_or("coins");
                let notice = match &q.msg {
                    Some(m) if !m.is_empty() => {
                        format!("<div class=\"notice ok\">{}</div>", esc(m))
                    }
                    _ => String::new(),
                };
                (
                    nav_html("home", is_admin(&uid)),
                    format!(
                        "{notice}{}",
                        inventory_home(
                            &st.pool,
                            &uid,
                            &name,
                            coins,
                            total_earned,
                            &grants,
                            tab,
                            is_admin(&uid)
                        )
                    ),
                    true,
                )
            } else {
                (String::new(), rules_body(&name), false)
            }
        }
    };
    Html(shell("Inventory — Meadow Market", &nav, wide, &body))
}

/// Aftel-teller (HTML + script) voor lopende tijdelijke rollen.
fn grants_html(grants: &[(String, f64)]) -> String {
    if grants.is_empty() {
        return String::new();
    }
    let rows: String = grants
        .iter()
        .map(|(label, exp)| {
            let lbl = if label.is_empty() {
                "Temporary access".to_string()
            } else {
                esc(label)
            };
            format!(
                "<div class=\"grant\" data-exp=\"{exp}\">\
                   <span class=\"glabel\">🎟 {lbl}</span>\
                   <span class=\"gtime\">…</span></div>"
            )
        })
        .collect();
    format!(
        "<div class=\"k\" style=\"margin:1.1rem 0 .4rem\">Active access</div>\
         <div class=\"grants\">{rows}</div>\
         <script>(function(){{function f(s){{s=Math.max(0,Math.floor(s));\
         var m=Math.floor(s/60),x=s%60;return m+':'+(x<10?'0':'')+x;}}\
         function t(){{var n=Date.now()/1000;document.querySelectorAll('.grant')\
         .forEach(function(g){{var e=parseFloat(g.dataset.exp),r=e-n,\
         q=g.querySelector('.gtime');if(r<=0){{q.textContent='expired';\
         g.classList.add('expired');}}else{{q.textContent=f(r);}}}});}}\
         t();setInterval(t,1000);}})();</script>"
    )
}

/// Eén gem-vakje op de bingokaart: greyed slot als niet ontgrendeld, anders
/// afbeelding + naam + uitleg + Use.
fn gem_slot(it: &db::Item, owned: bool, equipped: bool) -> String {
    if !owned {
        return "<div class=\"slot locked\"><div class=\"thumb\">\
                <span class=\"qmark\">?</span></div>\
                <div class=\"name muted\">???</div></div>"
            .to_string();
    }
    // De geëquipte gem krijgt géén dode "Equipped"-knop meer, maar Unequip: dat zet je
    // naamkleur terug op standaard en trekt de bijhorende Discord-rol in.
    let (label, extra, action) = if equipped {
        ("Unequip", " eq", "/use/gem/unequip")
    } else {
        ("Use", "", "/use/gem")
    };
    // Tweede, kleinere afbeelding onder de titel (zoals in de shop-kaart en booster_slot).
    let img2 = if it.image2.is_empty() {
        String::new()
    } else {
        format!(
            "<div class=\"thumb2\"><img src=\"/uploads/{}?v={}\" alt=\"\"></div>",
            esc(&it.image2),
            img_ver(&it.image2)
        )
    };
    format!(
        "<div class=\"slot gemcard previewable\" data-color=\"{col}\">\
         <div class=\"thumb\">{thumb}</div>\
         <div class=\"name\">{name}</div>{img2}<div class=\"gdesc\">{desc}</div>\
         <form method=\"post\" action=\"{action}\" class=\"buyform\">\
           <input type=\"hidden\" name=\"item_id\" value=\"{id}\">\
           <button class=\"buy on{extra}\" type=\"submit\">{label}</button></form></div>",
        col = esc(&it.color),
        thumb = thumb_html(&it.image, &it.color),
        name = esc(&it.name),
        desc = esc(&it.description),
        id = it.id,
    )
}

/// Eén booster-vakje (Lucky Horseshoe): vergrendeld "???" tot je het koopt, daarna
/// onthuld (afbeelding + naam + uitleg). Géén knop — bezit = permanent dubbele chest-kans.
fn booster_slot(it: &db::Item, owned: bool) -> String {
    if !owned {
        return "<div class=\"slot locked\"><div class=\"thumb\">\
                <span class=\"qmark\">?</span></div>\
                <div class=\"name muted\">???</div></div>"
            .to_string();
    }
    // Tweede, kleinere afbeelding onder de titel (zoals in de shop-kaart).
    let img2 = if it.image2.is_empty() {
        String::new()
    } else {
        format!(
            "<div class=\"thumb2\"><img src=\"/uploads/{}?v={}\" alt=\"\"></div>",
            esc(&it.image2),
            img_ver(&it.image2)
        )
    };
    format!(
        "<div class=\"slot gemcard\"><div class=\"thumb\">{thumb}</div>\
         <div class=\"name\">{name}</div>{img2}<div class=\"gdesc\">{desc}</div></div>",
        thumb = thumb_html(&it.image, &it.color),
        name = esc(&it.name),
        desc = esc(&it.description),
    )
}

/// Inventory-home met sub-tabs Coins / Gems / Boosts.
fn inventory_home(
    pool: &db::DbPool,
    uid: &str,
    name: &str,
    coins: i64,
    total_earned: i64,
    grants: &[(String, f64)],
    active: &str,
    admin: bool,
) -> String {
    let (lvl, n, m) = level_info(total_earned);
    let pct = if m > 0 { (n * 100 / m).clamp(0, 100) } else { 100 };
    let nm = if m > 0 {
        format!("{n}/{m}")
    } else {
        "MAX".to_string()
    };

    // Onderaan de Coins-tab: de ronde Hytale-knop met de resterende pas-geldigheid eróver.
    // Enkel bij een lópende dagpas. Geen pas, verlopen, óf permanente toegang → geen knop:
    // een afteller zonder einddatum (of zonder pas) zegt niets.
    // Eigen `data-passexp` i.p.v. de `.grant[data-exp]` van de Boosts-tab: die scripts
    // scannen het hele document, en alle tabs staan tegelijk in de HTML (enkel verborgen
    // via CSS), dus anders zouden twee timers op hetzelfde element vechten.
    let pass_btn = |inner: String| {
        format!(
            "<div class=\"passbtn\"><img src=\"/img/hytalepass.png\" alt=\"Hytale Day Pass\">\
               {inner}</div>"
        )
    };
    let pass_row = match db::get_whitelist(pool, uid, now_secs()) {
        Some((_n, Some(exp))) => format!(
            "{}\
             <script>(function(){{var e=document.querySelector('[data-passexp]');if(!e)return;\
               var exp=+e.dataset.passexp;function t(){{var s=Math.max(0,exp-Date.now()/1000);\
               var h=Math.floor(s/3600),m=Math.floor(s%3600/60),sec=Math.floor(s%60);\
               e.textContent=s>0?(h>0?h+'h '+m+'m':m+'m '+sec+'s'):'expired';\
               e.classList.toggle('out',s<=0);\
               if(s>0)setTimeout(t,1000);}}t();}})();</script>",
            pass_btn(format!("<span class=\"passtime\" data-passexp=\"{exp}\">…</span>"))
        ),
        // Permanente pas (of helemaal geen pas): niets te tellen → geen knop.
        Some((_, None)) | None => String::new(),
    };
    let coins_panel = format!(
        "<div class=\"earned\">{MC} <span data-bal>{coins}</span></div>\
         <p class=\"muted\" style=\"text-align:center;margin:.15rem 0 0\">current balance</p>\
         <div class=\"levelrow\"><span class=\"lvlbadge\" data-lvl>Lv {lvl}</span>\
           <div class=\"bar\"><div class=\"fill\" data-fill style=\"width:{pct}%\"></div></div>\
           <span class=\"lvlnm\" data-lvlnm>{nm}</span></div>\
         <div class=\"statrow\"><span class=\"k\">Coins earned all-time</span>\
           <span>{MC} <b data-earned>{total_earned}</b></span></div>{grants}{pass_row}",
        grants = grants_html(grants),
    );

    // Gems — bingokaart: alle gems, ontgrendelde onthuld.
    let owned: std::collections::HashSet<i64> =
        db::owned_item_ids(pool, uid).into_iter().collect();
    let name_color = db::get_name_color(pool, uid);
    let shown_color = if name_color.is_empty() {
        "#e8f0e4".to_string()
    } else {
        esc(&name_color)
    };
    // Discord-profielkleur als achtergrond-swatch (preview in Discord-look).
    let discord = db::get_discord_color(pool, uid);
    let discord_swatch = if discord.is_empty() {
        String::new()
    } else {
        format!(
            "<div class=\"swatch\" style=\"background:{bg};color:{c}\" \
               title=\"Your Discord profile color\">{nm}</div>",
            bg = esc(&discord),
            c = shown_color,
            nm = esc(name),
        )
    };
    // Verzamelkaart: elk (niet-pas) shop-item krijgt een kaart — grijs met "?" tot je het
    // koopt, daarna onthuld (afbeelding + naam + uitleg). ALLE gems staan samen in één set
    // ("Basic Gems"), zo compact mogelijk: `.shelf wrap` vult de rij en wrapt pas als hij vol
    // is — geen vaste rij-per-schap meer. Volgorde = schap-volgorde (primary → secondary → …).
    let slots: String = db::list_shelves(pool)
        .iter()
        .flat_map(|(sid, _)| db::shelf_items(pool, *sid))
        .filter(|it| it.category == "inventory")
        .map(|it| {
            let own = owned.contains(&it.id);
            let eq = own && !it.color.is_empty() && it.color.eq_ignore_ascii_case(&name_color);
            gem_slot(&it, own, eq)
        })
        .collect();
    let collection = if slots.is_empty() {
        String::new()
    } else {
        format!("<h2 class=\"shelf-title center fancy\">Basic Gems</h2><div class=\"shelf wrap gems6\">{slots}</div>")
    };
    // Admin-testhulp: verzamel-aankopen terugdraaien (coins terug). (Sync gem colors staat
    // op de Manage-pagina.)
    let admin_reset = if admin {
        "<form method=\"post\" action=\"/admin/reset-collection\" style=\"margin:.2rem 0 .8rem\">\
           <button class=\"btn small ghost\" type=\"submit\">🧪 Reset all test purchases</button></form>"
    } else {
        ""
    };
    let gems_panel = format!(
        "{admin_reset}<div class=\"nameshow\">{ds}\
           <div class=\"swatch dark\" style=\"color:{c}\">{nm2}</div>\
           <div class=\"swatch light\" style=\"color:{c}\">{nm2}</div></div>\
         <p class=\"muted preview-hint\">Click a gem below to preview your name in its color.</p>\
         {collection}{GEM_PREVIEW_JS}",
        ds = discord_swatch,
        c = shown_color,
        nm2 = esc(name),
    );

    // Boosts — Hytale-whitelist: enkel de status + de booster/pass-verzamelvakjes.
    // Kopen/verlengen gebeurt in de Shop en whitelistet meteen — geen Use meer.
    let boosts_panel = {

        // Whitelist-status: permanent, lopende afteller, of niets.
        let status = match db::get_whitelist(pool, uid, now_secs()) {
            Some((n, None)) => format!(
                "<div class=\"notice ok\" style=\"margin:.2rem 0 1rem\">\
                   🔑 Permanent Hytale access — whitelisted as <b>{}</b>.</div>",
                esc(&n)
            ),
            Some((n, Some(exp))) => format!(
                "<div class=\"grant\" data-exp=\"{exp}\" style=\"margin:.2rem 0 1rem\">\
                   <span class=\"glabel\">🎟 Whitelisted as {}</span>\
                   <span class=\"gtime\">…</span></div>\
                 <script>(function(){{document.querySelectorAll('.grant[data-exp]').forEach(function(g){{\
                   var exp=+g.dataset.exp;function t(){{var s=Math.max(0,exp-Date.now()/1000);\
                   var h=Math.floor(s/3600),m=Math.floor(s%3600/60),sec=Math.floor(s%60);\
                   g.querySelector('.gtime').textContent=h+'h '+m+'m '+sec+'s';\
                   if(s>0)setTimeout(t,1000);}}t();}});}})();</script>",
                esc(&n)
            ),
            None => String::new(),
        };

        // Geen naam- of "no pass"-tekst meer: spelers kennen hun eigen naam wel, en de
        // whitelist-status hierboven zegt al of ze toegang hebben.

        // Boosters (Lucky Horseshoe): permanent verzamel-item, getoond als grey-out-slot
        // zoals de gems — vergrendeld "???" tot je het koopt, daarna onthuld. Géén Use:
        // bezit = altijd dubbele kans bij de treasure chest (Fortuna's Favor).
        let booster_owned: std::collections::HashSet<i64> =
            db::owned_item_ids(pool, uid).into_iter().collect();
        let boosters = db::all_booster_items(pool);
        let mut cards: String = boosters
            .iter()
            .map(|it| booster_slot(it, booster_owned.contains(&it.id)))
            .collect();
        // Permanente pas als greyed-out verzamelvakje — hoort visueel het best hier. LET OP:
        // hij is category 'boost', NIET 'booster', dus hij zit NOOIT in de rnd-korf en blijft
        // altijd gewoon in de shop te koop. Onthuld zodra je permanente toegang bezit.
        if let Some(perma) = db::boost_items(pool).into_iter().find(|it| it.duration == 0) {
            cards.push_str(&booster_slot(&perma, db::has_perma_access(pool, uid)));
        }
        let shelf = if cards.is_empty() {
            String::new()
        } else {
            format!("<h2 class=\"shelf-title center fancy\">Trinkets</h2><div class=\"shelf\">{cards}</div>")
        };
        format!("{status}{shelf}")
    };

    let cls = |t: &str| if t == active { " on" } else { "" };
    format!(
        "<div class=\"bigname\">{uname}</div>\
         <div class=\"subtabs center\">\
           <button class=\"subtab{ca}\" data-t=\"coins\">{MC} Coins</button>\
           <button class=\"subtab{cg}\" data-t=\"gems\">💎 Gems</button>\
           <button class=\"subtab{cb}\" data-t=\"boosts\">🍀 Trinkets</button></div>\
         <div class=\"panel{ca}\" id=\"p-coins\">{coins_panel}</div>\
         <div class=\"panel{cg}\" id=\"p-gems\">{gems_panel}</div>\
         <div class=\"panel{cb}\" id=\"p-boosts\">{boosts_panel}</div>\
         <script>(function(){{var ts=document.querySelectorAll('.subtab');\
           ts.forEach(function(b){{b.addEventListener('click',function(){{\
             ts.forEach(function(x){{x.classList.remove('on');}});b.classList.add('on');\
             document.querySelectorAll('.panel').forEach(function(p){{p.classList.remove('on');}});\
             document.getElementById('p-'+b.dataset.t).classList.add('on');}});}});}})();</script>{KEEP_SCROLL_JS}",
        uname = esc(name),
        ca = cls("coins"),
        cg = cls("gems"),
        cb = cls("boosts"),
    )
}

/// Shop: 4 dagelijkse random items (24u-rotatie) + de vaste Hytale-tickets.
async fn market(
    State(st): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<MarketQuery>,
) -> Response {
    let refresh = auto_refresh_js(AUTO_REFRESH_SHOP_MS);
    let Some((uid, name)) = require_flowerborn(&st, &headers).await else {
        return Redirect::to("/").into_response();
    };
    let admin = is_admin(&uid);
    let (coins, _m, _p, _te) = db::get_stats(&st.pool, &uid);
    let notice = market_notice(&q);
    let owned: std::collections::HashSet<i64> =
        db::owned_item_ids(&st.pool, &uid).into_iter().collect();

    let has_name = !db::get_hytale_name(&st.pool, &uid).is_empty();
    let has_perma = db::has_perma_access(&st.pool, &uid);

    // Lopende pas? Dan toont de dagpas als "Bought" tot de timer afloopt (één tegelijk).
    let has_pass = db::get_whitelist(&st.pool, &uid, now_secs()).is_some();
    let slot = |it: &db::Item| shop_slot(it, owned.contains(&it.id), has_name, has_perma, has_pass, coins);

    // Volledig shop-ontwerp (zoals de Admin shop preview): dagrotatie + Hytale-passen.
    // Twee onderdelen zijn nog niet vrijgegeven en staan grijs (zie de flags bovenaan).
    // Afteller tot de volgende rotatie = eerstvolgende UTC-middernacht (epoch-seconden);
    // de client rekent met Date.now() → tijdzone-onafhankelijk, tikt elke seconde.
    let next_refresh = (shop_day() + 1) * 86400;
    let countdown =
        format!("<span class=\"shop-countdown\" data-refresh=\"{next_refresh}\">⏳ …</span>");
    let picks = if SHOP_DAILY_PICKS_LIVE {
        let offers: String = db::shop_offers(
            &st.pool,
            shop_day(),
            SHOP_DAILY_N,
            settings::i64_of(&st.pool, "horseshoe_shop_odds_days"),
        )
        .iter()
        .map(slot)
        .collect();
        // GEEN reroll-knop op de publieke shop (de admin-reroll leeft enkel op de
        // Admin shop preview). De publieke shop toont enkel de afteller.
        format!(
            "<h2 class=\"shelf-title\">✨ Today's picks{countdown}</h2>\
             <div class=\"shelf shop picks\">{offers}</div>"
        )
    } else {
        // Nog niet vrijgegeven: SHOP_DAILY_N grijze 🔒-placeholders i.p.v. echte gems.
        let ph: String = (0..SHOP_DAILY_N).map(|_| placeholder_slot()).collect();
        format!(
            "<h2 class=\"shelf-title\">✨ Today's picks{countdown}</h2>\
             <div class=\"shelf shop picks\">{ph}</div>"
        )
    };
    // Hytale-passen: dagpas blijft koopbaar; de permanente pas staat grijs als textloze
    // teaser (zelfde 🔒-placeholder als de picks — géén naam/prijs) tot vrijgegeven.
    let passes: String = db::boost_items(&st.pool)
        .iter()
        .map(|it| {
            if it.duration == 0 && !SHOP_PERMA_PASS_LIVE {
                placeholder_slot()
            } else {
                slot(it)
            }
        })
        .collect();
    let shelves = format!(
        "{picks}\
         <h2 class=\"shelf-title\">🎟 Hytale access</h2>\
         <div class=\"shelf shop\">{passes}</div>"
    );

    let from = q.from.unwrap_or(coins);

    let body = format!(
        "<div class=\"purse-box\" data-from=\"{from}\">Purse {MC} \
           <span class=\"purse-n\" data-bal>{coins}</span></div>\
         <h1 class=\"shoptitle\">🛒 Shop</h1>{notice}{shelves}\
         <script>(function(){{\
           try{{var u=new URL(location.href);if(u.searchParams.has('from')){{\
             u.searchParams.delete('from');\
             history.replaceState({{}},'',u.pathname+u.search+u.hash);}}}}catch(e){{}}\
           var p=document.querySelector('.purse-box');if(!p)return;\
           var el=p.querySelector('.purse-n'),to=+el.textContent,from=+p.dataset.from;\
           if(from===to||isNaN(from))return;var s=performance.now(),d=800;\
           function step(t){{var k=Math.min(1,(t-s)/d);\
             el.textContent=Math.round(from+(to-from)*k);\
             if(k<1)requestAnimationFrame(step);}}requestAnimationFrame(step);}})();</script>\
         <script>(function(){{\
           var el=document.querySelector('.shop-countdown');if(!el)return;\
           var target=+el.dataset.refresh*1000;\
           function p(n){{return(n<10?'0':'')+n;}}\
           function tick(){{\
             var s=Math.max(0,Math.round((target-Date.now())/1000));\
             if(s<=0){{el.innerHTML='⏳ Refreshing…';return;}}\
             var h=Math.floor(s/3600),m=Math.floor(s%3600/60),ss=s%60;\
             el.innerHTML='⏳ New picks in <b>'+h+':'+p(m)+':'+p(ss)+'</b>';\
             setTimeout(tick,1000);}}\
           tick();}})();</script>\
         {KEEP_SCROLL_JS}{refresh}",
    );
    Html(shell("Shop — Meadow Market", &chrome(&name, "market", admin, ""), true, &body))
        .into_response()
}

/// **Admin shop preview**: het beoogde publieke shop-ontwerp — de dagrotatie (`SHOP_DAILY_N`
/// willekeurige items, voor iedereen dezelfde, stabiel tot middernacht UTC) met de passen los
/// eronder, precies zoals de shop eruitzag vóór hij voor de Hytale-test werd verborgen. Nu enkel
/// hier, op een admin-pagina, ter goedkeuring — onafhankelijk van `SHOP_TEST_DAY_PASS_ONLY`.
async fn admin_shop_preview(
    State(st): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<MarketQuery>,
) -> Response {
    let refresh = auto_refresh_js(AUTO_REFRESH_SHOP_MS);
    let Some((uid, name)) = require_admin(&st, &headers) else {
        return Redirect::to("/").into_response();
    };
    let (coins, _m, _p, _te) = db::get_stats(&st.pool, &uid);
    let notice = market_notice(&q);
    let owned: std::collections::HashSet<i64> =
        db::owned_item_ids(&st.pool, &uid).into_iter().collect();
    let has_name = !db::get_hytale_name(&st.pool, &uid).is_empty();
    let has_perma = db::has_perma_access(&st.pool, &uid);
    let has_pass = db::get_whitelist(&st.pool, &uid, now_secs()).is_some();
    let slot = |it: &db::Item| shop_slot(it, owned.contains(&it.id), has_name, has_perma, has_pass, coins);

    let offers: String =
        db::shop_offers(
            &st.pool,
            shop_day(),
            SHOP_DAILY_N,
            settings::i64_of(&st.pool, "horseshoe_shop_odds_days"),
        )
        .iter()
        .map(slot)
        .collect();
    // Reroll keert terug naar deze preview (niet naar /market zoals de publieke knop).
    let reroll = "<form method=\"post\" action=\"/admin/shop/reroll?next=/admin/shop/preview\" \
                   class=\"reroll-f\">\
                   <button class=\"reroll\" title=\"Roll a new daily selection (admin)\">↻</button></form>";
    let passes: String = db::boost_items(&st.pool).iter().map(slot).collect();
    let shelves = format!(
        "<h2 class=\"shelf-title\">✨ Today's picks{reroll}</h2>\
         <div class=\"shelf shop\">{offers}</div>\
         <h2 class=\"shelf-title\">🎟 Hytale access</h2>\
         <div class=\"shelf shop\">{passes}</div>"
    );

    let from = q.from.unwrap_or(coins);
    let body = format!(
        "{subtabs}\
         <div class=\"purse-box\" data-from=\"{from}\">Purse {MC} \
           <span class=\"purse-n\" data-bal>{coins}</span></div>\
         <h1 class=\"shoptitle\">🛒 Shop preview</h1>\
         {notice}{shelves}{KEEP_SCROLL_JS}{refresh}",
        subtabs = admin_subtabs("shop_preview"),
    );
    Html(shell("Shop preview — Meadow Market", &chrome(&name, "admin", true, ""), true, &body))
        .into_response()
}

/// Admin-preview van de VOLLEDIGE inventory: alle verzamel-items (gems + boosters) getoond
/// als owned/ontgrendeld, zodat je kunt inschatten hoe een volle inventory eruitziet. Enkel
/// admin. Puur visueel — de knoppen (Use/Unequip) blijven functioneel zoals bij een lid.
async fn admin_inventory_preview(State(st): State<AppState>, headers: HeaderMap) -> Response {
    let Some((_uid, name)) = require_admin(&st, &headers) else {
        return Redirect::to("/").into_response();
    };
    let all_items: Vec<db::Item> = db::list_shelves(&st.pool)
        .iter()
        .flat_map(|(sid, _)| db::shelf_items(&st.pool, *sid))
        .collect();
    // Alle gems als owned → afbeelding + naam + volledige omschrijving.
    let gems: String = all_items
        .iter()
        .filter(|it| it.category == "inventory")
        .map(|it| gem_slot(it, true, false))
        .collect();
    let gems_set = if gems.is_empty() {
        String::new()
    } else {
        format!("<h2 class=\"shelf-title center fancy\">Basic Gems</h2><div class=\"shelf wrap gems6\">{gems}</div>")
    };
    // Alle boosters als owned.
    let boosters: String = all_items
        .iter()
        .filter(|it| it.category == "booster")
        .map(|it| booster_slot(it, true))
        .collect();
    let boost_set = if boosters.is_empty() {
        String::new()
    } else {
        format!("<h2 class=\"shelf-title center fancy\">Trinkets</h2><div class=\"shelf wrap gems6\">{boosters}</div>")
    };
    let body = format!(
        "{subtabs}<h1 class=\"shoptitle\">🎒 Preview inventory</h1>\
         <p class=\"muted\">Every collectible shown as owned/unlocked — to gauge how a full inventory looks.</p>\
         {gems_set}{boost_set}{KEEP_SCROLL_JS}",
        subtabs = admin_subtabs("inv_preview"),
    );
    Html(shell("Inventory preview — Meadow Market", &chrome(&name, "admin", true, ""), true, &body))
        .into_response()
}

/// Admin-knopje naast de dagitems: gooi de selectie van vandaag weg en trek opnieuw.
async fn admin_shop_reroll(
    State(st): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<RerollQuery>,
) -> Response {
    if let Some((admin_uid, admin)) = require_admin(&st, &headers) {
        db::clear_shop_day(&st.pool, shop_day());
        db::log_event(
            &st.pool,
            now_secs(),
            &db::LogEntry::new("admin", "shop_reroll")
                .actor(&admin_uid, &admin)
                .detail(format!("nieuwe dagselectie getrokken · by {admin}")),
        );
    }
    // Terug naar de pagina die de reroll aanvroeg (preview blijft op preview); de publieke
    // shop-knop stuurt geen `next` mee → default `/market` (ongewijzigd gedrag).
    let dest = match q.next.as_deref() {
        Some(p) if p.starts_with('/') && !p.starts_with("//") => p.to_string(),
        _ => "/market".to_string(),
    };
    Redirect::to(&dest).into_response()
}

#[derive(Deserialize)]
struct RerollQuery {
    next: Option<String>,
}

/// Cache-buster op basis van de bestand-mtime: een vervangen afbeelding krijgt zo een nieuwe
/// URL, terwijl de browser ongewijzigde afbeeldingen uit z'n cache haalt (geen herlaad-flits).
fn img_ver(image: &str) -> u64 {
    std::fs::metadata(format!("{UPLOAD_DIR}/{image}"))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Thumbnail uit (afbeelding, kleur): geüploade afbeelding, anders een gem-bol.
fn thumb_html(image: &str, color: &str) -> String {
    if !image.is_empty() {
        format!("<img src=\"/uploads/{}?v={}\" alt=\"\">", esc(image), img_ver(image))
    } else if !color.is_empty() {
        format!(
            "<span class=\"gem\" style=\"background:radial-gradient(circle at 35% 30%,#ffffffcc,{})\"></span>",
            esc(color)
        )
    } else {
        "<span class=\"gem gem-empty\"></span>".to_string()
    }
}

/// Getal met een punt als duizendtal-scheidingsteken: 1000 → "1.000", 20000 → "20.000".
/// Puur cosmetisch, voor bedragen die spelers zien (bv. shop-prijzen).
fn dots(n: i64) -> String {
    let neg = n < 0;
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push('.');
        }
        out.push(c);
    }
    if neg {
        format!("-{out}")
    } else {
        out
    }
}

fn item_thumb(it: &db::Item) -> String {
    // Een zelf geüploade afbeelding wint altijd. De dagpas kreeg vroeger onvoorwaardelijk
    // het ingebakken ticket-icoon, waardoor een upload wél opsloeg maar nooit te zien was
    // ("de upload doet niks"). Het ticket is nu enkel de terugval zolang er niets geüpload is.
    if it.image.is_empty() && it.category == "boost" && it.duration > 0 {
        return "<img src=\"/img/ticket.png\" alt=\"24h Hytale pass\">".to_string();
    }
    thumb_html(&it.image, &it.color)
}

/// Eén winkelvakje: thumb, naam, prijs, effect-badge en Buy (of Owned voor
/// reeds verzamelde gems).
fn shop_slot(it: &db::Item, owned: bool, has_name: bool, has_perma: bool, has_pass: bool, coins: i64) -> String {
    // Lopende dagpas (boost mét looptijd + een actieve pas): zolang de timer loopt koop je
    // geen tweede. Toon de kaart dan als "Bought" (grijs + ✓ + Bought-knop), niet als
    // "Out of Stock" — er ís voorraad, jíj hebt er gewoon al een lopen.
    let day_pass_active = it.category == "boost" && it.duration > 0 && has_pass;
    // Reeds gekocht → kaart grijs + groene ✓, geen Buy-knop. Geldt voor bezeten
    // verzamel-items, de permanente pas (bij permanente toegang) én de dagpas zolang die loopt.
    let bought = (owned && (it.category == "inventory" || it.category == "booster"))
        || (it.category == "boost" && it.duration == 0 && has_perma)
        || day_pass_active;
    // Dicht voor iedereen, om twee redenen:
    //  * handmatig op Out of stock gezet (sold_out);
    //  * voorraad op 0 → dicht tot een admin aanvult.
    // Item blijft wél staan: je ziet wát er te koop is. De échte rem zit in buy()/purchase() —
    // een grijze knop houdt niemand tegen die zelf een POST stuurt.
    let dicht = it.sold_out || it.stock == 0;
    // Te weinig coins → Buy-knop grijs/uitgeschakeld (dan hoeft de "not enough coins"-banner
    // niet meer te verschijnen in de normale flow). buy()/purchase() blijft server-side de
    // rem als vangnet (race of handmatige POST).
    let cant_afford = coins < it.price;
    let action = if day_pass_active {
        // Eigen "Bought"-knop (grijs, niet klikbaar) i.p.v. de lege owned-actie, zodat
        // duidelijk is dat je pas loopt.
        "<button class=\"buy owned\" type=\"button\" disabled>Bought</button>".to_string()
    } else if dicht && !bought {
        "<button class=\"buy\" type=\"button\" disabled>Out of Stock</button>".to_string()
    } else if bought {
        String::new()
    } else if cant_afford {
        "<button class=\"buy\" type=\"button\" disabled>Buy</button>".to_string()
    } else {
        // Eerste pas-aankoop: vraag de Hytale-naam mee in het koopformulier.
        // Zodra die bewaard is (has_name) verdwijnt het veld voorgoed.
        let name_field = if it.category == "boost" && !has_name {
            "<input name=\"hytale_name\" maxlength=\"32\" required \
               pattern=\"[A-Za-z0-9_]{1,32}\" placeholder=\"your Hytale name\" \
               style=\"width:100%;padding:.35rem;margin-bottom:.4rem;border:1px solid #2c3d2a;\
                 border-radius:7px;background:#0e1510;color:#e8f0e4;font:inherit;font-size:.8rem\">"
        } else {
            ""
        };
        format!(
            "<form method=\"post\" action=\"/buy\" class=\"buyform\">\
               <input type=\"hidden\" name=\"item_id\" value=\"{id}\">{name_field}\
               <button class=\"buy on\" type=\"submit\">Buy</button></form>",
            id = it.id,
        )
    };
    let slotcls = if bought { " bought" } else { "" };
    let mark = if bought {
        "<div class=\"boughtmark\" title=\"Owned\">✓</div>"
    } else {
        ""
    };
    // Plain items (geen gem/pass) mogen een tweede, kleinere afbeelding onder de
    // titel dragen.
    let img2 = if it.image2.is_empty() {
        String::new()
    } else {
        format!(
            "<div class=\"thumb2\"><img src=\"/uploads/{}?v={}\" alt=\"\"></div>",
            esc(&it.image2),
            img_ver(&it.image2)
        )
    };
    // Omschrijving (uit Manage) tonen onder de titel/2e afbeelding.
    let desc = if it.description.is_empty() {
        String::new()
    } else {
        format!("<div class=\"sdesc\">{}</div>", esc(&it.description))
    };
    // Voorraad tonen zodra ze geteld wordt (-1 = onbeperkt → niets tonen): eerlijk naar de
    // speler, die ziet meteen of het de moeite is om te wachten. Uitzondering: zette een
    // admin het item handmatig op "Out of stock" (`sold_out`), dan verbergen we het
    // resterende aantal — de knop zegt dan al Out of Stock en "1 left" ernaast zou
    // tegenstrijdig zijn.
    let stock = if it.sold_out || it.stock < 0 {
        String::new()
    } else if it.stock == 0 {
        "<div class=\"stock none\">out of stock</div>".to_string()
    } else {
        format!("<div class=\"stock\">{} left</div>", it.stock)
    };
    format!(
        "<div class=\"slot{slotcls}\">{mark}<div class=\"thumb\">{thumb}</div>\
         <div class=\"name\">{name}</div>{img2}{desc}\
         <div class=\"price\">{MC} {price}</div>{stock}{action}</div>",
        thumb = item_thumb(it),
        name = esc(&it.name),
        price = dots(it.price),
    )
}

/// Grijs, textloos 🔒-placeholder-vakje als teaser voor iets dat nog niet vrijgegeven is:
/// de dagpicks (`SHOP_DAILY_PICKS_LIVE = false`) én de permanente pas (`SHOP_PERMA_PASS_LIVE
/// = false`). Bewust géén naam/prijs/tekst — enkel het slotje — zodat er niks speler-zichtbaars
/// verzonnen wordt; de kaart houdt wel de vakjes-hoogte aan.
fn placeholder_slot() -> String {
    "<div class=\"slot soon\"><div class=\"thumb\">🔒</div></div>".to_string()
}

/// Inventory-sectie — placeholder tot items bestaan (gekocht/gewonnen).
/// De Inventory is nu de home (`/`); oude link doorsturen.
async fn inventory() -> Response {
    Redirect::to("/").into_response()
}

/// Eén ranglijst renderen: medailles per rang (👑 / 🥈 / 🥉), eigen rij gemarkeerd.
fn lb_list(rows: &[(String, String, i64)], me: &str) -> String {
    if rows.is_empty() {
        return "<p class=\"muted\">No one on the board yet.</p>".to_string();
    }
    let items: String = rows
        .iter()
        .enumerate()
        .map(|(i, (uid, uname, val))| {
            let rk = match i {
                0 => "👑".to_string(),
                1 => "🥈".to_string(),
                2 => "🥉".to_string(),
                n => format!("{}", n + 1),
            };
            let me_cls = if uid == me { " class=\"me\"" } else { "" };
            format!(
                "<li{me_cls}><span class=\"rk\">{rk}</span>\
                 <span class=\"nm\">{name}</span>\
                 <span class=\"amt\">{MC} {val}</span></li>",
                name = esc(uname),
            )
        })
        .collect();
    format!("<ol class=\"lb\">{items}</ol>")
}

/// Leaderboard met tabs All-time (ooit verdiend) en Now (huidig saldo).
async fn leaderboard_page(State(st): State<AppState>, headers: HeaderMap) -> Response {
    let Some((me, name)) = require_flowerborn(&st, &headers).await else {
        return Redirect::to("/").into_response();
    };
    let all_list = lb_list(&db::leaderboard_alltime(&st.pool, 50), &me);
    let now_list = lb_list(&db::leaderboard_now(&st.pool, 50), &me);
    let week_since = db::last_saturday_1500_brussels(now_secs());
    let week_list = lb_list(&db::leaderboard_week(&st.pool, week_since, 50), &me);

    let body = format!(
        "<h1>🏆 Leaderboard</h1>\
         <div class=\"subtabs\">\
           <button class=\"subtab on\" data-t=\"alltime\">All-time</button>\
           <button class=\"subtab\" data-t=\"week\">This week</button>\
           <button class=\"subtab\" data-t=\"now\">Now</button></div>\
         <div class=\"panel on\" id=\"p-alltime\">{all_list}</div>\
         <div class=\"panel\" id=\"p-week\">{week_list}</div>\
         <div class=\"panel\" id=\"p-now\">{now_list}</div>\
         <script>(function(){{var ts=document.querySelectorAll('.subtab');\
           ts.forEach(function(b){{b.addEventListener('click',function(){{\
             ts.forEach(function(x){{x.classList.remove('on');}});b.classList.add('on');\
             document.querySelectorAll('.panel').forEach(function(p){{p.classList.remove('on');}});\
             document.getElementById('p-'+b.dataset.t).classList.add('on');}});}});}})();</script>"
    );
    Html(shell("Leaderboard — Meadow Market", &chrome(&name, "leaderboard", is_admin(&me), ""), true, &body))
        .into_response()
}

#[derive(Deserialize)]
struct PublicForm {
    // Checkbox: aanwezig (=Some) betekent aangevinkt/publiek; afwezig = privé.
    public: Option<String>,
}

/// public-toggle vanaf de Coins-pagina (checkbox auto-submit → terug naar `/`).
async fn set_public_route(
    State(st): State<AppState>,
    headers: HeaderMap,
    Form(f): Form<PublicForm>,
) -> Response {
    if let Some((uid, name)) = require_flowerborn(&st, &headers).await {
        db::set_public(&st.pool, &uid, &name, f.public.is_some());
    }
    Redirect::to("/").into_response()
}

fn login_body(cfg: &Config) -> String {
    if !cfg.oauth_ready() {
        return "<h1>🌼 Meadow Market</h1><p class=\"muted\">OAuth is not configured \
            yet (client_id/secret missing in secrets.json).</p>"
            .to_string();
    }
    "<h1>🌼 Welcome to Meadow Market</h1>\
     <a class=\"btn\" href=\"/login\">Log in with Discord</a>"
        .to_string()
}

fn rules_body(name: &str) -> String {
    format!(
        "<h1>🌼 Hi, {name}</h1>\
         <p>You're logged in, but you don't have the <b>Flowerborn</b> role (yet). \
         A Meadow Market account is only for Flowerborns.</p>\
         <a class=\"link\" href=\"/logout\">Log out</a>",
        name = esc(name)
    )
}

fn err_page(msg: &str) -> Response {
    let body = format!(
        "<h1>🌼 Something went wrong</h1><p>{}</p><a class=\"link\" href=\"/\">Back</a>",
        esc(msg)
    );
    (StatusCode::BAD_REQUEST, Html(shell("Meadow Market", "", false, &body))).into_response()
}

// --- OAuth2-flow --------------------------------------------------------

#[derive(Deserialize)]
struct LoginQuery {
    /// Lokaal pad waar we na een geslaagde login naartoe willen (bv. `/market`).
    /// Enkel interne paden toegestaan — zie `safe_next`.
    next: Option<String>,
}

/// Sta enkel eigen-site-paden toe als post-login-bestemming (open-redirect-guard):
/// moet met één `/` beginnen en niet met `//` (dat is een protocol-relatieve URL).
fn safe_next(next: Option<&str>) -> String {
    match next {
        Some(p) if p.starts_with('/') && !p.starts_with("//") => p.to_string(),
        _ => "/".to_string(),
    }
}

async fn login(
    State(st): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<LoginQuery>,
) -> Response {
    if !st.cfg.oauth_ready() {
        return err_page("OAuth is not configured on the server yet.");
    }
    // Al een geldige sessie? Dan niet opnieuw langs Discord — meteen naar de bestemming.
    // De embed-knop wijst naar `/login?next=/market`, dus zónder deze check stuurt élke
    // klik je door de hele OAuth-roundtrip, ook al ben je al ingelogd (en dat voelt als
    // "ik moet steeds opnieuw inloggen", ook al is de cookie 90 dagen geldig).
    if session_user(&st, &headers).is_some() {
        return Redirect::to(&safe_next(q.next.as_deref())).into_response();
    }
    let state = rand_token();
    let redirect = st.cfg.oauth_redirect();
    let url = format!(
        "https://discord.com/oauth2/authorize?response_type=code\
         &client_id={}&redirect_uri={}&scope=identify&state={}",
        pct(&st.cfg.client_id),
        pct(&redirect),
        pct(&state),
    );
    // Bewaar zowel de CSRF-state als de gewenste eindbestemming over de OAuth-roundtrip.
    let next = safe_next(q.next.as_deref());
    let state_cookie = format!("oauth_state={state}; HttpOnly; SameSite=Lax; Path=/; Max-Age=600");
    let next_cookie = format!("oauth_next={next}; HttpOnly; SameSite=Lax; Path=/; Max-Age=600");
    (set_cookies(&[&state_cookie, &next_cookie]), Redirect::to(&url)).into_response()
}

#[derive(Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
}

async fn callback(
    State(st): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<CallbackQuery>,
) -> Response {
    let (Some(code), Some(state)) = (q.code, q.state) else {
        return err_page("Login was cancelled or denied.");
    };
    // CSRF: de state moet overeenkomen met de cookie die we bij /login zetten.
    match cookie(&headers, "oauth_state") {
        Some(c) if c == state => {}
        _ => return err_page("Invalid or expired login state. Please try again."),
    }

    let redirect = st.cfg.oauth_redirect();
    let form = [
        ("client_id", st.cfg.client_id.as_str()),
        ("client_secret", st.cfg.client_secret.as_str()),
        ("grant_type", "authorization_code"),
        ("code", code.as_str()),
        ("redirect_uri", redirect.as_str()),
    ];
    let token: Value = match st
        .http
        .post("https://discord.com/api/v10/oauth2/token")
        .form(&form)
        .send()
        .await
        .and_then(|r| r.error_for_status())
    {
        Ok(r) => match r.json().await {
            Ok(v) => v,
            Err(e) => return err_page(&format!("Token response unreadable: {e}")),
        },
        Err(e) => return err_page(&format!("Token exchange failed: {e}")),
    };
    let access = token["access_token"].as_str().unwrap_or_default();
    if access.is_empty() {
        return err_page("No access_token received from Discord.");
    }

    let me: Value = match st
        .http
        .get("https://discord.com/api/v10/users/@me")
        .header("Authorization", format!("Bearer {access}"))
        .send()
        .await
        .and_then(|r| r.error_for_status())
    {
        Ok(r) => match r.json().await {
            Ok(v) => v,
            Err(e) => return err_page(&format!("Profile response unreadable: {e}")),
        },
        Err(e) => return err_page(&format!("Failed to fetch profile: {e}")),
    };
    let uid = me["id"].as_str().unwrap_or_default().to_string();
    if uid.is_empty() {
        return err_page("No Discord user ID received.");
    }
    let name = me["global_name"]
        .as_str()
        .filter(|s| !s.is_empty())
        .or_else(|| me["username"].as_str())
        .unwrap_or("unknown")
        .to_string();

    // Discord-profielkleur (accent_color, integer) → hex, voor de naam-swatch.
    if let Some(c) = me["accent_color"].as_i64() {
        let hex = format!("#{:06x}", (c as u32) & 0xff_ffff);
        db::set_discord_color(&st.pool, &uid, &name, &hex);
    }

    let sess = rand_token();
    db::create_session(&st.pool, &sess, &uid, &name, now_secs());
    tracing::info!("Login: {name} ({uid})");
    let c = format!(
        "session={sess}; HttpOnly; SameSite=Lax; Path=/; Max-Age={SESSION_MAX_AGE}"
    );
    // Terug naar de gewenste bestemming (bv. /market voor admins), met open-redirect-guard.
    // De gate laat non-admins alsnog naar /info lopen; enkel admins raken echt in /market.
    let dest = safe_next(cookie(&headers, "oauth_next").as_deref());
    let clear_next = "oauth_next=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0";
    (set_cookies(&[&c, clear_next]), Redirect::to(&dest)).into_response()
}

async fn logout(State(st): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(t) = cookie(&headers, "session") {
        db::delete_session(&st.pool, &t);
    }
    let c = "session=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0";
    (set_cookie(c), Redirect::to("/")).into_response()
}

// --- /admin : Fase-I rol-toggle (ongewijzigd) ---------------------------

#[derive(Deserialize)]
struct RevokeBody {
    /// De in-game Hytale-naam; het panel kent geen Discord-ID's.
    name: String,
}

/// **Dienst-tot-dienst**: het Hytale-panel laat market een pas intrekken. Het panel draait
/// als user `hytale` en kan `coins.db` (user `market`) enkel lézen — vandaar dat het ons
/// vraagt i.p.v. zelf te schrijven. Zo blijft market de enige schrijver van z'n eigen DB.
///
/// Beveiliging in twee lagen: Caddy blokkeert `/internal/*` van buitenaf (enkel lokale
/// diensten komen aan 127.0.0.1:8700), en dit endpoint eist een gedeeld geheim. Dat geheim
/// staat in `secrets.json` (mode 600), **niet** in de systemd-unit — die zit in git.
/// Geen geheim geconfigureerd ⇒ de route weigert alles.
async fn internal_revoke_pass(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(b): Json<RevokeBody>,
) -> JsonResp {
    let want = st.cfg.internal_secret.as_bytes();
    let got = headers
        .get("x-internal-secret")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .as_bytes();
    // Constante tijd + lengte-check: een timing-verschil zou het geheim laten raden.
    let ok = !want.is_empty()
        && want.len() == got.len()
        && want.iter().zip(got).fold(0u8, |a, (x, y)| a | (x ^ y)) == 0;
    if !ok {
        return (StatusCode::FORBIDDEN, Json(json!({"ok": false, "error": "forbidden"})));
    }

    let name = b.name.trim();
    if !valid_hytale_name(name) {
        return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "error": "bad name"})));
    }
    let hits = db::revoke_pass_by_name(&st.pool, name);
    for (uid, uname) in &hits {
        db::log_event(
            &st.pool,
            now_secs(),
            &db::LogEntry::new("admin", "pass_revoke")
                .actor(uid, uname)
                .detail(format!("pass revoked via the panel — {name} (no coins refunded)")),
        );
    }
    tracing::info!("interne revoke: {name} → {} grant(s) ingetrokken", hits.len());
    (StatusCode::OK, Json(json!({"ok": true, "revoked": hits.len()})))
}

/// De oorspronkelijke rol-toggle-UI (PoC). Blijft bestaan naast de echte Manage-sectie,
/// maar is **admin-only**: ze bedient `/api/toggle`, dat rollen op de échte guild zet.
async fn admin(State(st): State<AppState>, headers: HeaderMap) -> Response {
    if require_admin(&st, &headers).is_none() {
        return Redirect::to("/").into_response();
    }
    let tmpl = include_str!("../templates/index.html");
    let css = include_str!("../static/style.css");
    let pinned_json = serde_json::to_string(&st.cfg.user_id).unwrap_or_else(|_| "\"\"".into());
    let label_json = serde_json::to_string(&st.cfg.role_label).unwrap_or_else(|_| "\"\"".into());
    let html = tmpl
        .replace("{{STYLE}}", css)
        .replace("{{ROLE_LABEL}}", &st.cfg.role_label)
        .replace("{{PINNED_USER_JSON}}", &pinned_json)
        .replace("{{ROLE_LABEL_JSON}}", &label_json);
    Html(html).into_response()
}

fn is_digits(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

#[derive(Deserialize)]
struct StatusQuery {
    user_id: String,
}

/// Rol-status van een willekeurige Discord-ID. **Admin-only**: dit vertelt of iemand een
/// rol heeft, en dat gaat een bezoeker niet aan.
async fn api_status(
    State(st): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<StatusQuery>,
) -> JsonResp {
    if require_admin(&st, &headers).is_none() {
        return (StatusCode::FORBIDDEN, Json(json!({"ok": false, "error": "admin only"})));
    }
    let uid = q.user_id.trim().to_string();
    if !is_digits(&uid) {
        return bad("Enter a valid Discord user ID (digits).");
    }
    match st.dc.has_role(&uid, &st.cfg.role_id).await {
        Ok(Some(has)) => (StatusCode::OK, Json(json!({"ok": true, "has_role": has}))),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({"ok": false, "error": "That user is not a member of the guild."})),
        ),
        Err(e) => bad(&e),
    }
}

/// Lichte balans-polling voor de live-refresh op de site. Enkel een sessie nodig
/// (géén Discord-rolcheck per poll — dat zou elke 5s een API-call zijn).
async fn api_balance(State(st): State<AppState>, headers: HeaderMap) -> JsonResp {
    let Some((uid, _name)) = session_user(&st, &headers) else {
        return (StatusCode::UNAUTHORIZED, Json(json!({"ok": false})));
    };
    let (coins, _m, _p, total_earned) = db::get_stats(&st.pool, &uid);
    let (lvl, n, m) = level_info(total_earned);
    let pct = if m > 0 { (n * 100 / m).clamp(0, 100) } else { 100 };
    let nm = if m > 0 {
        format!("{n}/{m}")
    } else {
        "MAX".to_string()
    };
    (
        StatusCode::OK,
        Json(json!({
            "ok": true,
            "coins": coins,
            "earned": total_earned,
            "lvl": lvl,
            "pct": pct,
            "nm": nm,
        })),
    )
}

#[derive(Deserialize)]
struct ToggleBody {
    user_id: String,
    enable: bool,
}

/// Kent de shop-toegangsrol toe of neemt ze af, op de échte guild. **Admin-only** — zonder
/// deze check kan eender wie zichzelf Flowerborn maken. Stond tot 2026-07-15 enkel achter de
/// site-brede `gate`; die is bij de go-live weg, dus de check hoort hier.
async fn api_toggle(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(b): Json<ToggleBody>,
) -> JsonResp {
    if require_admin(&st, &headers).is_none() {
        return (StatusCode::FORBIDDEN, Json(json!({"ok": false, "error": "admin only"})));
    }
    let uid = b.user_id.trim().to_string();
    if !is_digits(&uid) {
        return bad("Enter a valid Discord user ID (digits).");
    }
    if let Err(e) = st.dc.set_role(&uid, &st.cfg.role_id, b.enable).await {
        return bad(&e);
    }
    match st.dc.has_role(&uid, &st.cfg.role_id).await {
        Ok(Some(has)) => (StatusCode::OK, Json(json!({"ok": true, "has_role": has}))),
        Ok(None) => (StatusCode::OK, Json(json!({"ok": true, "has_role": false}))),
        Err(e) => bad(&e),
    }
}

fn bad(msg: &str) -> JsonResp {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({"ok": false, "error": msg})),
    )
}

// --- kopen --------------------------------------------------------------

#[derive(Deserialize)]
struct BuyForm {
    item_id: i64,
    // Enkel bij de eerste pas-aankoop meegestuurd (Hytale-naam voor de whitelist).
    #[serde(default)]
    hytale_name: Option<String>,
}

#[derive(Deserialize)]
struct MarketQuery {
    ok: Option<String>,
    err: Option<String>,
    from: Option<i64>,
}

/// Bannertekst voor de Market na een koop (ok/err via query).
fn market_notice(q: &MarketQuery) -> String {
    if let Some(m) = &q.ok {
        format!("<div class=\"notice ok\">✅ {}</div>", esc(m))
    } else if let Some(m) = &q.err {
        format!("<div class=\"notice err\">⚠️ {}</div>", esc(m))
    } else {
        String::new()
    }
}

/// Koop een item. **Passen (`boost`) activeren meteen** = de speler whitelisten
/// (geen inventory-tussenstap, geen aparte Use). De eerste keer typt de koper zijn
/// Hytale-naam mee in het koopformulier; die wordt persistent bewaard en nadien niet
/// meer gevraagd. Gems e.d. blijven "kopen = ontgrendelen" (effect volgt bij Use).
async fn buy(State(st): State<AppState>, headers: HeaderMap, Form(f): Form<BuyForm>) -> Response {
    let Some((uid, name)) = require_flowerborn(&st, &headers).await else {
        return Redirect::to("/").into_response();
    };
    let Some(item) = db::get_item(&st.pool, f.item_id) else {
        return Redirect::to(&format!("/market?err={}", pct("This item no longer exists.")))
            .into_response();
    };
    // Uitverkocht: hier weigeren, niet enkel de knop grijzen. Die knop bestaat alleen in
    // de browser; deze POST kan iedereen zelf sturen. (De voorraad en de één-pas-per-persoon
    // -regel zitten in `db::purchase`, atomisch samen met het afboeken van de coins.)
    if item.sold_out {
        return Redirect::to(&format!(
            "/market?err={}",
            pct(&format!("{} is out of stock.", item.name))
        ))
        .into_response();
    }

    // --- Passen: koop = direct whitelisten -------------------------------
    if item.category == "boost" {
        // Eerste aankoop: sla de meegestuurde Hytale-naam op. ÉÉNMALIG — is er al een
        // naam, dan wordt een meegestuurde waarde genegeerd (ook bij een zelf-gemaakte
        // POST). Een lid mag zijn naam niet meer wijzigen, anders kan hij zijn pas
        // doorgeven door een andere naam te whitelisten.
        if db::get_hytale_name(&st.pool, &uid).is_empty() {
            if let Some(raw) = f.hytale_name.as_deref() {
                let n = raw.trim();
                if valid_hytale_name(n) {
                    db::set_hytale_name(&st.pool, &uid, &name, n);
                }
            }
        }
        let hname = db::get_hytale_name(&st.pool, &uid);
        if !valid_hytale_name(&hname) {
            return Redirect::to(&format!(
                "/market?err={}",
                pct("Enter your Hytale name to buy a pass.")
            ))
            .into_response();
        }
        // Al permanent? Dan is een tweede permanente pas zinloos (dagpas blokkeert
        // `purchase` zelf al).
        if item.duration == 0 && db::has_perma_access(&st.pool, &uid) {
            return Redirect::to(&format!(
                "/market?err={}",
                pct("You already have permanent access.")
            ))
            .into_response();
        }
        // Saldo eraf + regelcontrole (blokkeert dagpas-bij-perma en te laag saldo).
        let oldbal = match db::purchase(&st.pool, &uid, f.item_id, now_secs()) {
            Ok((bal, it)) => bal + it.price,
            Err(e) => return Redirect::to(&format!("/market?err={}", pct(&e))).into_response(),
        };
        // Activeer meteen: dagpas → whitelist-timer (stapelt), perma → permanente whitelist.
        let msg = if item.duration > 0 {
            let exp =
                db::grant_day_whitelist(&st.pool, &uid, &hname, item.duration as f64, now_secs());
            if exp.is_finite() {
                // Rond op hele minuten af: `now` valt ~1s ná de grant, anders "1439 min".
                let left = ((exp - now_secs()) / 60.0).round().max(0.0) as i64 * 60;
                format!("Whitelisted as {hname} — {} of access left.", human_duration(left))
            } else {
                format!("You already have permanent access ({hname}).")
            }
        } else {
            db::set_perma_access(&st.pool, &uid, &name);
            db::grant_perma_whitelist(&st.pool, &uid, &hname);
            format!("Permanent Hytale access — whitelisted as {hname}.")
        };
        // Logboek: pas gekocht (dag/perma) + wie er onder welke Hytale-naam gewhitelist is.
        db::log_event(
            &st.pool,
            now_secs(),
            &db::LogEntry::new("shop", if item.duration > 0 { "pass_day" } else { "pass_perma" })
                .actor(&uid, &name)
                .reference(item.id as u64)
                .amount(item.price)
                .detail(format!("{} → whitelisted as {hname}", item.name)),
        );
        // Publieke aankoopmelding in #coins (pas).
        announce_purchase(&st, &name, &item);
        return Redirect::to(&format!("/market?ok={}&from={}", pct(&msg), oldbal)).into_response();
    }

    // --- Gems e.d.: koop = ontgrendelen in de inventory ------------------
    let dest = match db::purchase(&st.pool, &uid, f.item_id, now_secs()) {
        // Geen succesbanner: de Purse telt zelf af naar het nieuwe saldo.
        Ok((bal, item)) => {
            // Logboek: aankoop vastleggen (coins eraf + item ontgrendeld).
            db::log_event(
                &st.pool,
                now_secs(),
                &db::LogEntry::new("shop", "buy")
                    .actor(&uid, &name)
                    .reference(item.id as u64)
                    .amount(item.price)
                    .detail(item.name.clone()),
            );
            // Publieke aankoopmelding in #coins (gem of booster — de helper kiest de tekst).
            announce_purchase(&st, &name, &item);
            format!("/market?from={}", bal + item.price)
        }
        Err(e) => format!("/market?err={}", pct(&e)),
    };
    Redirect::to(&dest).into_response()
}

/// Geldige Hytale-naam? (`^[A-Za-z0-9_]{1,32}$`)
pub(crate) fn valid_hytale_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 32
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Welke rol-ID's moeten weg bij het equipen van gem `keep`: elke rol die het lid
/// nu draagt én de naam van een gem/kleuritem heeft, behalve de gem die je equipt.
/// Puur (geen I/O) zodat de dry-run dit los kan testen.
fn other_gem_role_ids(
    all_roles: &[(String, String)], // (id, naam)
    held: &std::collections::HashSet<String>,
    gem_names: &[String],
    keep: &str,
) -> Vec<String> {
    all_roles
        .iter()
        .filter(|(rid, rname)| {
            held.contains(rid)
                && !rname.eq_ignore_ascii_case(keep)
                && gem_names.iter().any(|g| g.eq_ignore_ascii_case(rname))
        })
        .map(|(rid, _)| rid.clone())
        .collect()
}

/// Een bezeten gem "gebruiken": zet je naamkleur (site) én ken de bijhorende Discord-rol
/// toe (gem-naam = rolnaam, in `cfg.guild_id`). **Elke andere** kleur-gem-rol die het lid
/// al draagt wordt eerst weggehaald, zodat er maar één gem-kleur tegelijk actief is —
/// self-healing (niet afhankelijk van `equipped_gem`). Het item zelf blijft in de inventory.
async fn use_gem(State(st): State<AppState>, headers: HeaderMap, Form(f): Form<BuyForm>) -> Response {
    let Some((uid, name)) = require_flowerborn(&st, &headers).await else {
        return Redirect::to("/").into_response();
    };
    if !db::owned_item_ids(&st.pool, &uid).contains(&f.item_id) {
        return Redirect::to("/?tab=gems").into_response();
    }
    let Some(item) = db::get_item(&st.pool, f.item_id) else {
        return Redirect::to("/?tab=gems").into_response();
    };
    if item.category != "inventory" {
        return Redirect::to("/?tab=gems").into_response();
    }

    // Alle andere kleur-gem-rollen die dit lid nu draagt intrekken. We kijken naar de
    // échte rollen op het lid (niet naar equipped_gem), zodat ook rollen van vroegere
    // tests/handmatige toekenningen mee opgeruimd worden. Faalt de rol-/lid-lookup, dan
    // valt het terug op de oude enkelvoudige revoke via equipped_gem.
    match (st.dc.all_roles().await, st.dc.member_role_ids(&uid).await) {
        (Ok(roles), Ok(held)) => {
            let held: std::collections::HashSet<String> = held.into_iter().collect();
            let gem_names = db::inventory_item_names(&st.pool);
            for rid in other_gem_role_ids(&roles, &held, &gem_names, &item.name) {
                let _ = st.dc.set_role(&uid, &rid, false).await;
            }
        }
        _ => {
            let prev = db::get_equipped_gem(&st.pool, &uid);
            if !prev.is_empty() && !prev.eq_ignore_ascii_case(&item.name) {
                if let Ok(Some(rid)) = st.dc.role_id_by_name(&prev).await {
                    let _ = st.dc.set_role(&uid, &rid, false).await;
                }
            }
        }
    }

    // Naamkleur (site) + equipped bijhouden.
    db::set_name_color(&st.pool, &uid, &name, &item.color);
    db::set_equipped_gem(&st.pool, &uid, &item.name);

    // Logboek: gem geëquipt (naamkleur gezet + Discord-rol wisselt hieronder).
    db::log_event(
        &st.pool,
        now_secs(),
        &db::LogEntry::new("gem", "equip")
            .actor(&uid, &name)
            .detail(item.name.clone()),
    );

    // Discord-rol toekennen (gem-naam = rolnaam).
    let msg = match st.dc.role_id_by_name(&item.name).await {
        Ok(Some(rid)) => match st.dc.set_role(&uid, &rid, true).await {
            Ok(_) => format!("✨ Equipped {} — your Discord name colour is now set.", item.name),
            Err(e) => format!("⚠️ Couldn't assign the '{}' Discord role: {e}", item.name),
        },
        Ok(None) => format!("⚠️ No Discord role named '{}' found (yet).", item.name),
        Err(e) => format!("⚠️ Discord lookup failed: {e}"),
    };
    Redirect::to(&format!("/?tab=gems&msg={}", pct(&msg))).into_response()
}

/// Leg de geëquipte gem weer af: naamkleur terug naar standaard en de bijhorende
/// Discord-rol eraf. De gem blijft uiteraard in je collectie.
async fn unequip_gem(
    State(st): State<AppState>,
    headers: HeaderMap,
    Form(f): Form<BuyForm>,
) -> Response {
    let Some((uid, name)) = require_flowerborn(&st, &headers).await else {
        return Redirect::to("/").into_response();
    };
    let Some(item) = db::get_item(&st.pool, f.item_id) else {
        return Redirect::to("/?tab=gems").into_response();
    };
    // Enkel de gem die je écht draagt kan eraf — anders zou een gepost formulier van een
    // andere kaart je huidige kleur wissen.
    let equipped = db::get_equipped_gem(&st.pool, &uid);
    if !equipped.eq_ignore_ascii_case(&item.name) {
        return Redirect::to("/?tab=gems").into_response();
    }

    db::set_name_color(&st.pool, &uid, &name, "");
    db::set_equipped_gem(&st.pool, &uid, "");

    db::log_event(
        &st.pool,
        now_secs(),
        &db::LogEntry::new("gem", "unequip").actor(&uid, &name).detail(item.name.clone()),
    );

    let msg = match st.dc.role_id_by_name(&item.name).await {
        Ok(Some(rid)) => match st.dc.set_role(&uid, &rid, false).await {
            Ok(_) => format!("Unequipped {} — your name colour is back to default.", item.name),
            Err(e) => format!("⚠️ Couldn't remove the '{}' Discord role: {e}", item.name),
        },
        // Geen rol gevonden = niets in te trekken; de site-kleur is al terug op standaard.
        Ok(None) => format!("Unequipped {} — your name colour is back to default.", item.name),
        Err(e) => format!("⚠️ Discord lookup failed: {e}"),
    };
    Redirect::to(&format!("/?tab=gems&msg={}", pct(&msg))).into_response()
}

/// Human-friendly duration ("1 day", "24 h", "30 min").
fn human_duration(secs: i64) -> String {
    if secs % 86400 == 0 {
        let d = secs / 86400;
        format!("{d} day{}", if d == 1 { "" } else { "s" })
    } else if secs % 3600 == 0 {
        format!("{} h", secs / 3600)
    } else {
        format!("{} min", secs / 60)
    }
}

// --- admin: market-beheer ----------------------------------------------

/// Eén item-editor op de beheerpagina: thumb, naam, prijs, upload, verwijder.
fn admin_item(it: &db::Item, shelves: &[(i64, String)], saved: Option<i64>) -> String {
    let dur_min = it.duration / 60;
    let sel = |c: &str| if it.category == c { " selected" } else { "" };
    let so = if it.sold_out { " checked" } else { "" };
    // Voorraad: eigen formuliertje (los van Save), want "Add stock" telt óp bij wat er al
    // ligt i.p.v. een waarde te zetten — dat is hoe een admin erover denkt: "er komen er 3 bij".
    let stock_ui = {
        let nu = if it.stock < 0 {
            "<b>unlimited</b>".to_string()
        } else if it.stock == 0 {
            "<b class=\"soldout\">0 — out of stock</b>".to_string()
        } else {
            format!("<b>{}</b> in stock", it.stock)
        };
        let inf = if it.stock >= 0 {
            "<button class=\"btn small ghost\" type=\"submit\" name=\"unlimited\" value=\"1\" \
               title=\"Stop counting: unlimited, always for sale\">∞</button>"
        } else {
            ""
        };
        format!(
            "<form method=\"post\" action=\"/admin/item/stock\" class=\"stockbox\">\
               <input type=\"hidden\" name=\"id\" value=\"{id}\">\
               <div class=\"lbl\">Stock: {nu}</div>\
               <div class=\"arow\">\
                 <input class=\"num\" type=\"number\" name=\"add\" value=\"1\" step=\"1\" \
                   title=\"Hoeveel exemplaren erbij\">\
                 <button class=\"btn small\" type=\"submit\">+ Add stock</button>{inf}\
               </div></form>",
            id = it.id,
        )
    };

    // Duur. De **dagpas** (boost mét looptijd) krijgt een instelbaar minuten-veld: dát
    // getal bepaalt sinds 2026-07-15 écht hoe lang de pas geldig is (`buy()` leest
    // `item.duration`, niet meer een hardcoded 24u). Handig om verval te testen zonder
    // een dag te wachten. De **permanente** pas heeft niets in te stellen (duration 0 =
    // eeuwig) en houdt zijn vlag via een hidden input. Alle andere items hebben geen duur
    // — géén veld, en de update zet hun duration gewoon op 0.
    let dur_field = if it.category == "boost" && it.duration > 0 {
        let uren = dur_min as f64 / 60.0;
        format!(
            "<div class=\"fld\">Access (minutes)\
               <input type=\"number\" name=\"duration_min\" value=\"{dur_min}\" min=\"1\" \
                 step=\"1\" title=\"How long a day pass stays valid. 1440 = 24 hours.\">\
               <div class=\"hint\">= {uren:.1} h · 1440 = 24 h</div></div>"
        )
    } else if it.category == "boost" {
        "<div class=\"fld\">Access<div class=\"rdonly\">permanent</div></div>\
         <input type=\"hidden\" name=\"duration_min\" value=\"0\">"
            .to_string()
    } else {
        String::new()
    };


    // Bevestigings-flits na een bewaaractie (?saved=<id>).
    let flash = if saved == Some(it.id) {
        "<div class=\"savedflash\">✓ Saved</div>"
    } else {
        ""
    };

    // "Remove image" enkel tonen als er een geüploade afbeelding is.
    let remove_img = if it.image.is_empty() {
        String::new()
    } else {
        format!(
            "<form method=\"post\" action=\"/admin/item/image/clear\" class=\"iform\">\
               <input type=\"hidden\" name=\"id\" value=\"{id}\">\
               <button class=\"btn small ghost\" type=\"submit\">Remove image</button></form>",
            id = it.id,
        )
    };

    // Schap-verplaatsing: enkel voor schap-items en enkel als er >1 schap is.
    let move_shelf = if it.zone == "shelf" && shelves.len() > 1 {
        let opts: String = shelves
            .iter()
            .map(|(sid, title)| {
                let s = if it.shelf_id == Some(*sid) { " selected" } else { "" };
                format!("<option value=\"{sid}\"{s}>{}</option>", esc(title))
            })
            .collect();
        format!(
            "<form method=\"post\" action=\"/admin/item/shelf\" class=\"mvshelf\">\
               <input type=\"hidden\" name=\"id\" value=\"{id}\">\
               <select name=\"shelf_id\" title=\"Move to shelf\">{opts}</select>\
               <button class=\"btn small ghost\" type=\"submit\">Move</button></form>",
            id = it.id,
        )
    } else {
        String::new()
    };

    // Tweede afbeelding (klein, onder de titel in de shop) — voor elk item beschikbaar.
    let img2_ui = {
        let thumb2 = if it.image2.is_empty() {
            "<div class=\"thumb2 empty\">— no 2nd image —</div>".to_string()
        } else {
            format!(
                "<div class=\"thumb2\"><img src=\"/uploads/{}?v={}\" alt=\"\"></div>",
                esc(&it.image2),
                img_ver(&it.image2)
            )
        };
        let remove2 = if it.image2.is_empty() {
            String::new()
        } else {
            format!(
                "<form method=\"post\" action=\"/admin/item/image2/clear\" class=\"iform\">\
                   <input type=\"hidden\" name=\"id\" value=\"{id}\">\
                   <button class=\"btn small ghost\" type=\"submit\">Remove 2nd</button></form>",
                id = it.id,
            )
        };
        format!(
            "<div class=\"img2box\"><div class=\"lbl\">2nd image <span class=\"hint\">(under title in shop — drag &amp; drop or browse)</span></div>{thumb2}\
               <form class=\"iupload\" method=\"post\" action=\"/admin/item/image\" enctype=\"multipart/form-data\">\
                 <input type=\"hidden\" name=\"id\" value=\"{id}\">\
                 <input type=\"hidden\" name=\"slot\" value=\"2\">\
                 <input type=\"file\" name=\"file\" accept=\"image/*\">\
                 <button class=\"btn small ghost\" type=\"submit\">Browse / Upload</button></form>{remove2}</div>",
            id = it.id,
        )
    };

    format!(
        "<div class=\"aitem\" id=\"item-{id}\">{flash}\
         <div class=\"imgblock\">\
           <div class=\"lbl\">Main image <span class=\"hint\">(drag &amp; drop or browse)</span></div>\
           <div class=\"thumb\">{thumb}</div>\
           <form class=\"iupload\" method=\"post\" action=\"/admin/item/image\" enctype=\"multipart/form-data\">\
             <input type=\"hidden\" name=\"id\" value=\"{id}\">\
             <input type=\"file\" name=\"file\" accept=\"image/*\">\
             <button class=\"btn small ghost\" type=\"submit\">Browse / Upload</button></form>{remove_img}</div>\
         <form method=\"post\" action=\"/admin/item/update\">\
           <input type=\"hidden\" name=\"id\" value=\"{id}\">\
           <label class=\"fld\">Name<input name=\"name\" value=\"{name}\" placeholder=\"e.g. Amber\"></label>\
           <label class=\"fld\">Price <span class=\"hint\">(coins)</span>\
             <input name=\"price\" type=\"number\" min=\"0\" value=\"{price}\"></label>\
           <label class=\"fld\">Description <span class=\"hint\">(shown in italic in the shop)</span>\
             <input name=\"description\" value=\"{desc}\" placeholder=\"e.g. Gives the Amber role\"></label>\
           <label class=\"fld\">Type<select name=\"category\">\
             <option value=\"inventory\"{ci}>Inventory item</option>\
             <option value=\"noninv\"{cn}>Non-inventory item</option>\
             <option value=\"booster\"{cboo}>Booster (lucky item)</option>\
             <option value=\"boost\"{cb}>Hytale pass</option></select></label>\
           {dur_field}\
           <label class=\"chk\"><input type=\"checkbox\" name=\"sold_out\" value=\"1\"{so}>\
             Out of stock <span class=\"hint\">(zichtbaar, maar niet koopbaar)</span></label>\
           <button class=\"btn small save\" type=\"submit\">💾 Save</button></form>{stock_ui}{img2_ui}\
         <div class=\"arow\">\
           <form method=\"post\" action=\"/admin/item/move\" class=\"iform\">\
             <input type=\"hidden\" name=\"id\" value=\"{id}\">\
             <input type=\"hidden\" name=\"dir\" value=\"-1\">\
             <button class=\"btn small ghost\" type=\"submit\" title=\"Move left\">◀</button></form>\
           <form method=\"post\" action=\"/admin/item/move\" class=\"iform\">\
             <input type=\"hidden\" name=\"id\" value=\"{id}\">\
             <input type=\"hidden\" name=\"dir\" value=\"1\">\
             <button class=\"btn small ghost\" type=\"submit\" title=\"Move right\">▶</button></form>\
           <form method=\"post\" action=\"/admin/item/delete\" class=\"iform\" onsubmit=\"return confirm('Delete item?')\">\
             <input type=\"hidden\" name=\"id\" value=\"{id}\">\
             <button class=\"btn small danger\" type=\"submit\">Delete</button></form></div>{move_shelf}</div>",
        thumb = item_thumb(it),
        id = it.id,
        name = esc(&it.name),
        price = it.price,
        desc = esc(&it.description),
        ci = sel("inventory"),
        cn = sel("noninv"),
        cboo = sel("booster"),
        cb = sel("boost"),
        img2_ui = img2_ui,
    )
}

async fn admin_market(
    State(st): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<SavedQuery>,
) -> Response {
    let Some((_uid, name)) = require_admin(&st, &headers) else {
        return Redirect::to("/").into_response();
    };
    let saved = q.saved;
    let all_shelves = db::list_shelves(&st.pool);
    let shelves: String = all_shelves
        .iter()
        .map(|(sid, title)| {
            let items: String = db::shelf_items(&st.pool, *sid)
                .iter()
                .map(|it| admin_item(it, &all_shelves, saved))
                .collect();
            format!(
                "<section class=\"ashelf\"><div class=\"ashelf-head\">\
                   <form class=\"rn\" method=\"post\" action=\"/admin/shelf/rename\">\
                     <input type=\"hidden\" name=\"id\" value=\"{sid}\">\
                     <input name=\"title\" value=\"{title}\">\
                     <button class=\"btn small\" type=\"submit\">Rename</button></form>\
                   <form method=\"post\" action=\"/admin/shelf/delete\" onsubmit=\"return confirm('Delete shelf and its items?')\">\
                     <input type=\"hidden\" name=\"id\" value=\"{sid}\">\
                     <button class=\"btn small danger\" type=\"submit\">Delete shelf</button></form></div>\
                 <div class=\"aitems\">{items}\
                   <form method=\"post\" action=\"/admin/item/add\">\
                     <input type=\"hidden\" name=\"zone\" value=\"shelf\">\
                     <input type=\"hidden\" name=\"shelf_id\" value=\"{sid}\">\
                     <button class=\"plus\" type=\"submit\" title=\"Extra slot\">＋</button></form></div></section>",
                title = esc(title),
            )
        })
        .collect();

    let lucky_items: String = db::lucky_items(&st.pool)
        .iter()
        .map(|it| admin_item(it, &all_shelves, saved))
        .collect();
    let lucky = format!(
        "<section class=\"ashelf\"><div class=\"ashelf-head\"><b>🍀 Lucky items</b></div>\
         <div class=\"aitems\">{lucky_items}\
           <form method=\"post\" action=\"/admin/item/add\">\
             <input type=\"hidden\" name=\"zone\" value=\"lucky\">\
             <button class=\"plus\" type=\"submit\" title=\"Extra lucky item\">＋</button></form></div></section>"
    );

    let body = format!(
        "<h1>⚙ Shop management</h1>\
         <form method=\"post\" action=\"/admin/sync-gem-colors\" style=\"margin:0 0 1rem\">\
           <button class=\"btn small ghost\" type=\"submit\" title=\"Fetch each gem's color from the matching Discord role\">🎨 Sync gem colors from Discord</button></form>\
         {shelves}{lucky}\
         <form class=\"addbar\" method=\"post\" action=\"/admin/shelf/add\">\
           <input name=\"title\" placeholder=\"New shelf name\" required>\
           <button class=\"btn\" type=\"submit\">＋ Shelf</button></form>{KEEP_SCROLL_JS}{SAVED_FLASH_JS}{AUTOSAVE_JS}{DND_JS}"
    );
    let body = format!("{}{}", admin_subtabs("market"), body);
    Html(shell("Manage — Meadow Market", &chrome(&name, "admin", true, ""), true, &body)).into_response()
}

#[derive(Deserialize)]
struct CoinOp {
    user_id: String,
    #[serde(default)]
    username: String,
    amount: i64,
    #[serde(default)]
    cur: Option<String>, // checkbox "current" (coins)
    #[serde(default)]
    alltime: Option<String>, // checkbox "all time" (total_earned)
}

#[derive(Deserialize)]
struct CoinsQuery {
    #[serde(default)]
    sort: Option<String>,
}

/// Admin coins-beheer (prod-guild): alle leden (ook 0-coin), Add/Set, undo, archief.
async fn admin_coins(
    State(st): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<CoinsQuery>,
) -> Response {
    let refresh = auto_refresh_js(AUTO_REFRESH_MS);
    let Some((_uid, name)) = require_admin(&st, &headers) else {
        return Redirect::to("/").into_response();
    };
    let sort = q.sort.as_deref().unwrap_or("desc"); // az | za | asc | desc

    let balances = db::all_balances(&st.pool);
    let earned = db::all_earned(&st.pool);
    let archives = db::all_archives(&st.pool);

    // Namen uit de prod-guild, aangevuld met archief-namen / id's.
    let mut names: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let note = match st.dc.list_members(COINS_GUILD_ID).await {
        Ok(members) => {
            for (id, n) in members {
                names.insert(id, n);
            }
            String::new()
        }
        Err(e) => format!(
            "<p class=\"notice err\">Couldn't fetch the member list ({}).</p>",
            esc(&e)
        ),
    };

    // Iedereen tonen: alle leden + wie een saldo of archief heeft.
    let mut uids: std::collections::HashSet<String> = names.keys().cloned().collect();
    for (id, b) in &balances {
        if *b != 0 {
            uids.insert(id.clone());
        }
    }
    uids.extend(archives.keys().cloned());
    for id in &uids {
        names.entry(id.clone()).or_insert_with(|| {
            archives
                .get(id)
                .map(|(_, _, n)| n.clone())
                .unwrap_or_else(|| id.clone())
        });
    }

    let name_of = |id: &String| names.get(id).map(|s| s.to_lowercase()).unwrap_or_default();
    let bal_of = |id: &String| *balances.get(id).unwrap_or(&0);
    let mut list: Vec<String> = uids.into_iter().collect();
    match sort {
        "az" => list.sort_by_key(name_of),
        "za" => {
            list.sort_by_key(name_of);
            list.reverse();
        }
        "asc" => list.sort_by(|a, b| bal_of(a).cmp(&bal_of(b)).then_with(|| name_of(a).cmp(&name_of(b)))),
        _ => list.sort_by(|a, b| bal_of(b).cmp(&bal_of(a)).then_with(|| name_of(a).cmp(&name_of(b)))),
    }

    let rows: String = list
        .iter()
        .map(|uid| {
            let bal = *balances.get(uid).unwrap_or(&0);
            let ea = *earned.get(uid).unwrap_or(&0);
            let nm = esc(names.get(uid).map(|s| s.as_str()).unwrap_or(uid));
            let archive = match archives.get(uid) {
                Some((c, _e, _n)) => format!(
                    "<div class=\"archline\"><span class=\"amuted\">left the server with {MC} {c}</span>\
                     <form method=\"post\" action=\"/admin/coins/restore\" class=\"iform\">\
                       <input type=\"hidden\" name=\"user_id\" value=\"{uid}\">\
                       <button class=\"btn small\">Restore</button></form>\
                     <form method=\"post\" action=\"/admin/coins/discard\" class=\"iform\">\
                       <input type=\"hidden\" name=\"user_id\" value=\"{uid}\">\
                       <button class=\"btn small ghost\">Discard</button></form></div>"
                ),
                None => String::new(),
            };
            format!(
                "<tr><td class=\"cname\">{nm}</td><td class=\"cbal\">{MC} {bal}</td>\
                 <td class=\"cbal\">{MC} {ea}</td>\
                 <td class=\"cact\"><form method=\"post\" class=\"coinform\">\
                   <input type=\"hidden\" name=\"user_id\" value=\"{uid}\">\
                   <input type=\"hidden\" name=\"username\" value=\"{nm}\">\
                   <input type=\"number\" name=\"amount\" value=\"0\" required>\
                   <label class=\"cbx\"><input type=\"checkbox\" name=\"cur\" value=\"1\" checked> current</label>\
                   <label class=\"cbx\"><input type=\"checkbox\" name=\"alltime\" value=\"1\" checked> all&nbsp;time</label>\
                   <button class=\"btn small\" formaction=\"/admin/coins/add\">Add</button>\
                   <button class=\"btn small ghost\" formaction=\"/admin/coins/set\">Set</button>\
                 </form>{archive}</td></tr>"
            )
        })
        .collect();

    let undo = match db::admin_get_undo(&st.pool) {
        Some((_id, uname, pc, pe)) => format!(
            "<form method=\"post\" action=\"/admin/coins/undo\" class=\"undoform\" \
               title=\"Undo the last change ({nm} → current {pc}, all-time {pe})\">\
               <button class=\"btn small\" type=\"submit\">↶ Undo</button></form>\
             <span class=\"muted undonote\">last: <b>{nm}</b> → revert to current {pc} / all-time {pe}</span>",
            nm = esc(&uname),
        ),
        None => "<span class=\"undoform\"><button class=\"btn small ghost\" type=\"button\" disabled \
                 title=\"Nothing to undo yet\">↶ Undo</button></span>\
                 <span class=\"muted undonote\">nothing to undo yet</span>"
            .to_string(),
    };

    // Sorteerknoppen: A–Z, Z–A, coins ↑ (oplopend), coins ↓ (aflopend).
    let sbtn = |key: &str, label: &str| {
        let on = if sort == key { " on" } else { "" };
        format!("<a class=\"btn small ghost{on}\" href=\"/admin/coins?sort={key}\">{label}</a>")
    };
    let sorts = format!(
        "{}{}{}{}",
        sbtn("az", "A–Z"),
        sbtn("za", "Z–A"),
        sbtn("asc", "Coins ↑"),
        sbtn("desc", "Coins ↓")
    );

    let body = format!(
        "<div class=\"chead\"><h1>🪙 Coins management</h1>{undo}</div>\
         <div class=\"ctoolbar\">{sorts}</div>{note}\
         <table class=\"ctable\"><thead><tr><th>Member</th><th>Balance</th><th>All-time</th><th>Adjust</th></tr></thead>\
         <tbody>{rows}</tbody></table>{refresh}"
    );
    let body = format!("{}{}", admin_subtabs("coins"), body);
    Html(shell(
        "Coins — Meadow Market",
        &chrome(&name, "admin", true, ""),
        true,
        &body,
    ))
    .into_response()
}

/// Voer een Add/Set uit op de aangevinkte rekening(en) en bewaar de undo.
fn apply_coin_op(st: &AppState, f: &CoinOp, set: bool, admin: &str) {
    let uid = f.user_id.trim();
    if uid.is_empty() {
        return;
    }
    let current = f.cur.is_some();
    let alltime = f.alltime.is_some();
    if !current && !alltime {
        return; // geen rekening aangevinkt → niks doen
    }
    let (pc, pe) = db::admin_adjust(&st.pool, uid, &f.username, f.amount, set, current, alltime);
    db::admin_record_undo(&st.pool, uid, &f.username, pc, pe);
    // Logboek: welke rekening(en) een admin met hoeveel aanpaste (add/set).
    let acct = match (current, alltime) {
        (true, true) => "balance+all-time",
        (true, false) => "balance",
        (false, true) => "all-time",
        (false, false) => "—",
    };
    db::log_event(
        &st.pool,
        now_secs(),
        &db::LogEntry::new("admin", if set { "coins_set" } else { "coins_add" })
            .actor(uid, &f.username)
            .amount(f.amount)
            .detail(format!("{acct} · by {admin}")),
    );
}

async fn admin_coins_add(
    State(st): State<AppState>,
    headers: HeaderMap,
    Form(f): Form<CoinOp>,
) -> Response {
    if let Some((_uid, admin)) = require_admin(&st, &headers) {
        apply_coin_op(&st, &f, false, &admin);
    }
    Redirect::to("/admin/coins").into_response()
}

async fn admin_coins_set(
    State(st): State<AppState>,
    headers: HeaderMap,
    Form(f): Form<CoinOp>,
) -> Response {
    if let Some((_uid, admin)) = require_admin(&st, &headers) {
        apply_coin_op(&st, &f, true, &admin);
    }
    Redirect::to("/admin/coins").into_response()
}

async fn admin_coins_undo(State(st): State<AppState>, headers: HeaderMap) -> Response {
    if let Some((_uid, admin)) = require_admin(&st, &headers) {
        db::admin_apply_undo(&st.pool);
        db::log_event(
            &st.pool,
            now_secs(),
            &db::LogEntry::new("admin", "coins_undo").detail(format!("by {admin}")),
        );
    }
    Redirect::to("/admin/coins").into_response()
}

#[derive(Deserialize)]
struct UidForm {
    user_id: String,
}

async fn admin_coins_restore(
    State(st): State<AppState>,
    headers: HeaderMap,
    Form(f): Form<UidForm>,
) -> Response {
    if let Some((_uid, admin)) = require_admin(&st, &headers) {
        let uid = f.user_id.trim();
        if !uid.is_empty() {
            db::restore_archive(&st.pool, uid);
            db::log_event(
                &st.pool,
                now_secs(),
                &db::LogEntry::new("admin", "coins_restore")
                    .actor(uid, "")
                    .detail(format!("by {admin}")),
            );
        }
    }
    Redirect::to("/admin/coins").into_response()
}

async fn admin_coins_discard(
    State(st): State<AppState>,
    headers: HeaderMap,
    Form(f): Form<UidForm>,
) -> Response {
    if let Some((_uid, admin)) = require_admin(&st, &headers) {
        let uid = f.user_id.trim();
        if !uid.is_empty() {
            db::discard_archive(&st.pool, uid);
            db::log_event(
                &st.pool,
                now_secs(),
                &db::LogEntry::new("admin", "coins_discard")
                    .actor(uid, "")
                    .detail(format!("by {admin}")),
            );
        }
    }
    Redirect::to("/admin/coins").into_response()
}

// --- admin: coin-kanalen ------------------------------------------------

#[derive(Deserialize)]
struct ChannelAdd {
    channel: String,
}
#[derive(Deserialize)]
struct ChannelRemove {
    channel_id: String,
}

#[derive(Deserialize)]
struct LogQuery {
    #[serde(default)]
    cat: Option<String>, // filter op categorie ('' / afwezig = alles)
    #[serde(default)]
    err: Option<String>, // foutmelding na een mislukte refund
}

/// Server-logboek (admin): alle gelogde events, nieuwste eerst, filterbaar op
/// categorie. Nu vooral chest-events (spawn/join/win/despawn/te-laat); later
/// breiden we de categorieën en filters uit.
/// De filterknoppen op de logpagina: (`?cat=`-sleutel, knoptekst, categorieën erachter).
/// Eén knop mag meerdere categorieën bundelen — "Inventory" zit verspreid over `gem`
/// (equip/unequip) en `booster` (gebruik), maar is voor een admin één ding.
const LOG_GROUPS: [(&str, &str, &[&str]); 6] = [
    ("shop", "🛒 Shop", &["shop"]),
    ("inventory", "🎒 Inventory", &["gem", "booster"]),
    ("chest", "🎁 Chests", &["chest"]),
    ("coins", "🪙 Coins", &["daily", "level"]),
    ("admin", "⚙ Admin", &["admin"]),
    ("twitch", "🟣 Twitch", &["twitch"]),
];

/// De categorieën achter een filterknop, of None als de sleutel geen groep is.
fn log_group(key: &str) -> Option<&'static [&'static str]> {
    LOG_GROUPS.iter().find(|(k, _, _)| *k == key).map(|(_, _, cats)| *cats)
}

async fn admin_log(
    State(st): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<LogQuery>,
) -> Response {
    let refresh = auto_refresh_js(AUTO_REFRESH_MS);
    let Some((_uid, name)) = require_admin(&st, &headers) else {
        return Redirect::to("/").into_response();
    };
    let cats = db::log_categories(&st.pool);
    let active = q.cat.as_deref().filter(|c| !c.is_empty());
    // Een filterknop staat voor een groep categorieën, niet per se voor één. Onbekende
    // `?cat=` (bv. een oude link) valt terug op de categorie zelf, zodat niets breekt.
    let selected: Vec<&str> = match active {
        Some(k) => log_group(k).unwrap_or(&[]).to_vec(),
        None => Vec::new(),
    };
    let selected: Vec<&str> = match (active, selected.is_empty()) {
        (Some(k), true) => vec![k],
        _ => selected,
    };
    let rows = db::recent_log(&st.pool, &selected, 500);

    // Filterknoppen: "All" + de vaste groepen, daarna nog losse knoppen voor categorieën
    // die (nog) in geen groep zitten — zo verdwijnt een nieuw event-type nooit uit beeld.
    let chip = |key: Option<&str>, label: &str| {
        let on = active == key;
        let href = match key {
            Some(k) => format!("/admin/log?cat={k}"),
            None => "/admin/log".to_string(),
        };
        let cls = if on { "chip on" } else { "chip" };
        format!("<a class=\"{cls}\" href=\"{href}\">{}</a>", esc(label))
    };
    let mut filters = chip(None, "All");
    for (key, label, _) in LOG_GROUPS {
        filters.push_str(&chip(Some(key), label));
    }
    let grouped: Vec<&str> = LOG_GROUPS.iter().flat_map(|(_, _, c)| c.iter().copied()).collect();
    for c in cats.iter().filter(|c| !grouped.contains(&c.as_str())) {
        filters.push_str(&chip(Some(c), c));
    }

    // Eén gekleurd label per event-type (uitbreidbaar; onbekend = grijs).
    let badge = |cat: &str, event: &str| -> String {
        let (bg, txt) = match (cat, event) {
            ("chest", "spawn") => ("#3b5bdb", "🎁 spawn"),
            ("chest", "join") => ("#2f9e44", "✅ join"),
            ("chest", "already_in") => ("#868e96", "↩ already in"),
            ("chest", "too_late") => ("#e8590c", "⏰ too late"),
            ("chest", "win") => ("#f08c00", "🏆 win"),
            ("chest", "despawn") => ("#adb5bd", "💧 despawn"),
            // Shop-aankopen
            ("shop", "buy") => ("#7048e8", "🛒 buy"),
            ("shop", "pass_day") => ("#1098ad", "🎫 day pass"),
            ("shop", "pass_perma") => ("#0c8599", "🎟 perma pass"),
            // Inventory-gebruik
            ("gem", "equip") => ("#e64980", "💎 equip gem"),
            ("gem", "unequip") => ("#a61e4d", "💎 unequip gem"),
            ("booster", "use") => ("#66a80f", "🍀 booster"),
            // Dagelijkse check-in + level-up
            ("daily", "checkin") => ("#f59f00", "📅 daily"),
            ("level", "levelup") => ("#f76707", "⬆ level-up"),
            // Twitch-whitelist
            ("twitch", "whitelist") => ("#9146FF", "🟣 twitch pass"),
            ("twitch", "rejected") => ("#e03131", "🚫 twitch reject"),
            // Admin-ingrepen
            ("admin", "coins_add") => ("#495057", "➕ coins add"),
            ("admin", "coins_set") => ("#343a40", "🖊 coins set"),
            ("admin", "coins_undo") => ("#868e96", "↶ undo"),
            ("admin", "coins_restore") => ("#2b8a3e", "♻ restore"),
            ("admin", "coins_discard") => ("#c92a2a", "🗑 discard"),
            ("admin", "reset_collection") => ("#a61e4d", "🧪 test reset"),
            ("admin", "refund") => ("#1971c2", "↩ refund"),
            ("admin", "item_add") => ("#5f3dc4", "➕ item added"),
            ("admin", "item_update") => ("#6741d9", "🏷 item changed"),
            ("admin", "item_delete") => ("#862e9c", "🗑 item deleted"),
            ("admin", "correction") => ("#c92a2a", "🩹 correction"),
            ("admin", "shop_reroll") => ("#3b5bdb", "↻ shop reroll"),
            _ => ("#868e96", event),
        };
        format!(
            "<span class=\"badge\" style=\"background:{bg}\">{}</span>",
            esc(txt)
        )
    };

    // Refund-actie: enkel op shop-aankopen. Nog niet gerefund → knop; anders een tag.
    let cat_hidden = match active {
        Some(c) => format!("<input type=\"hidden\" name=\"cat\" value=\"{}\">", esc(c)),
        None => String::new(),
    };
    let action = |r: &db::LogRow| -> String {
        if r.category != "shop" {
            return String::new();
        }
        if r.refunded {
            return "<span class=\"muted refd\">↩ refunded</span>".to_string();
        }
        format!(
            "<form method=\"post\" action=\"/admin/refund\" class=\"iform\" \
               onsubmit=\"return confirm('Refund this purchase? Coins go back and the item/pass is removed.')\">\
               <input type=\"hidden\" name=\"log_id\" value=\"{id}\">{cat_hidden}\
               <button class=\"refbtn\" type=\"submit\">↩ Refund</button></form>",
            id = r.id,
        )
    };

    let body_rows: String = if rows.is_empty() {
        "<tr><td colspan=\"6\" class=\"muted\">No events logged yet.</td></tr>".to_string()
    } else {
        rows.iter()
            .map(|r| {
                let actor = if r.actor_name.is_empty() {
                    "<span class=\"muted\">—</span>".to_string()
                } else {
                    esc(&r.actor_name)
                };
                let amt = match r.amount {
                    Some(a) => format!("<b>{a}</b>"),
                    None => "<span class=\"muted\">—</span>".to_string(),
                };
                format!(
                    "<tr>\
                       <td class=\"tsc\"><span class=\"ts\" data-ts=\"{ts}\"></span></td>\
                       <td>{badge}</td>\
                       <td>{actor}</td>\
                       <td class=\"amt\">{amt}</td>\
                       <td class=\"det\">{detail}</td>\
                       <td class=\"act\">{action}</td>\
                     </tr>",
                    ts = r.ts as i64,
                    badge = badge(&r.category, &r.event),
                    detail = esc(&r.detail),
                    action = action(r),
                )
            })
            .collect()
    };

    // Klok-script: zet de epoch-tijden om naar de lokale tijd van de kijker.
    let ts_js = "<script>document.querySelectorAll('.ts').forEach(function(e){\
        var t=parseInt(e.dataset.ts,10);if(t)e.textContent=new Date(t*1000)\
        .toLocaleString([], {month:'short',day:'2-digit',hour:'2-digit',minute:'2-digit',second:'2-digit'});});</script>";

    let style = "<style>\
        .chips{display:flex;flex-wrap:wrap;gap:.4rem;margin:.6rem 0 1rem}\
        .chip{padding:.25rem .7rem;border-radius:999px;background:#e9ecef;color:#495057;\
          text-decoration:none;font-size:.85rem;border:1px solid transparent}\
        .chip.on{background:#495057;color:#fff}\
        table.log{width:100%;border-collapse:collapse;font-size:.9rem}\
        table.log th,table.log td{padding:.4rem .5rem;text-align:left;border-bottom:1px solid #e9ecef;vertical-align:top}\
        table.log th{font-size:.75rem;text-transform:uppercase;letter-spacing:.03em;color:#868e96}\
        .tsc{white-space:nowrap;color:#495057;font-variant-numeric:tabular-nums}\
        .badge{display:inline-block;padding:.12rem .5rem;border-radius:.4rem;color:#fff;font-size:.8rem;white-space:nowrap}\
        .amt{text-align:right;white-space:nowrap}\
        .det{color:#495057}\
        .act{white-space:nowrap;text-align:right}\
        .iform{display:inline;margin:0}\
        .refbtn{cursor:pointer;border:1px solid #1971c2;background:#e7f5ff;color:#1971c2;\
          border-radius:.4rem;padding:.15rem .55rem;font-size:.8rem}\
        .refbtn:hover{background:#1971c2;color:#fff}\
        .refd{font-size:.8rem}\
        </style>";

    let err_banner = match q.err.as_deref().filter(|e| !e.is_empty()) {
        Some(e) => format!("<p class=\"notice err\">⚠️ {}</p>", esc(e)),
        None => String::new(),
    };

    let body = format!(
        "{style}<h1>📜 Server log</h1>{err_banner}\
         <div class=\"chips\">{filters}</div>\
         <table class=\"log\"><thead><tr>\
           <th>When</th><th>Event</th><th>Who</th><th>Amount</th><th>Detail</th><th></th>\
         </tr></thead><tbody>{body_rows}</tbody></table>{ts_js}{refresh}"
    );
    let body = format!("{}{}", admin_subtabs("log"), body);
    Html(shell(
        "Server log — Meadow Market",
        &chrome(&name, "admin", true, ""),
        true,
        &body,
    ))
    .into_response()
}

#[derive(Deserialize)]
struct RefundForm {
    log_id: i64,
    #[serde(default)]
    cat: Option<String>, // om na de refund terug naar dezelfde filter te keren
}

/// Draai één shop-aankoop terug vanaf de logpagina: coins terug, item weg,
/// neveneffecten (whitelist/perma/gem-rol) ingetrokken, en de logrij gemarkeerd.
async fn admin_refund(
    State(st): State<AppState>,
    headers: HeaderMap,
    Form(f): Form<RefundForm>,
) -> Response {
    let back = match f.cat.as_deref().filter(|c| !c.is_empty()) {
        Some(c) => format!("/admin/log?cat={c}"),
        None => "/admin/log".to_string(),
    };
    let Some((admin_uid, admin)) = require_admin(&st, &headers) else {
        return Redirect::to("/").into_response();
    };
    match db::refund_purchase(&st.pool, f.log_id) {
        Ok(out) => {
            // Gem-rol op Discord intrekken bij de kóper (db-laag kon dat niet async doen).
            if !out.gem_role_removed.is_empty() && !out.buyer_uid.is_empty() {
                if let Ok(Some(rid)) = st.dc.role_id_by_name(&out.gem_role_removed).await {
                    let _ = st.dc.set_role(&out.buyer_uid, &rid, false).await;
                }
            }
            // De refund zelf loggen (audittrail), met wie 'm uitvoerde.
            db::log_event(
                &st.pool,
                now_secs(),
                &db::LogEntry::new("admin", "refund")
                    .actor(&admin_uid, &admin)
                    .amount(out.amount)
                    .detail(format!("refunded {} · by {admin}", out.item_name)),
            );
        }
        Err(e) => {
            return Redirect::to(&format!("{back}{}err={}", sep(&back), pct(&e))).into_response();
        }
    }
    Redirect::to(&back).into_response()
}

/// '?' of '&' kiezen om een querystring aan een URL te hangen.
fn sep(url: &str) -> &'static str {
    if url.contains('?') {
        "&"
    } else {
        "?"
    }
}

/// Beheer de lijst van kanalen waar coins verdiend kunnen worden.
async fn admin_channels(State(st): State<AppState>, headers: HeaderMap) -> Response {
    let refresh = auto_refresh_js(AUTO_REFRESH_MS);
    let Some((_uid, name)) = require_admin(&st, &headers) else {
        return Redirect::to("/").into_response();
    };
    let current = db::coin_channels(&st.pool);
    let current_ids: std::collections::HashSet<&String> =
        current.iter().map(|(id, _)| id).collect();

    let (options, note) = match st.dc.list_channels(COINS_GUILD_ID).await {
        Ok(chans) => {
            let opts: String = chans
                .iter()
                .filter(|(id, _)| !current_ids.contains(id))
                .map(|(id, n)| {
                    format!(
                        "<option value=\"{id}|{n}\">#{nm}</option>",
                        nm = esc(n),
                        n = esc(n)
                    )
                })
                .collect();
            (opts, String::new())
        }
        Err(e) => (
            String::new(),
            format!(
                "<p class=\"notice err\">Couldn't fetch the channel list ({}).</p>",
                esc(&e)
            ),
        ),
    };

    let list = if current.is_empty() {
        "<li class=\"muted\">No channels yet — coins can't be earned anywhere until you add one.</li>"
            .to_string()
    } else {
        current
            .iter()
            .map(|(id, n)| {
                format!(
                    "<li class=\"chrow\"><span class=\"chname\">#{nm}</span>\
                     <form method=\"post\" action=\"/admin/channels/remove\" class=\"iform\" title=\"Remove\">\
                       <input type=\"hidden\" name=\"channel_id\" value=\"{id}\">\
                       <button class=\"chrm\" type=\"submit\">✕</button></form></li>",
                    nm = esc(n)
                )
            })
            .collect()
    };

    let body = format!(
        "<h1>📋 Coin channels</h1>{note}\
         <ul class=\"chlist\">{list}</ul>\
         <form method=\"post\" action=\"/admin/channels/add\" class=\"addbar\">\
           <select name=\"channel\" required>\
             <option value=\"\" disabled selected>Pick a channel…</option>{options}</select>\
           <button class=\"btn\" type=\"submit\">＋ Add</button></form>{refresh}"
    );
    let body = format!("{}{}", admin_subtabs("channels"), body);
    Html(shell(
        "Coin channels — Meadow Market",
        &chrome(&name, "admin", true, ""),
        true,
        &body,
    ))
    .into_response()
}

async fn admin_channels_add(
    State(st): State<AppState>,
    headers: HeaderMap,
    Form(f): Form<ChannelAdd>,
) -> Response {
    if require_admin(&st, &headers).is_some() {
        if let Some((id, nm)) = f.channel.split_once('|') {
            if !id.trim().is_empty() {
                db::add_coin_channel(&st.pool, id.trim(), nm.trim());
            }
        }
    }
    Redirect::to("/admin/channels").into_response()
}

async fn admin_channels_remove(
    State(st): State<AppState>,
    headers: HeaderMap,
    Form(f): Form<ChannelRemove>,
) -> Response {
    if require_admin(&st, &headers).is_some() {
        let id = f.channel_id.trim();
        if !id.is_empty() {
            db::remove_coin_channel(&st.pool, id);
        }
    }
    Redirect::to("/admin/channels").into_response()
}

// --- ⚙ Settings ---------------------------------------------------------
// De economie-parameters die vroeger als `const` in bot.rs stonden. Alles wat
// hier gezet wordt, wordt LIVE gelezen door de bot — geen deploy, geen herstart.
// De veldenlijst zelf komt uit `settings::SPECS`, dus een nieuwe parameter
// toevoegen = één Spec bijzetten; deze pagina tekent hem vanzelf.

/// Kies de eenheid die achter het invoerveld komt, uit het achtervoegsel van de
/// sleutel. Dat is meteen de reden dat de unit ín de sleutelnaam zit.
fn unit_of(key: &str) -> &'static str {
    if key.ends_with("_sec") {
        "seconds"
    } else if key.ends_with("_min") {
        "minutes"
    } else if key.ends_with("_hours") {
        "hours"
    } else if key.ends_with("_coins") {
        "coins"
    } else if key.ends_with("_days") {
        "days"
    } else {
        ""
    }
}

async fn admin_settings(State(st): State<AppState>, headers: HeaderMap) -> Response {
    let Some((_uid, name)) = require_admin(&st, &headers) else {
        return Redirect::to("/").into_response();
    };

    // De losse parameters, gegroepeerd zoals ze in SPECS staan.
    let mut groups = String::new();
    let mut current_group = "";
    for sp in settings::SPECS {
        if sp.group != current_group {
            if !current_group.is_empty() {
                groups.push_str("</div>");
            }
            groups.push_str(&format!(
                "<div class=\"sgroup\"><h2>{}</h2>",
                esc(sp.group)
            ));
            current_group = sp.group;
        }
        let val = settings::f64_of(&st.pool, sp.key);
        let field = match sp.kind {
            // Het verborgen `on_form`-veld is er omdat een uitgevinkt vakje in HTML
            // niets meestuurt: zonder dit ziet de save-route geen verschil tussen
            // "uitgezet" en "stond niet op dit formulier", en zou een partiële POST
            // stil elk vinkje uitzetten.
            settings::Kind::Bool => format!(
                "<input type=\"hidden\" name=\"on_form\" value=\"{k}\">\
                 <input type=\"checkbox\" id=\"{k}\" name=\"{k}\" value=\"1\"{on}>",
                k = sp.key,
                on = if val != 0.0 { " checked" } else { "" },
            ),
            settings::Kind::Int => format!(
                "<input type=\"number\" id=\"{k}\" name=\"{k}\" value=\"{v}\" min=\"{min}\" max=\"{max}\" step=\"1\">",
                k = sp.key,
                v = val.round() as i64,
                min = sp.min,
                max = sp.max,
            ),
        };
        groups.push_str(&format!(
            "<div class=\"sfield\"><label for=\"{k}\">{label}</label>{field}\
             <span class=\"unit\">{unit}</span>\
             <div class=\"shelp\">{help}</div></div>",
            k = sp.key,
            label = esc(sp.label),
            unit = unit_of(sp.key),
            help = esc(sp.help),
        ));
    }
    if !current_group.is_empty() {
        groups.push_str("</div>");
    }

    // Weegsysteem 1 — coins per bericht.
    let cw = db::coin_weights_all(&st.pool);
    let cw_total: f64 = cw.iter().map(|(_, w)| w.max(0.0)).sum();
    let cw_rows: String = cw
        .iter()
        .map(|(amount, w)| {
            let pct = if cw_total > 0.0 { w / cw_total * 100.0 } else { 0.0 };
            format!(
                "<tr><td><b>+{amount}</b> coins</td>\
                 <td><form method=\"post\" action=\"/admin/settings/weight/set\" class=\"iform\">\
                   <input type=\"hidden\" name=\"amount\" value=\"{amount}\">\
                   <input type=\"text\" name=\"weight\" value=\"{w}\" inputmode=\"decimal\">\
                   <button class=\"btn\" type=\"submit\">✓</button></form></td>\
                 <td class=\"pct\">{pct:.1}%<span class=\"bar\" style=\"width:{bar:.1}%\"></span></td>\
                 <td><form method=\"post\" action=\"/admin/settings/weight/delete\" class=\"iform\">\
                   <input type=\"hidden\" name=\"amount\" value=\"{amount}\">\
                   <button class=\"chrm\" type=\"submit\" title=\"Verwijderen\">✕</button></form></td></tr>",
                bar = pct.min(100.0),
            )
        })
        .collect();

    // Weegsysteem 2 — chest-prijsverdeling.
    let ct = db::chest_tiers_all(&st.pool);
    let ct_total: f64 = ct.iter().map(|(_, w, _, _)| w.max(0.0)).sum();
    let ct_rows: String = ct
        .iter()
        .map(|(id, w, lo, hi)| {
            let pct = if ct_total > 0.0 { w / ct_total * 100.0 } else { 0.0 };
            format!(
                "<tr><td colspan=\"2\">\
                   <form method=\"post\" action=\"/admin/settings/tier/update\" class=\"iform prow\">\
                     <input type=\"hidden\" name=\"id\" value=\"{id}\">\
                     <input type=\"text\" name=\"weight\" value=\"{w}\" inputmode=\"decimal\" title=\"Gewicht\">\
                     <input type=\"number\" name=\"lo\" value=\"{lo}\" title=\"Min coins\">\
                     <input type=\"number\" name=\"hi\" value=\"{hi}\" title=\"Max coins\">\
                     <button class=\"btn\" type=\"submit\">✓</button></form></td>\
                 <td class=\"pct\">{pct:.1}%<span class=\"bar\" style=\"width:{bar:.1}%\"></span></td>\
                 <td><form method=\"post\" action=\"/admin/settings/tier/delete\" class=\"iform\">\
                   <input type=\"hidden\" name=\"id\" value=\"{id}\">\
                   <button class=\"chrm\" type=\"submit\" title=\"Verwijderen\">✕</button></form></td></tr>",
                bar = pct.min(100.0),
            )
        })
        .collect();

    let body = format!(
        "<h1>⚙ Settings</h1>\
         <p class=\"shelp\" style=\"margin:0 0 .5rem\">Alles hier werkt <b>meteen</b> — de bot leest \
          deze waarden live, dus er is geen herstart of deploy nodig.</p>\
         <form method=\"post\" action=\"/admin/settings/save\">{groups}\
           <div class=\"ctoolbar\" style=\"margin-top:1.2rem\">\
             <button class=\"btn\" type=\"submit\">Opslaan</button></div></form>\
         <div class=\"sgroup\"><h2>Coins per bericht — verdeling</h2>\
          <p class=\"shelp\" style=\"margin:0 0 .6rem\">Gewichten zijn <b>relatief</b>: de som mag alles \
           zijn. Een gewicht van 0,5 naast een 1 betekent gewoon 'half zoveel kans'. Het percentage \
           rechts is berekend en klopt dus altijd.</p>\
          <table class=\"wtable\"><thead><tr><th>Uitkomst</th><th>Gewicht</th><th>Kans</th><th></th></tr></thead>\
          <tbody>{cw_rows}</tbody>\
          <tfoot><tr><td colspan=\"4\">Som van de gewichten: <b>{cw_total}</b></td></tr></tfoot></table>\
          <form method=\"post\" action=\"/admin/settings/weight/set\" class=\"addbar\">\
            <input type=\"number\" name=\"amount\" placeholder=\"Coins (bv. 6)\" required>\
            <input type=\"text\" name=\"weight\" placeholder=\"Gewicht (bv. 0,2)\" inputmode=\"decimal\" required>\
            <button class=\"btn\" type=\"submit\">＋ Uitkomst</button></form></div>\
         <div class=\"sgroup\"><h2>Treasure chest — prijsverdeling</h2>\
          <p class=\"shelp\" style=\"margin:0 0 .6rem\">Per tier een gewicht en een coin-bereik \
           (min–max). Er wordt eerst een tier getrokken, dan een bedrag binnen dat bereik.</p>\
          <table class=\"wtable\"><thead><tr><th colspan=\"2\">Gewicht · min · max</th><th>Kans</th><th></th></tr></thead>\
          <tbody>{ct_rows}</tbody>\
          <tfoot><tr><td colspan=\"4\">Som van de gewichten: <b>{ct_total}</b></td></tr></tfoot></table>\
          <form method=\"post\" action=\"/admin/settings/tier/add\" class=\"addbar\">\
            <input type=\"text\" name=\"weight\" placeholder=\"Gewicht\" inputmode=\"decimal\" required>\
            <input type=\"number\" name=\"lo\" placeholder=\"Min\" required>\
            <input type=\"number\" name=\"hi\" placeholder=\"Max\" required>\
            <button class=\"btn\" type=\"submit\">＋ Tier</button></form></div>\
         {KEEP_SCROLL_JS}{SAVED_FLASH_JS}"
    );
    let body = format!("{}{}", admin_subtabs("settings"), body);
    Html(shell(
        "Settings — Meadow Market",
        &chrome(&name, "admin", true, ""),
        true,
        &body,
    ))
    .into_response()
}

/// Alle losse parameters in één keer. Als paren-lijst i.p.v. een map, want het
/// formulier stuurt `on_form` één keer per vinkje en een map zou daar maar één
/// van overhouden. Een veld dat niet meekwam blijft ongemoeid — zo raakt een
/// gedeeltelijk formulier nooit stil de rest van de economie.
async fn admin_settings_save(
    State(st): State<AppState>,
    headers: HeaderMap,
    Form(f): Form<Vec<(String, String)>>,
) -> Response {
    if require_admin(&st, &headers).is_some() {
        let value_of = |key: &str| f.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
        // Welke vinkjes stonden er op dit formulier? (Zie de `on_form`-comment
        // bij het renderen: uitgevinkt = geen veld, dus dit is het enige verschil
        // tussen "uitgezet" en "niet getoond".)
        let on_form: Vec<&str> =
            f.iter().filter(|(k, _)| k == "on_form").map(|(_, v)| v.as_str()).collect();
        for sp in settings::SPECS {
            let raw = match sp.kind {
                settings::Kind::Bool => {
                    if !on_form.contains(&sp.key) {
                        continue; // vinkje stond niet op dit formulier
                    }
                    if value_of(sp.key).is_some() { "1" } else { "0" }.to_string()
                }
                settings::Kind::Int => match value_of(sp.key) {
                    Some(v) => v,
                    None => continue, // veld stond niet op het formulier
                },
            };
            settings::set(&st.pool, sp.key, &raw);
        }
    }
    Redirect::to("/admin/settings?saved=1").into_response()
}

#[derive(Deserialize)]
struct WeightSet {
    amount: i64,
    weight: String,
}

#[derive(Deserialize)]
struct WeightDelete {
    amount: i64,
}

/// Voegt een uitkomst toe óf wijzigt er een (`amount` is de sleutel). Een gewicht
/// van 0 of minder zou de rij stil onbereikbaar maken; dat weigeren we, want
/// "weg" is wat de ✕-knop doet.
async fn admin_settings_weight_set(
    State(st): State<AppState>,
    headers: HeaderMap,
    Form(f): Form<WeightSet>,
) -> Response {
    if require_admin(&st, &headers).is_some() {
        if let Ok(w) = f.weight.trim().replace(',', ".").parse::<f64>() {
            if w > 0.0 && w.is_finite() && f.amount >= 0 {
                db::coin_weight_set(&st.pool, f.amount, w);
            }
        }
    }
    Redirect::to("/admin/settings?saved=1").into_response()
}

async fn admin_settings_weight_delete(
    State(st): State<AppState>,
    headers: HeaderMap,
    Form(f): Form<WeightDelete>,
) -> Response {
    if require_admin(&st, &headers).is_some() {
        db::coin_weight_delete(&st.pool, f.amount);
    }
    Redirect::to("/admin/settings?saved=1").into_response()
}

#[derive(Deserialize)]
struct TierAdd {
    weight: String,
    lo: i64,
    hi: i64,
}

#[derive(Deserialize)]
struct TierUpdate {
    id: i64,
    weight: String,
    lo: i64,
    hi: i64,
}

#[derive(Deserialize)]
struct TierDelete {
    id: i64,
}

/// Valideer een tier-invoer: gewicht > 0 en een bereik dat niet omgekeerd staat.
/// Bij twijfel wint de kleinste als ondergrens, zodat de trekking nooit paniekt.
fn tier_fields(weight: &str, lo: i64, hi: i64) -> Option<(f64, i64, i64)> {
    let w = weight.trim().replace(',', ".").parse::<f64>().ok()?;
    if !w.is_finite() || w <= 0.0 || lo < 0 || hi < 0 {
        return None;
    }
    Some((w, lo.min(hi), lo.max(hi)))
}

async fn admin_settings_tier_add(
    State(st): State<AppState>,
    headers: HeaderMap,
    Form(f): Form<TierAdd>,
) -> Response {
    if require_admin(&st, &headers).is_some() {
        if let Some((w, lo, hi)) = tier_fields(&f.weight, f.lo, f.hi) {
            db::chest_tier_add(&st.pool, w, lo, hi);
        }
    }
    Redirect::to("/admin/settings?saved=1").into_response()
}

async fn admin_settings_tier_update(
    State(st): State<AppState>,
    headers: HeaderMap,
    Form(f): Form<TierUpdate>,
) -> Response {
    if require_admin(&st, &headers).is_some() {
        if let Some((w, lo, hi)) = tier_fields(&f.weight, f.lo, f.hi) {
            db::chest_tier_update(&st.pool, f.id, w, lo, hi);
        }
    }
    Redirect::to("/admin/settings?saved=1").into_response()
}

async fn admin_settings_tier_delete(
    State(st): State<AppState>,
    headers: HeaderMap,
    Form(f): Form<TierDelete>,
) -> Response {
    if require_admin(&st, &headers).is_some() {
        db::chest_tier_delete(&st.pool, f.id);
    }
    Redirect::to("/admin/settings?saved=1").into_response()
}

#[derive(Deserialize)]
struct ShelfAdd {
    title: String,
}
#[derive(Deserialize)]
struct ShelfRename {
    id: i64,
    title: String,
}
#[derive(Deserialize)]
struct IdForm {
    id: i64,
}
#[derive(Deserialize)]
struct ItemAdd {
    zone: String,
    #[serde(default)]
    shelf_id: Option<i64>,
}
#[derive(Deserialize)]
struct ItemMove {
    id: i64,
    dir: i64,
}
#[derive(Deserialize)]
struct ItemShelf {
    id: i64,
    shelf_id: i64,
}
#[derive(Deserialize)]
struct SavedQuery {
    #[serde(default)]
    saved: Option<i64>,
}
#[derive(Deserialize)]
struct ItemUpdate {
    id: i64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    price: i64,
    #[serde(default)]
    role_id: String,
    #[serde(default)]
    duration_min: i64,
    /// Checkbox: aangevinkt ⇒ aanwezig in de POST, anders helemaal afwezig (zo werkt een
    /// HTML-checkbox). Aanwezigheid = uitverkocht.
    #[serde(default)]
    sold_out: Option<String>,
    #[serde(default)]
    category: String,
    #[serde(default)]
    description: String,
}

async fn admin_shelf_add(
    State(st): State<AppState>,
    headers: HeaderMap,
    Form(f): Form<ShelfAdd>,
) -> Response {
    if require_admin(&st, &headers).is_some() {
        let t = f.title.trim();
        if !t.is_empty() {
            db::add_shelf(&st.pool, t);
        }
    }
    Redirect::to("/admin/market").into_response()
}

async fn admin_shelf_rename(
    State(st): State<AppState>,
    headers: HeaderMap,
    Form(f): Form<ShelfRename>,
) -> Response {
    if require_admin(&st, &headers).is_some() {
        let t = f.title.trim();
        if !t.is_empty() {
            db::rename_shelf(&st.pool, f.id, t);
        }
    }
    Redirect::to("/admin/market").into_response()
}

async fn admin_shelf_delete(
    State(st): State<AppState>,
    headers: HeaderMap,
    Form(f): Form<IdForm>,
) -> Response {
    if require_admin(&st, &headers).is_some() {
        db::delete_shelf(&st.pool, f.id);
    }
    Redirect::to("/admin/market").into_response()
}

async fn admin_item_add(
    State(st): State<AppState>,
    headers: HeaderMap,
    Form(f): Form<ItemAdd>,
) -> Response {
    if let Some((admin_uid, admin)) = require_admin(&st, &headers) {
        let zone = if f.zone == "lucky" { "lucky" } else { "shelf" };
        let shelf_id = if zone == "shelf" { f.shelf_id } else { None };
        let id = db::add_item(&st.pool, zone, shelf_id);
        // Nog een leeg slot: naam/prijs volgen bij de eerste item_update. We loggen hier enkel
        // dát er een slot bijkwam, zodat de reeks item_update-regels erna een begin heeft.
        db::log_event(
            &st.pool,
            now_secs(),
            &db::LogEntry::new("admin", "item_add")
                .actor(&admin_uid, &admin)
                .reference(id as u64)
                .detail(format!("new {zone} slot · by {admin}")),
        );
    }
    Redirect::to("/admin/market").into_response()
}

async fn admin_item_update(
    State(st): State<AppState>,
    headers: HeaderMap,
    Form(f): Form<ItemUpdate>,
) -> Response {
    if let Some((admin_uid, admin)) = require_admin(&st, &headers) {
        // Vóór de schrijf lezen: enkel zo kunnen we "prijs 1000 → 1200" loggen. Zonder dat
        // spoor is achteraf niet meer te achterhalen wat een item ooit kostte, en dus ook
        // niet waarom een oude aankoop/refund een bepaald bedrag had.
        let before = db::get_item(&st.pool, f.id);
        let price = f.price.max(0);
        let name = f.name.trim();
        // `duration == 0` betekent "permanente pas". Een dagpas op 0 minuten zetten zou
        // hem dus stil in een permanente pas veranderen — nooit de bedoeling van dat
        // veldje. Een item dat al een dagpas is, blijft daarom minstens 1 minuut.
        let mut duration = f.duration_min.max(0) * 60;
        if let Some(b) = &before {
            if b.category == "boost" && b.duration > 0 {
                duration = duration.max(60);
            }
        }
        let sold_out = f.sold_out.is_some();
        db::update_item(
            &st.pool,
            f.id,
            name,
            price,
            f.role_id.trim(),
            duration,
            f.category.trim(),
            f.description.trim(),
            sold_out,
        );
        // Enkel de velden die écht veranderden in het logboek; niets gewijzigd = geen regel.
        if let Some(b) = before {
            let mut changes: Vec<String> = Vec::new();
            if b.price != price {
                changes.push(format!("price {} → {}", b.price, price));
            }
            if b.name != name {
                changes.push(format!("name '{}' → '{}'", b.name, name));
            }
            // Duur stuurt sinds 2026-07-15 de échte pas-lengte → wijzigingen horen in de log.
            if b.duration != duration {
                changes.push(format!("duration {} min → {} min", b.duration / 60, duration / 60));
            }
            if b.sold_out != sold_out {
                changes.push(
                    if sold_out { "→ out of stock" } else { "→ back in stock" }.to_string(),
                );
            }
            if !changes.is_empty() {
                db::log_event(
                    &st.pool,
                    now_secs(),
                    &db::LogEntry::new("admin", "item_update")
                        .actor(&admin_uid, &admin)
                        .reference(f.id as u64)
                        .amount(price)
                        .detail(format!("{} · {} · by {admin}", b.name, changes.join(" · "))),
                );
            }
        }
        return Redirect::to(&format!("/admin/market?saved={}", f.id)).into_response();
    }
    Redirect::to("/admin/market").into_response()
}

async fn admin_item_delete(
    State(st): State<AppState>,
    headers: HeaderMap,
    Form(f): Form<IdForm>,
) -> Response {
    if let Some((admin_uid, admin)) = require_admin(&st, &headers) {
        // Naam/prijs vastleggen vóór het wissen — daarna is het item weg en kan de logregel
        // niet meer zeggen wát er verdween.
        let gone = db::get_item(&st.pool, f.id);
        db::delete_item(&st.pool, f.id);
        if let Some(it) = gone {
            db::log_event(
                &st.pool,
                now_secs(),
                &db::LogEntry::new("admin", "item_delete")
                    .actor(&admin_uid, &admin)
                    .reference(f.id as u64)
                    .amount(it.price)
                    .detail(format!("{} (was {} coins) · by {admin}", it.name, it.price)),
            );
        }
    }
    Redirect::to("/admin/market").into_response()
}

/// Item één plaats naar links/rechts binnen z'n zone/schap verschuiven.
async fn admin_item_move(
    State(st): State<AppState>,
    headers: HeaderMap,
    Form(f): Form<ItemMove>,
) -> Response {
    if require_admin(&st, &headers).is_some() {
        db::move_item(&st.pool, f.id, f.dir);
    }
    Redirect::to("/admin/market").into_response()
}

/// Schap-item naar een ander schap verplaatsen.
async fn admin_item_shelf(
    State(st): State<AppState>,
    headers: HeaderMap,
    Form(f): Form<ItemShelf>,
) -> Response {
    if require_admin(&st, &headers).is_some() {
        db::set_item_shelf(&st.pool, f.id, f.shelf_id);
    }
    Redirect::to(&format!("/admin/market?saved={}", f.id)).into_response()
}

#[derive(Deserialize)]
struct StockForm {
    id: i64,
    #[serde(default)]
    add: i64,
    /// Aanwezig als er op de ∞-knop geduwd is: voorraad niet meer tellen.
    #[serde(default)]
    unlimited: Option<String>,
}

/// Voorraad aanvullen: "Add stock 3" telt er drie bíj. Of ∞ = niet meer tellen.
async fn admin_item_stock(
    State(st): State<AppState>,
    headers: HeaderMap,
    Form(f): Form<StockForm>,
) -> Response {
    if let Some((admin_uid, admin)) = require_admin(&st, &headers) {
        let naam = db::get_item(&st.pool, f.id).map(|i| i.name).unwrap_or_default();
        let detail = if f.unlimited.is_some() {
            db::set_stock_unlimited(&st.pool, f.id);
            format!("{naam} · stock → unlimited · by {admin}")
        } else if f.add != 0 {
            let nieuw = db::add_stock(&st.pool, f.id, f.add);
            format!("{naam} · stock {:+} → {nieuw} · by {admin}", f.add)
        } else {
            return Redirect::to("/admin/market").into_response();
        };
        db::log_event(
            &st.pool,
            now_secs(),
            &db::LogEntry::new("admin", "stock")
                .actor(&admin_uid, &admin)
                .reference(f.id as u64)
                .detail(detail),
        );
    }
    Redirect::to(&format!("/admin/market?saved={}", f.id)).into_response()
}

/// Compacte resterende-tijd voor de accounts-tabel: "2d 3h", "5h 23m", "42m" of "< 1m".
fn fmt_dur(secs: i64) -> String {
    if secs <= 0 {
        return "verlopen".to_string();
    }
    let (d, h, m) = (secs / 86400, (secs % 86400) / 3600, (secs % 3600) / 60);
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else if m > 0 {
        format!("{m}m")
    } else {
        "< 1m".to_string()
    }
}

/// Manage → Accounts: alle leden die ooit iets kochten, met hun pas-status.
/// Kolommen (voorlopig): lid, dagpas actief (+ resterende tijd), permanente pas.
/// Later uit te breiden met meer info per account.
async fn admin_accounts(State(st): State<AppState>, headers: HeaderMap) -> Response {
    let Some((_uid, name)) = require_admin(&st, &headers) else {
        return Redirect::to("/").into_response();
    };
    let accounts = db::list_accounts(&st.pool, now_secs());
    let rows: String = accounts
        .iter()
        .map(|a| {
            let member = if a.hytale_name.is_empty() {
                esc(&a.username)
            } else {
                format!("{} <span class=\"hint\">({})</span>", esc(&a.username), esc(&a.hytale_name))
            };
            let daypass = match a.day_pass_secs_left {
                Some(secs) => format!(
                    "<span class=\"yes\">Ja</span> <span class=\"hint\">— {} resterend</span>",
                    fmt_dur(secs)
                ),
                None => "<span class=\"no\">Nee</span>".to_string(),
            };
            let perma = if a.perma {
                "<span class=\"yes\">Ja</span>"
            } else {
                "<span class=\"no\">Nee</span>"
            };
            // `data-uid` alvast meegeven: haakje voor de latere extra info / per-account acties.
            format!(
                "<tr data-uid=\"{uid}\"><td>{member}</td><td>{daypass}</td><td>{perma}</td></tr>",
                uid = esc(&a.user_id),
            )
        })
        .collect();
    let table = if accounts.is_empty() {
        "<p class=\"muted\">Nobody has bought anything yet.</p>".to_string()
    } else {
        format!(
            "<table class=\"ctable\"><thead><tr>\
               <th>Lid</th><th>Dagpas actief</th><th>Permanente pas</th>\
             </tr></thead><tbody>{rows}</tbody></table>"
        )
    };
    let body = format!(
        "{}<div class=\"k\" style=\"margin:.2rem 0 .6rem\">Accounts</div>{table}",
        admin_subtabs("accounts"),
    );
    Html(shell(
        "Accounts — Meadow Market",
        &chrome(&name, "admin", true, ""),
        true,
        &body,
    ))
    .into_response()
}

/// Manage → Inactives: alle gevolgde leden, **aflopend op afwezigheid** (langst inactief
/// bovenaan). Voorbereiding op de latere "verdeel-kist": een lid dat ~een jaar niets deed
/// wordt opgegeven, waarna een speciale 24u-chest zijn coins onder de deelnemers verdeelt
/// (mechaniek nog uit te werken).
///
/// ⚠️ De afwezigheids-teller wordt **vooruit** opgebouwd vanaf de uitrol van deze feature:
/// Discord levert geen retro "laatst getypt", dus iedereen startte op 0 dagen. Pas na een
/// echt jaar zonder message/reactie haalt iemand "365 dagen".
async fn admin_inactives(State(st): State<AppState>, headers: HeaderMap) -> Response {
    let Some((_uid, name)) = require_admin(&st, &headers) else {
        return Redirect::to("/").into_response();
    };
    let now = now_secs();
    let members = db::list_inactives(&st.pool);
    let rows: String = members
        .iter()
        .map(|m| {
            let elapsed = (now - m.last_seen).max(0.0) as i64;
            let days = elapsed / 86400;
            let member = if m.name.is_empty() {
                format!("<span class=\"hint\">({})</span>", esc(&m.user_id))
            } else {
                esc(&m.name)
            };
            // ≥1 jaar inactief → markeren (kandidaat voor de verdeel-kist).
            let flag = if days >= 365 {
                " <span class=\"yes\">⚑ ≥1 jaar</span>"
            } else {
                ""
            };
            format!(
                "<tr data-uid=\"{uid}\"><td>{member}{flag}</td>\
                 <td>{days}</td><td><span class=\"hint\">{ago} geleden</span></td>\
                 <td>{MC} {coins}</td></tr>",
                uid = esc(&m.user_id),
                ago = fmt_dur(elapsed),
                coins = dots(m.coins),
            )
        })
        .collect();
    let table = if members.is_empty() {
        "<p class=\"muted\">Nog geen activiteit gevolgd — de klok start zodra de bot met deze \
         versie draait en de leden inleest.</p>"
            .to_string()
    } else {
        format!(
            "<table class=\"ctable\"><thead><tr>\
               <th>Lid</th><th>Dagen inactief</th><th>Laatst actief</th><th>Saldo</th>\
             </tr></thead><tbody>{rows}</tbody></table>"
        )
    };
    let note = "<p class=\"muted\" style=\"margin:.2rem 0 .8rem\">\
        Afwezigheid = tijd sinds het laatste bericht of de laatste reactie in de prod-server. \
        De teller wordt <b>vooruit</b> opgebouwd vanaf de uitrol (Discord kent geen retro \
        “laatst getypt”), dus iedereen startte op 0 dagen. Kandidaten voor de \
        verdeel-kist (≥ 1 jaar) worden gemarkeerd; de kist-mechaniek zelf komt later.</p>";
    let body = format!(
        "{}<div class=\"k\" style=\"margin:.2rem 0 .6rem\">Inactives</div>{note}{table}",
        admin_subtabs("inactives"),
    );
    Html(shell(
        "Inactives — Meadow Market",
        &chrome(&name, "admin", true, ""),
        true,
        &body,
    ))
    .into_response()
}

/// Geüploade afbeelding van een item wissen (terug naar kleur-thumb).
async fn admin_item_image_clear(
    State(st): State<AppState>,
    headers: HeaderMap,
    Form(f): Form<IdForm>,
) -> Response {
    if require_admin(&st, &headers).is_some() {
        db::clear_item_image(&st.pool, f.id);
        return Redirect::to(&format!("/admin/market?saved={}", f.id)).into_response();
    }
    Redirect::to("/admin/market").into_response()
}

/// Afbeelding uploaden voor een item (multipart). Bewaart op schijf + zet de naam.
async fn admin_item_image(
    State(st): State<AppState>,
    headers: HeaderMap,
    mut mp: Multipart,
) -> Response {
    if require_admin(&st, &headers).is_none() {
        return Redirect::to("/").into_response();
    }
    let mut id: Option<i64> = None;
    let mut slot = String::new();
    let mut file: Option<(String, Vec<u8>)> = None;
    while let Ok(Some(field)) = mp.next_field().await {
        let field_name = field.name().map(|s| s.to_string());
        match field_name.as_deref() {
            Some("id") => {
                id = field.text().await.ok().and_then(|s| s.trim().parse().ok());
            }
            Some("slot") => {
                slot = field.text().await.unwrap_or_default();
            }
            Some("file") => {
                let ct = field.content_type().map(|s| s.to_string());
                let orig = field.file_name().map(|s| s.to_string());
                if let Ok(bytes) = field.bytes().await {
                    if !bytes.is_empty() {
                        file = Some((ext_from(ct.as_deref(), orig.as_deref()), bytes.to_vec()));
                    }
                }
            }
            _ => {}
        }
    }
    if let (Some(id), Some((ext, bytes))) = (id, file) {
        // slot "2" = de tweede afbeelding; alle andere waarden = de hoofdafbeelding.
        let is2 = slot.trim() == "2";
        let filename = if is2 {
            format!("item_{id}_2.{ext}")
        } else {
            format!("item_{id}.{ext}")
        };
        if std::fs::write(format!("{UPLOAD_DIR}/{filename}"), &bytes).is_ok() {
            if is2 {
                db::set_item_image2(&st.pool, id, &filename);
            } else {
                db::set_item_image(&st.pool, id, &filename);
            }
        }
    }
    Redirect::to(&format!("/admin/market?saved={}", id.unwrap_or(0))).into_response()
}

/// De tweede afbeelding van een item wissen.
async fn admin_item_image2_clear(
    State(st): State<AppState>,
    headers: HeaderMap,
    Form(f): Form<IdForm>,
) -> Response {
    if require_admin(&st, &headers).is_some() {
        db::clear_item_image2(&st.pool, f.id);
        return Redirect::to(&format!("/admin/market?saved={}", f.id)).into_response();
    }
    Redirect::to("/admin/market").into_response()
}

/// Admin: sync de gem-kleuren opnieuw uit de Discord-rollen (handig na een kleurwijziging
/// in Discord). Leest uit de omgeving-guild (dev in test, prod in prod).
async fn admin_sync_gem_colors(State(st): State<AppState>, headers: HeaderMap) -> Response {
    if require_admin(&st, &headers).is_some() {
        let cg = color_guild(&st.cfg);
        let _ = match st.dc.list_roles(&cg).await {
            Ok(roles) => db::sync_gem_colors(&st.pool, &roles),
            Err(_) => 0,
        };
        return Redirect::to("/admin/market").into_response();
    }
    Redirect::to("/").into_response()
}

/// Admin-testhulp: draai je eigen verzamel-aankopen terug (coins terug + items weer
/// ontgrendelbaar + naamkleur gereset). Zo kan je de shop/gems testen zonder blijvende
/// coin-gevolgen. Raakt passen niet.
async fn admin_reset_collection(State(st): State<AppState>, headers: HeaderMap) -> Response {
    if let Some((uid, _name)) = require_admin(&st, &headers) {
        // Eerst de eventueel geëquipte gem-rol op Discord intrekken.
        let prev = db::get_equipped_gem(&st.pool, &uid);
        if !prev.is_empty() {
            if let Ok(Some(rid)) = st.dc.role_id_by_name(&prev).await {
                let _ = st.dc.set_role(&uid, &rid, false).await;
            }
        }
        let refunded = db::reset_test_collection(&st.pool, &uid);
        // Logboek: test-reset (coins terug + collectie/passen/whitelist gewist).
        db::log_event(
            &st.pool,
            now_secs(),
            &db::LogEntry::new("admin", "reset_collection")
                .actor(&uid, &_name)
                .amount(refunded)
                .detail("test reset — collection + passes/whitelist cleared"),
        );
        let msg = format!(
            "🧪 Test reset — refunded {refunded} coins, cleared your collection and removed passes/whitelist."
        );
        return Redirect::to(&format!("/?tab=gems&msg={}", pct(&msg))).into_response();
    }
    Redirect::to("/").into_response()
}

/// De ingebakken 24h-pas ticket-afbeelding serveren.
async fn serve_ticket() -> Response {
    (
        [(axum::http::header::CONTENT_TYPE, "image/png")],
        TICKET_IMG,
    )
        .into_response()
}

/// De ingebakken treasure-chest-afbeelding serveren (voor de chest-embed).
async fn serve_chest() -> Response {
    (
        [(axum::http::header::CONTENT_TYPE, "image/png")],
        CHEST_PNG,
    )
        .into_response()
}

/// De ingebakken ronde Hytale-knop (draagt de pas-timer op de Coins-tab).
async fn serve_hytale_pass() -> Response {
    (
        [(axum::http::header::CONTENT_TYPE, "image/png")],
        HYTALE_PASS_PNG,
    )
        .into_response()
}

/// De ingebakken "Spicy Sale"-display-font serveren (voor de Basic Gems-titel).
async fn serve_spicy_sale_font() -> Response {
    (
        [
            (axum::http::header::CONTENT_TYPE, "font/ttf"),
            (axum::http::header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
        ],
        SPICY_SALE_TTF,
    )
        .into_response()
}

/// Publieke info-pagina: hoe je coins verdient (accordion, klik = uit/invouwen).
async fn info_page() -> Response {
    let items = [
        (
            "Chatting in the main channels",
            "Every message gives you 1 to 3 coins, earnable every 30 seconds.",
        ),
        (
            "Leveling up",
            "You get a level-up reward for each level you gain!",
        ),
        (
            "Gain Fortuna's Favor by being active in chat with multiple people at once!",
            "A special treasure chest can appear during active chat hours. You need multiple people to open these.",
        ),
        (
            "Checking in daily in the Meadow Market and building a streak.",
            "Every day you can check in to gain coins. The higher your streak, the higher your min and max amounts become.",
        ),
        (
            "(WIP) Registering your Birthday",
            "By registering your Birthday, you can claim a Birthday present!",
        ),
    ];
    let acc: String = items
        .into_iter()
        .map(|(t, b)| {
            format!(
                "<details class=\"acc\"><summary>{MC} {}</summary><p>{}</p></details>",
                esc(t),
                esc(b)
            )
        })
        .collect();
    let body = format!(
        "<h1>Earning Coins in the Magic Meadow</h1>\
         <p class=\"muted\">You can earn Meadowcoins in many different ways:</p>{acc}"
    );
    Html(shell("Info — Meadow Market", "", false, &body)).into_response()
}

/// Bewaarde afbeelding serveren vanuit de uploads-map (met naam-sanitatie).
async fn serve_upload(Path(name): Path<String>) -> Response {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return StatusCode::NOT_FOUND.into_response();
    }
    match std::fs::read(format!("{UPLOAD_DIR}/{name}")) {
        Ok(bytes) => (
            [
                (axum::http::header::CONTENT_TYPE, content_type_for(&name)),
                // Lang cachen; de render hangt er een ?v=<mtime> aan, dus een vervangen
                // afbeelding krijgt automatisch een nieuwe URL.
                (axum::http::header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
            ],
            bytes,
        )
            .into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Kies een veilige extensie op basis van content-type of bestandsnaam.
fn ext_from(ct: Option<&str>, fname: Option<&str>) -> String {
    let by_name = fname
        .and_then(|f| f.rsplit('.').next())
        .map(|e| e.to_ascii_lowercase())
        .filter(|e| matches!(e.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp"));
    if let Some(e) = by_name {
        return if e == "jpeg" { "jpg".into() } else { e };
    }
    match ct {
        Some("image/png") => "png",
        Some("image/jpeg") => "jpg",
        Some("image/gif") => "gif",
        Some("image/webp") => "webp",
        _ => "png",
    }
    .to_string()
}

fn content_type_for(name: &str) -> &'static str {
    match name.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
}

// ---------------------------------------------------------------------------
// Dry-run: gem-Use haalt élke andere kleur-gem-rol weg (self-healing swap).
// Test de pure selectie other_gem_role_ids — de kern van de Ruby-blijft-staan-fix.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod gem_swap_dryrun {
    use super::other_gem_role_ids;
    use std::collections::HashSet;

    fn roles() -> Vec<(String, String)> {
        // (rol-id, naam) — mix van gem-rollen en niet-gem-rollen.
        [
            ("10", "Ruby"),
            ("11", "Lapis Lazuli"),
            ("12", "Sapphire"),
            ("99", "Flowerborn"), // niet-gem-rol: mag NOOIT geraakt worden
            ("98", "Hytaler"),    // idem
        ]
        .iter()
        .map(|(i, n)| (i.to_string(), n.to_string()))
        .collect()
    }

    fn gem_names() -> Vec<String> {
        ["Ruby", "Lapis Lazuli", "Sapphire", "Amber", "Topaz"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    #[test]
    fn swap_strips_only_the_other_gem_role() {
        // Lid draagt Ruby (oude kleur) + Flowerborn; equipt nu Lapis Lazuli.
        let held: HashSet<String> = ["10", "99"].iter().map(|s| s.to_string()).collect();
        let strip = other_gem_role_ids(&roles(), &held, &gem_names(), "Lapis Lazuli");
        assert_eq!(strip, vec!["10".to_string()], "enkel de Ruby-rol wordt weggehaald");
    }

    #[test]
    fn strips_all_stale_gem_roles_but_keeps_the_new_one() {
        // Vervuilde staat: lid draagt Ruby én Sapphire (twee oude kleuren) + Flowerborn.
        // Equipt Lapis → beide oude gem-rollen weg, Flowerborn blijft, Lapis niet in de lijst.
        let held: HashSet<String> =
            ["10", "12", "99"].iter().map(|s| s.to_string()).collect();
        let mut strip = other_gem_role_ids(&roles(), &held, &gem_names(), "Lapis Lazuli");
        strip.sort();
        assert_eq!(strip, vec!["10".to_string(), "12".to_string()]);
        assert!(!strip.contains(&"99".to_string()), "niet-gem-rol Flowerborn blijft");
    }

    #[test]
    fn re_equipping_same_gem_strips_nothing() {
        // Lid draagt al Lapis en equipt Lapis opnieuw → niets weghalen.
        let held: HashSet<String> = ["11"].iter().map(|s| s.to_string()).collect();
        let strip = other_gem_role_ids(&roles(), &held, &gem_names(), "Lapis Lazuli");
        assert!(strip.is_empty(), "dezelfde gem opnieuw = geen enkele revoke");
    }

    #[test]
    fn ignores_gem_roles_the_member_does_not_hold() {
        // Case-ongevoelig, en rollen die het lid niet draagt blijven buiten schot.
        let held: HashSet<String> = ["10"].iter().map(|s| s.to_string()).collect();
        let strip = other_gem_role_ids(&roles(), &held, &gem_names(), "sapphire");
        assert_eq!(strip, vec!["10".to_string()], "Ruby weg; Sapphire niet gedragen → niet geraakt");
    }
}
