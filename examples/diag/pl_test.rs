//! Какой browseId отдаёт плейлисты пользователя

use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, COOKIE, ORIGIN, REFERER};
use sha1::{Digest, Sha1};

#[tokio::main]
async fn main() {
    let cookie = std::fs::read_to_string(format!("{}/.config/youmz/cookie", std::env::var("HOME").unwrap()))
        .expect("нет cookie").trim().to_string();
    let sapisid = cookie.split(';').find_map(|c| c.trim().strip_prefix("SAPISID=")).unwrap().to_string();
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let mut h = Sha1::new();
    h.update(ts.to_string()); h.update(" "); h.update(&sapisid); h.update(" "); h.update("https://music.youtube.com");
    let auth = format!("SAPISIDHASH {ts}_{}", data_encoding::HEXLOWER.encode(&h.finalize()));

    let mut hdrs = HeaderMap::new();
    hdrs.insert(COOKIE, HeaderValue::from_str(&cookie).unwrap());
    hdrs.insert(AUTHORIZATION, HeaderValue::from_str(&auth).unwrap());
    hdrs.insert("X-Goog-AuthUser", HeaderValue::from_static("0"));
    hdrs.insert(ORIGIN, HeaderValue::from_static("https://music.youtube.com"));
    hdrs.insert(REFERER, HeaderValue::from_static("https://music.youtube.com/"));
    hdrs.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    hdrs.insert("X-YouTube-Client-Name", HeaderValue::from_static("67"));
    hdrs.insert("X-YouTube-Client-Version", HeaderValue::from_static("1.20241216.01.00"));

    let mut b = reqwest::Client::builder().default_headers(hdrs).timeout(std::time::Duration::from_secs(30));
    if let Ok(p) = std::env::var("YOUMZ_PROXY") {
        if !p.is_empty() { b = b.proxy(reqwest::Proxy::all(&p).unwrap()); }
    }
    let http = b.build().unwrap();

    for browse_id in ["FEmusic_liked_playlists", "FEmusic_library_landing"] {
        let body = serde_json::json!({
            "browseId": browse_id,
            "context": {"client": {"clientName": "WEB_REMIX", "clientVersion": "1.20241216.01.00", "hl": "ru", "gl": "RU"}}
        });
        let r = http.post("https://music.youtube.com/youtubei/v1/browse?prettyPrint=false")
            .json(&body).send().await.unwrap();
        let txt = r.text().await.unwrap();
        println!("\n===== {browse_id}: размер {} =====", txt.len());
        // ищем плейлисты: playlistId + title
        let re = regex::Regex::new(r#""playlistId":"([^"]{4,40})""#).unwrap();
        let ids: Vec<String> = re.captures_iter(&txt).map(|c| c[1].to_string()).collect();
        let re2 = regex::Regex::new(r#""title":"\{"runs":\[\{"text":"([^"]{2,60})""#).unwrap();
        let titles: Vec<String> = re2.captures_iter(&txt).map(|c| c[1].to_string()).collect();
        let re3 = regex::Regex::new(r#""text":"([^"]{2,60})""#).unwrap();
        let texts: Vec<String> = re3.captures_iter(&txt).map(|c| c[1].to_string()).collect();
        println!("playlistIds ({}): {:?}", ids.len(), ids.iter().take(15).collect::<Vec<_>>());
        println!("titles: {:?}", titles.iter().take(15).collect::<Vec<_>>());
        println!("texts: {:?}", texts.iter().take(25).collect::<Vec<_>>());
    }
}
