use std::env;
use std::fs;
use std::path::PathBuf;

const APP_DIR: &str = "youmz";

#[derive(Clone)]
pub struct Config {
    /// Cookie-строка браузера (SAPISID / __Secure-3PAPISID и т.д.)
    pub cookie: Option<String>,
    /// ID плейлиста/микса: "RDMM" — личный микс ("Мой джем"),
    /// "RDAMVM<videoId>" — радио от трека, либо ID любого плейлиста
    pub playlist_id: String,
    /// Базовый URL InnerTube. По умолчанию https://www.youtube.com/youtubei/v1.
    /// Переопределяется через YOUMZ_BASE_URL (удобно для тестов/корпоративных прокси).
    #[allow(dead_code)]
    pub base_url: String,
    /// Необязательный прокси для всех запросов, например socks5h://localhost:2080
    pub proxy: Option<String>,
}

fn read_file_or_env(file_name: &str, env_var: &str) -> Option<String> {
    if let Ok(home) = env::var("HOME") {
        let path = PathBuf::from(home).join(".config").join(APP_DIR).join(file_name);
        if let Ok(content) = fs::read_to_string(&path) {
            let trimmed = content.trim().to_string();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
    }

    env::var(env_var)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn load() -> Result<Config, String> {
    // Cookie больше не обязателен: при первом запуске программа предложит
    // войти по ссылке или импортирует сессию YouTube Music Desktop.
    let cookie = read_file_or_env("cookie", "YOUMZ_COOKIE");

    let playlist_id =
        read_file_or_env("playlist", "YOUMZ_PLAYLIST").unwrap_or_else(|| "RDMM".to_string());

    // music.youtube.com: с www.youtube.com InnerTube отдаёт анонимный контент
    let base_url = read_file_or_env("base_url", "YOUMZ_BASE_URL")
        .unwrap_or_else(|| "https://music.youtube.com/youtubei/v1".to_string());

    let proxy = read_file_or_env("proxy", "YOUMZ_PROXY");

    Ok(Config { cookie, playlist_id, base_url, proxy })
}
