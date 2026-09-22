use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, COOKIE, ORIGIN, REFERER};
use sha1::{Digest, Sha1};
use serde_json::Value;

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

    let body = serde_json::json!({
        "browseId": "FEmusic_library_landing",
        "context": {"client": {"clientName": "WEB_REMIX", "clientVersion": "1.20241216.01.00", "hl": "ru", "gl": "RU"}}
    });
    let r = http.post("https://music.youtube.com/youtubei/v1/browse?prettyPrint=false")
        .json(&body).send().await.unwrap();
    let v: Value = r.json().await.unwrap();

    // Рекурсивно ищем все объекты с playlistId и ближайшим title
    fn walk(node: &Value, out: &mut Vec<(String, String)>) {
        if let Some(obj) = node.as_object() {
            let pid = obj.get("playlistId").and_then(|x| x.as_str()).map(|s| s.to_string());
            let title = obj.get("title").and_then(|t| {
                t.pointer("/runs/0/text").and_then(|x| x.as_str()).map(|s| s.to_string())
                    .or_else(|| t.get("simpleText").and_then(|x| x.as_str()).map(|s| s.to_string()))
            });
            if let (Some(pid), Some(title)) = (pid, title) {
                out.push((pid, title));
            }
            for (_, child) in obj { walk(child, out); }
        } else if let Some(arr) = node.as_array() {
            for c in arr { walk(c, out); }
        }
    }
    let mut items = vec![];
    walk(&v, &mut items);
    // dedup
    let mut seen = std::collections::HashSet::new();
    for (pid, title) in &items {
        if seen.insert(pid.clone()) {
            println!("{pid}  |  {title}");
        }
    }
}
