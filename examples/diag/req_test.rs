//! Проверка: прямой InnerTube-запрос через reqwest+rustls с заголовками
//! как у rustypipe. Должен вернуть авторизованные данные.

use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, COOKIE, ORIGIN, REFERER};
use sha1::{Digest, Sha1};

#[tokio::main]
async fn main() {
    let cookie = std::fs::read_to_string(format!("{}/.config/youmz/cookie", std::env::var("HOME").unwrap()))
        .expect("нет cookie")
        .trim()
        .to_string();

    let sapisid = cookie
        .split(';')
        .find_map(|c| c.trim().strip_prefix("SAPISID="))
        .expect("нет SAPISID")
        .to_string();

    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let mut h = Sha1::new();
    h.update(ts.to_string());
    h.update(" ");
    h.update(&sapisid);
    h.update(" ");
    h.update("https://music.youtube.com");
    let hash = data_encoding::HEXLOWER.encode(&h.finalize());
    let auth = format!("SAPISIDHASH {ts}_{hash}");
    println!("SAPISIDHASH готов");

    let mut hdrs = HeaderMap::new();
    hdrs.insert(COOKIE, HeaderValue::from_str(&cookie).unwrap());
    hdrs.insert(AUTHORIZATION, HeaderValue::from_str(&auth).unwrap());
    hdrs.insert("X-Goog-AuthUser", HeaderValue::from_static("0"));
    hdrs.insert(ORIGIN, HeaderValue::from_static("https://music.youtube.com"));
    hdrs.insert(REFERER, HeaderValue::from_static("https://music.youtube.com/"));
    hdrs.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    hdrs.insert("X-YouTube-Client-Name", HeaderValue::from_static("67"));
    hdrs.insert("X-YouTube-Client-Version", HeaderValue::from_static("1.20241216.01.00"));
    hdrs.insert(
        "User-Agent",
        HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36"),
    );

    let mut b = reqwest::Client::builder().default_headers(hdrs).timeout(std::time::Duration::from_secs(30));
    if let Ok(p) = std::env::var("YOUMZ_PROXY") {
        if !p.is_empty() {
            b = b.proxy(reqwest::Proxy::all(&p).unwrap());
            println!("прокси: {p}");
        }
    }
    let http = b.build().unwrap();

    // 1) История музыки
    let body = serde_json::json!({
        "browseId": "FEmusic_history",
        "params": "oggECgIIAQ%3D%3D",
        "context": {"client": {"clientName": "WEB_REMIX", "clientVersion": "1.20241216.01.00", "hl": "ru", "gl": "RU"}}
    });
    let r = http.post("https://music.youtube.com/youtubei/v1/browse?prettyPrint=false")
        .json(&body).send().await.unwrap();
    println!("browse status: {}", r.status());
    let txt = r.text().await.unwrap();
    println!("  logged_in=1: {}", txt.contains("\"logged_in\":\"1\""));
    println!("  videoIds: {}", txt.matches("videoId").count());

    // 2) Мой джем (RDMM)
    let body = serde_json::json!({
        "isAudioOnly": true,
        "playlistId": "RDMM",
        "context": {"client": {"clientName": "WEB_REMIX", "clientVersion": "1.20241216.01.00", "hl": "ru", "gl": "RU"}}
    });
    let r = http.post("https://music.youtube.com/youtubei/v1/next?prettyPrint=false")
        .json(&body).send().await.unwrap();
    println!("next status: {}", r.status());
    let txt = r.text().await.unwrap();
    println!("  размер: {}, videoIds: {}", txt.len(), txt.matches("videoId").count());
    let re = regex::Regex::new(r#""title":\{"runs":\[\{"text":"([^"]{3,60})""#).unwrap();
    let titles: Vec<String> = re.captures_iter(&txt).map(|c| c[1].to_string()).collect();
    println!("  первые треки джема: {titles:?}");
}
