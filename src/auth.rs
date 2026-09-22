//! Авторизация YouTube Music.
//!
//! Источники учётных данных, по убыванию приоритета:
//! 1. Cookie из `~/.config/youmz/cookie` (права 600);
//! 2. Сессия десктопного приложения YouTube Music (Electron) —
//!    cookie лежат открытым текстом в Chromium-овой sqlite-базе;
//! 3. Сохранённый OAuth-токен (`~/.config/youmz/token.json`);
//! 4. OAuth 2.0 device flow — пользователь входит по ссылке
//!    `https://www.google.com/device?user_code=XXXX`, а программа сама
//!    достаёт токен и конвертирует его в полноценную cookie-сессию
//!    через `sw.js_data` (приём из yt-dlp).
//!
//! Всё это нужно, чтобы играл персональный микс «Мой джем» (RDMM),
//! требующий авторизованного пользователя.

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, COOKIE, ORIGIN, REFERER, SET_COOKIE};
use sha1::{Digest, Sha1};
use serde::{Deserialize, Serialize};

const APP_DIR: &str = "youmz";
pub const COOKIE_FILE: &str = "cookie";
pub const TOKEN_FILE: &str = "token.json";
pub const CLIENT_ID_FILE: &str = "client_id";
pub const CLIENT_SECRET_FILE: &str = "client_secret";

/// Публичный OAuth-клиент из документации ytmusicapi. Может быть отозван —
/// тогда его нужно заменить на свой (см. README, раздел «Вход»):
/// `~/.config/youmz/client_id` + `client_secret` или YOUMZ_CLIENT_ID/SECRET.
pub const DEFAULT_CLIENT_ID: &str =
    "861556708454-d6dlm3lh05idd8bepek2kcl4m04uu45k.apps.googleusercontent.com";
pub const DEFAULT_CLIENT_SECRET: &str = "SboVhoG9s0rNafixCSGGKXAT";

const DEVICE_CODE_URL: &str = "https://www.youtube.com/o/oauth2/device/code";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const OAUTH_SCOPE: &str = "https://www.googleapis.com/auth/youtube";
/// С Bearer-токеном отдаёт Set-Cookie с настоящей сессией аккаунта
const SW_JS_DATA_URL: &str = "https://www.youtube.com/sw.js_data";
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:88.0) Gecko/2010011 \
                  Firefox/88.0 Cobalt/Version";
const ORIGIN_URL: &str = "https://music.youtube.com";

/// Авторизация, которую InnerTube-клиент использует в запросах
#[allow(dead_code)]
#[derive(Clone)]
pub enum Auth {
    /// Cookie-строка браузера: "SAPISID=...; SID=..."
    Cookie(String),
    /// OAuth access token, если обмен на cookie не удался
    Bearer(String),
}

impl Auth {
    /// Применить авторизацию к заголовкам запроса к InnerTube
    #[allow(dead_code)]
    pub fn apply(&self, headers: &mut HeaderMap) {
        match self {
            Auth::Cookie(cookie) => {
                headers.insert(COOKIE, HeaderValue::from_str(cookie).expect("некорректный cookie"));
                // SAPISIDHASH — обязательный заголовок: без него InnerTube
                // отдаёт анонимный контент вместо персонального «Мой джем».
                if let Some(sapisid) = extract_sapisid(cookie) {
                    let ts = now_secs();
                    let mut sha = Sha1::new();
                    sha.update(ts.to_string());
                    sha.update(" ");
                    sha.update(&sapisid);
                    sha.update(" ");
                    sha.update(ORIGIN_URL);
                    let hash = data_encoding::HEXLOWER.encode(&sha.finalize());
                    if let Ok(value) = HeaderValue::from_str(&format!("SAPISIDHASH {ts}_{hash}")) {
                        headers.insert(AUTHORIZATION, value);
                    }
                    // У аккаунта один канал — индекс 0
                    headers.insert("X-Goog-AuthUser", HeaderValue::from_static("0"));
                }
            }
            Auth::Bearer(token) => {
                headers.insert(
                    AUTHORIZATION,
                    HeaderValue::from_str(&format!("Bearer {token}"))
                        .expect("некорректный токен"),
                );
            }
        }
        headers.insert(ORIGIN, HeaderValue::from_static(ORIGIN_URL));
        headers.insert(REFERER, HeaderValue::from_static("https://music.youtube.com/"));
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct TokenFile {
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// Unix-секунды, когда протухнет access_token
    pub expires_at: u64,
    pub client_id: String,
    pub client_secret: String,
    /// Кэшированная cookie-сессия, полученная из токена
    pub cookie: Option<String>,
}

/// Вытащить значение SAPISID из cookie-строки
fn extract_sapisid(cookie: &str) -> Option<String> {
    cookie
        .split(';')
        .find_map(|c| c.trim().strip_prefix("SAPISID=").map(|v| v.trim().to_string()))
        .filter(|s| !s.is_empty())
}

/// Каталог конфигурации: $XDG_CONFIG_HOME/youmz или ~/.config/youmz
pub fn config_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return PathBuf::from(xdg).join(APP_DIR);
        }
    }
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()))
        .join(".config")
        .join(APP_DIR)
}

/// Каталог для кэширования обложек: $XDG_CACHE_HOME/youmz/covers или ~/.cache/youmz/covers
pub fn covers_dir() -> PathBuf {
    let base = if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        if !xdg.is_empty() {
            PathBuf::from(xdg).join(APP_DIR)
        } else {
            PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()))
                .join(".cache")
                .join(APP_DIR)
        }
    } else {
        PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()))
            .join(".cache")
            .join(APP_DIR)
    };
    base.join("covers")
}

fn read_file(name: &str, env_var: &str) -> Option<String> {
    let path = config_dir().join(name);
    if let Ok(content) = fs::read_to_string(&path) {
        let trimmed = content.trim().to_string();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }
    std::env::var(env_var)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn write_file(name: &str, content: &str) -> Result<(), String> {
    let dir = config_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("Не удалось создать {}: {e}", dir.display()))?;
    let path = dir.join(name);
    let mut file =
        fs::File::create(&path).map_err(|e| format!("Не создать {}: {e}", path.display()))?;
    file.write_all(content.as_bytes())
        .map_err(|e| format!("Не удалось записать {}: {e}", path.display()))?;
    let mut perms = file.metadata().map_err(|e| e.to_string())?.permissions();
    perms.set_mode(0o600);
    fs::set_permissions(&path, perms).map_err(|e| e.to_string())?;
    Ok(())
}

/// Прочитать готовую cookie-сессию из конфига
pub fn load_cookie_file() -> Option<String> {
    read_file(COOKIE_FILE, "YOUMZ_COOKIE")
}

/// Преобразовать сохранённую cookie-строку в файл формата Netscape cookies.txt
/// (требуется для yt-dlp и других инструментов).
pub fn ensure_netscape_cookie_file() -> Option<PathBuf> {
    let cookie_str = load_cookie_file()?;
    let path = config_dir().join("cookies.txt");
    let mut out = String::from("# Netscape HTTP Cookie File\n");
    for part in cookie_str.split(';') {
        let part = part.trim();
        if let Some((k, v)) = part.split_once('=') {
            out.push_str(&format!(
                ".youtube.com\tTRUE\t/\tTRUE\t2147483647\t{}\t{}\n",
                k.trim(),
                v.trim()
            ));
        }
    }
    if let Err(e) = fs::write(&path, &out) {
        log::warn!("Не удалось записать cookies.txt: {e}");
        return None;
    }
    if let Ok(meta) = fs::metadata(&path) {
        let mut perms = meta.permissions();
        perms.set_mode(0o600);
        let _ = fs::set_permissions(&path, perms);
    }
    Some(path)
}

/// Сохранить cookie-сессию (права 600)
pub fn save_cookie(cookie: &str) -> Result<(), String> {
    write_file(COOKIE_FILE, cookie)?;
    ensure_netscape_cookie_file();
    Ok(())
}

/// SAPISIDHASH для произвольного origin: нужен для запросов к www.youtube.com
/// (player-эндпоинт), тогда как основной клиент ходит на music.youtube.com.
#[allow(dead_code)]
pub fn sapisidhash(cookie: &str, origin: &str) -> Option<String> {
    let sapisid = extract_sapisid(cookie)?;
    let ts = now_secs();
    let mut sha = Sha1::new();
    sha.update(ts.to_string());
    sha.update(" ");
    sha.update(&sapisid);
    sha.update(" ");
    sha.update(origin);
    Some(format!("SAPISIDHASH {ts}_{}", data_encoding::HEXLOWER.encode(&sha.finalize())))
}

pub fn load_client_id() -> String {
    read_file(CLIENT_ID_FILE, "YOUMZ_CLIENT_ID").unwrap_or_else(|| DEFAULT_CLIENT_ID.to_string())
}

pub fn load_client_secret() -> String {
    read_file(CLIENT_SECRET_FILE, "YOUMZ_CLIENT_SECRET")
        .unwrap_or_else(|| DEFAULT_CLIENT_SECRET.to_string())
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn load_token_file() -> Option<TokenFile> {
    let path = config_dir().join(TOKEN_FILE);
    match fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str::<TokenFile>(&content).ok(),
        Err(_) => None,
    }
}

fn save_token_file(token: &TokenFile) -> Result<(), String> {
    write_file(TOKEN_FILE, &serde_json::to_string_pretty(token).map_err(|e| e.to_string())?)
}

/// HTTP-клиент с прокси (если задан) для OAuth и обмена токена
pub fn http_client(proxy: &Option<String>) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder().user_agent(UA).timeout(Duration::from_secs(30));
    if let Some(proxy_url) = proxy {
        builder = builder
            .proxy(reqwest::Proxy::all(proxy_url).map_err(|e| format!("Некорректный прокси: {e}"))?);
    }
    builder
        .build()
        .map_err(|e| format!("Не удалось создать HTTP-клиент: {e}"))
}

/// Проверить, что cookie — это реальный авторизованный аккаунт, а не
/// анонимный посетитель. Анонимам YouTube отдаёт не персональный «Мой джем»,
/// а общий региональный микс, поэтому это надо отличать явно.
pub async fn verify_session(http: &reqwest::Client, cookie: &str) -> bool {
    let body = serde_json::json!({
        "browseId": "FEmusic_history",
        "context": {
            "client": {
                "clientName": "WEB_REMIX",
                "clientVersion": "1.20241216.01.00",
                "hl": "ru", "gl": "RU"
            }
        }
    });

    // Тот же шаблон, что у music.youtube.com: без API-ключа, с SAPISIDHASH
    let url = "https://music.youtube.com/youtubei/v1/browse?prettyPrint=false";
    let mut headers = HeaderMap::new();
    headers.insert(COOKIE, HeaderValue::from_str(cookie).unwrap_or_else(|_| HeaderValue::from_static("")));
    if let Some(sapisid) = extract_sapisid(cookie) {
        let ts = now_secs();
        let mut sha = Sha1::new();
        sha.update(ts.to_string());
        sha.update(" ");
        sha.update(&sapisid);
        sha.update(" ");
        sha.update(ORIGIN_URL);
        let hash = data_encoding::HEXLOWER.encode(&sha.finalize());
        if let Ok(value) = HeaderValue::from_str(&format!("SAPISIDHASH {ts}_{hash}")) {
            headers.insert(AUTHORIZATION, value);
        }
        headers.insert("X-Goog-AuthUser", HeaderValue::from_static("0"));
    }
    headers.insert(ORIGIN, HeaderValue::from_static(ORIGIN_URL));
    headers.insert(REFERER, HeaderValue::from_static("https://music.youtube.com/"));
    let resp = match http
        .post(url)
        .headers(headers)
        .json(&body)
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => r,
        _ => return false,
    };

    let value: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(_) => return false,
    };

    let Some(params) = value
        .pointer("/responseContext/serviceTrackingParams")
        .and_then(|v| v.as_array())
    else {
        return false;
    };

    for group in params {
        let Some(inner) = group.get("params").and_then(|v| v.as_array()) else {
            continue;
        };
        for p in inner {
            if p.get("key").and_then(|v| v.as_str()) == Some("logged_in")
                && p.get("value").and_then(|v| v.as_str()) == Some("1")
            {
                return true;
            }
        }
    }
    // Запрос авторизован, если YouTube отдаёт реальную историю просмотров
    // ( анонимному посетелю отдаётся пустая страница с предложением войти)
    value.to_string().matches("\"videoId\"").count() > 0
}

fn warn_anonymous() {
    log::warn!("⚠  Сессия НЕ авторизована (анонимный посетитель): играет общий");
    log::warn!("   региональный микс, а не ваш «Мой джем».");
    log::warn!("   Вариант 1: войдите в аккаунт в десктопном приложении YouTube Music,");
    log::warn!("             удалите ~/.config/youmz/cookie и перезапустите youmz.");
    log::warn!("   Вариант 2: youmz login — вход по ссылке со своим OAuth-клиентом.");
    log::warn!("   Вариант 3: положите cookie браузера в ~/.config/youmz/cookie.");
}

/// Импорт сессии из установленного десктопного приложения YouTube Music.
/// У него cookie лежат в Chromium-овой sqlite-базе; на Linux без keyring
/// они не зашифрованы. Возвращает готовую cookie-строку.
pub fn import_desktop_session() -> Result<String, String> {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let candidates = [
        PathBuf::from(&home).join(".config").join("YouTube Music").join("Cookies"),
        PathBuf::from(&home).join(".config").join("youtube-music").join("Cookies"),
    ];

    let db_path = candidates
        .iter()
        .find(|p| p.exists())
        .ok_or_else(|| "Десктопное приложение YouTube Music не найдено".to_string())?;

    log::info!("Импорт сессии из {}", db_path.display());

    let conn =
        rusqlite::Connection::open(db_path).map_err(|e| format!("Не открыть базу cookie: {e}"))?;

    let mut stmt = conn
        .prepare("SELECT name, value FROM cookies WHERE host_key = '.youtube.com'")
        .map_err(|e| format!("Не прочитать cookie: {e}"))?;

    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        .map_err(|e| format!("Не выполнить запрос cookie: {e}"))?;

    let mut pairs = Vec::new();
    let mut encrypted = false;
    for row in rows {
        let (name, value) = row.map_err(|e| format!("Не прочитать строку: {e}"))?;
        if value.is_empty() {
            continue;
        }
        // Chromium шифрует cookie, если в системе есть keyring
        if value.starts_with("v10") || value.starts_with("v11") {
            encrypted = true;
            continue;
        }
        pairs.push(format!("{name}={value}"));
    }

    if encrypted {
        return Err(
            "Cookie десктопного приложения зашифрованы (keyring). \
             Используйте `youmz login` для входа по ссылке"
                .to_string(),
        );
    }

    if !pairs.iter().any(|p| p.starts_with("SAPISID=")) {
        return Err("В сессии нет SAPISID — похоже, в приложении не выполнен вход".to_string());
    }

    Ok(pairs.join("; "))
}

/// Собрать cookie из заголовков Set-Cookie ответа
fn set_cookie_pairs(resp: &reqwest::Response) -> Vec<String> {
    let mut pairs = Vec::new();
    for value in resp.headers().get_all(SET_COOKIE) {
        // формат: "name=value; Path=/; Secure; ..."
        let raw = value.to_str().unwrap_or("");
        let kv = raw.split(';').next().unwrap_or("").trim();
        if let Some(idx) = kv.find('=') {
            let name = kv[..idx].trim();
            let val = kv[idx + 1..].trim();
            if !name.is_empty() {
                pairs.push(format!("{name}={val}"));
            }
        }
    }
    pairs
}

/// Обменять OAuth access_token на настоящую cookie-сессию.
/// `GET /sw.js_data` с `Authorization: Bearer ...` отдаёт Set-Cookie.
pub async fn token_to_cookie(http: &reqwest::Client, access_token: &str) -> Result<String, String> {
    let mut pairs = fetch_sw_js_cookies(http, access_token).await?;

    if !pairs.iter().any(|p| p.starts_with("SAPISID=")) {
        // Иногда Google не отдаёт SAPISID с первого раза — повторяем
        tokio::time::sleep(Duration::from_secs(1)).await;
        pairs = fetch_sw_js_cookies(http, access_token).await?;
    }

    if pairs.is_empty() {
        return Err("sw.js_data не вернул cookie — токен невалиден".to_string());
    }

    Ok(pairs.join("; "))
}

async fn fetch_sw_js_cookies(http: &reqwest::Client, access_token: &str) -> Result<Vec<String>, String> {
    let resp = http
        .get(SW_JS_DATA_URL)
        .header(AUTHORIZATION, format!("Bearer {access_token}"))
        .send()
        .await
        .map_err(|e| format!("Ошибка запроса sw.js_data: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("sw.js_data ответил {}", resp.status()));
    }

    Ok(set_cookie_pairs(&resp))
}

#[derive(Debug, Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_url: String,
    expires_in: u64,
    interval: u64,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
    error: Option<String>,
    error_description: Option<String>,
}

/// OAuth 2.0 Device Flow: печатает ссылку, ждёт входа пользователя,
/// возвращает готовый токен. Единственный способ авторизации для
/// headless-сервиса, не требующий интерфейса и ручного копирования.
pub async fn device_flow(
    http: &reqwest::Client,
    client_id: &str,
    client_secret: &str,
) -> Result<TokenFile, String> {
    let resp = http
        .post(DEVICE_CODE_URL)
        .form(&[("client_id", client_id), ("scope", OAUTH_SCOPE)])
        .send()
        .await
        .map_err(|e| format!("Ошибка запроса device code: {e}"))?;

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        let msg = serde_json::from_str::<TokenResponse>(&text)
            .ok()
            .and_then(|t| t.error_description.or(t.error))
            .unwrap_or_else(|| text.chars().take(200).collect());
        if msg.contains("not found") || msg.contains("not_found") || status.as_u16() == 401 {
            return Err(format!(
                "Google отклонил OAuth-клиент ({msg}). Создайте свой клиент типа «TVs and \
                 Limited Input devices» (см. README, раздел «Вход») и положите его в \
                 ~/.config/youmz/client_id или задайте YOUMZ_CLIENT_ID."
            ));
        }
        return Err(format!("device/code ответил {status}: {msg}"));
    }

    let code: DeviceCodeResponse =
        serde_json::from_str(&text).map_err(|e| format!("Некорректный ответ device code: {e}"))?;

    let link = format!("{}?user_code={}", code.verification_url, code.user_code);
    println!();
    println!("═══════════════════════════════════════════════════════════");
    println!(" Первый запуск: войдите в аккаунт Google");
    println!("═══════════════════════════════════════════════════════════");
    println!(" Откройте ссылку:  {link}");
    println!(" или вручную:      {}   код: {}", code.verification_url, code.user_code);
    println!(" Программа сама достанет токен — можно закрыть браузер");
    println!("═══════════════════════════════════════════════════════════");
    println!();
    log::info!("Ожидание входа пользователя по ссылке {link}");

    let mut interval = code.interval.max(2);
    let deadline = now_secs() + code.expires_in;

    loop {
        if now_secs() > deadline {
            return Err("Время ожидания входа истекло. Запустите `youmz login` снова".to_string());
        }

        tokio::time::sleep(Duration::from_secs(interval)).await;

        let resp = http
            .post(TOKEN_URL)
            .form(&[
                ("client_id", client_id),
                ("client_secret", client_secret),
                ("device_code", code.device_code.as_str()),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ])
            .send()
            .await
            .map_err(|e| format!("Ошибка запроса токена: {e}"))?;

        let token: TokenResponse =
            resp.json().await.map_err(|e| format!("Некорректный ответ token: {e}"))?;

        match (token.access_token, token.error.as_deref()) {
            (Some(access_token), _) => {
                let expires_in = token.expires_in.unwrap_or(3600);
                let tf = TokenFile {
                    access_token,
                    refresh_token: token.refresh_token,
                    expires_at: now_secs() + expires_in,
                    client_id: client_id.to_string(),
                    client_secret: client_secret.to_string(),
                    cookie: None,
                };
                log::info!("Токен получен, действителен {}с", expires_in);
                return Ok(tf);
            }
            (None, Some("authorization_pending")) => continue,
            (None, Some("slow_down")) => {
                interval *= 2;
                continue;
            }
            (None, Some(err)) => {
                return Err(format!(
                    "Вход не удался: {err} ({})",
                    token.error_description.unwrap_or_default()
                ))
            }
            (None, None) => return Err("Token endpoint вернул пустой ответ".to_string()),
        }
    }
}

/// Обновить access_token по refresh_token
async fn refresh_token(http: &reqwest::Client, tf: &TokenFile) -> Result<TokenFile, String> {
    let refresh = tf
        .refresh_token
        .clone()
        .ok_or_else(|| "Нет refresh_token — нужен повторный вход".to_string())?;

    let resp = http
        .post(TOKEN_URL)
        .form(&[
            ("client_id", tf.client_id.as_str()),
            ("client_secret", tf.client_secret.as_str()),
            ("refresh_token", refresh.as_str()),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await
        .map_err(|e| format!("Ошибка обновления токена: {e}"))?;

    let token: TokenResponse =
        resp.json().await.map_err(|e| format!("Некорректный ответ: {e}"))?;

    match token.access_token {
        Some(access_token) => {
            let expires_in = token.expires_in.unwrap_or(3600);
            let refreshed = TokenFile {
                access_token,
                refresh_token: token.refresh_token.or(Some(refresh)),
                expires_at: now_secs() + expires_in,
                client_id: tf.client_id.clone(),
                client_secret: tf.client_secret.clone(),
                cookie: None,
            };
            log::info!("Токен обновлён, действителен {}с", expires_in);
            Ok(refreshed)
        }
        None => Err(format!(
            "Обновление не удалось: {} ({})",
            token.error.unwrap_or_default(),
            token.error_description.unwrap_or_default()
        )),
    }
}

/// Превратить сохранённый токен в работающую авторизацию:
/// при необходимости обновить, затем обменять на cookie.
async fn auth_from_token(http: &reqwest::Client, mut tf: TokenFile) -> Result<Auth, String> {
    if tf.expires_at <= now_secs() + 60 {
        log::info!("Токен протух, обновляем...");
        tf = refresh_token(http, &tf).await?;
    }

    if let Some(cookie) = &tf.cookie {
        if !cookie.is_empty() {
            return Ok(Auth::Cookie(cookie.clone()));
        }
    }

    match token_to_cookie(http, &tf.access_token).await {
        Ok(cookie) => {
            tf.cookie = Some(cookie.clone());
            let _ = save_token_file(&tf);
            Ok(Auth::Cookie(cookie))
        }
        Err(e) => {
            // yt-dlp приём не сработал — токен всё равно можно использовать как Bearer
            log::warn!("Не удалось обменять токен на cookie ({e}); использую Bearer");
            Ok(Auth::Bearer(tf.access_token))
        }
    }
}

/// Интерактивный вход по ссылке (device flow). Сохраняет токен и cookie.
pub async fn interactive_login(
    proxy: &Option<String>,
    client_id: &str,
    client_secret: &str,
) -> Result<Auth, String> {
    let http = http_client(proxy)?;
    let tf = device_flow(&http, client_id, client_secret).await?;

    let auth = match token_to_cookie(&http, &tf.access_token).await {
        Ok(cookie) => {
            let mut tf = tf.clone();
            tf.cookie = Some(cookie.clone());
            let _ = save_token_file(&tf);
            let _ = save_cookie(&cookie);
            log::info!("Cookie-сессия сохранена в ~/.config/youmz/cookie");
            Auth::Cookie(cookie)
        }
        Err(e) => {
            let _ = save_token_file(&tf);
            log::warn!("Обмен токена на cookie не удался ({e}); работаю через Bearer");
            Auth::Bearer(tf.access_token)
        }
    };
    Ok(auth)
}

/// Главная точка: достать авторизацию при запуске.
/// 1. cookie из конфига → 2. импорт сессии YouTube Music Desktop →
/// 3. сохранённый токен → 4. вход по ссылке.
///
/// Для cookie-сессий дополнительно проверяется, что это реальный аккаунт:
/// анонимному посетителю YouTube отдаёт не «Мой джем», а общий микс.
pub async fn resolve(
    proxy: &Option<String>,
    // Cookie из конфига (файл `cookie` или `YOUMZ_COOKIE`) — имеет приоритет
    // над файлом, сохранённым предыдущим успешным входом.
    cookie: Option<&str>,
    client_id: &str,
    client_secret: &str,
) -> Result<Auth, String> {
    let from_file = cookie.map(str::to_string).or_else(load_cookie_file);
    let imported = if from_file.is_none() {
        match import_desktop_session() {
            Ok(cookie) => Some(cookie),
            Err(e) => {
                log::debug!("Импорт десктопной сессии не удался: {e}");
                None
            }
        }
    } else {
        None
    };

    if let Some(cookie) = from_file.or(imported) {
        let http = http_client(proxy)?;
        match verify_session(&http, &cookie).await {
            true => {
                log::info!("Сессия авторизована — играть будет ваш «Мой джем»");
                // Сохраняем только авторизованную сессию, чтобы при входе в
                // десктопное приложение хватило простого рестарта youmz
                let _ = save_cookie(&cookie);
                return Ok(Auth::Cookie(cookie));
            }
            false => {
                warn_anonymous();
                return Ok(Auth::Cookie(cookie));
            }
        }
    }

    if let Some(tf) = load_token_file() {
        let http = http_client(proxy)?;
        return auth_from_token(&http, tf).await;
    }

    // Самый первый запуск: предлагаем войти по ссылке
    println!(
        "Первый запуск: нужно войти в аккаунт Google. \
         Программа распечатает ссылку и сама достанет токен."
    );
    interactive_login(proxy, client_id, client_secret).await
}
