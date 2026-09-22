use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, COOKIE, ORIGIN, REFERER};
use sha1::{Digest, Sha1};

#[tokio::main]
async fn main() {
    let home = std::env::var("HOME").unwrap();
    let cookie = std::fs::read_to_string(format!("{home}/.config/youmz/cookie"))
        .expect("нет cookie").trim().to_string();
    let sapisid = cookie.split(';')
        .find_map(|c| c.trim().strip_prefix("SAPISID=")).expect("нет SAPISID").to_string();
    let proxy = std::fs::read_to_string(format!("{home}/.config/youmz/proxy"))
        .unwrap_or_default().trim().to_string();
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let mut h = Sha1::new();
    h.update(ts.to_string()); h.update(" "); h.update(&sapisid); h.update(" "); h.update("https://www.youtube.com");
    let hash = data_encoding::HEXLOWER.encode(&h.finalize());
    let sapisid_music = {
        let mut h = Sha1::new();
        h.update(ts.to_string()); h.update(" "); h.update(&sapisid); h.update(" "); h.update("https://music.youtube.com");
        format!("SAPISIDHASH {}_{}", ts, data_encoding::HEXLOWER.encode(&h.finalize()))
    };
    let sapisid_www = format!("SAPISIDHASH {ts}_{hash}");

    let mut b = reqwest::Client::builder().timeout(std::time::Duration::from_secs(20));
    if !proxy.is_empty() { b = b.proxy(reqwest::Proxy::all(&proxy).unwrap()); }
    let http = b.build().unwrap();

    // ids: первый — из «Мой джем», второй — популярный клип
    for vid in ["Ap4a80WsnBo", "l2bN4ddgFiY", "dQw4w9WgXcQ"] {
        println!("=== videoId {vid} ===");
        let mut vs: Vec<(&str, &str, String, HeaderMap, serde_json::Value)> = Vec::new();

        // 1. music.youtube.com WEB_REMIX + auth + isAudioOnly
        let mut hdrs = web_hdr(&cookie, &sapisid_music, "https://music.youtube.com");
        hdrs.insert("X-YouTube-Client-Name", HeaderValue::from_static("67"));
        hdrs.insert("X-YouTube-Client-Version", HeaderValue::from_static("1.20241216.01.00"));
        vs.push(("MUSIC WEB_REMIX auth+audioOnly", "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36",
            "https://music.youtube.com/youtubei/v1/player?prettyPrint=false".into(), hdrs,
            serde_json::json!({"videoId": vid, "isAudioOnly": true, "context": {"client": {"clientName": "WEB_REMIX", "clientVersion": "1.20241216.01.00", "hl": "ru", "gl": "RU"}}})));

        // 2. www.youtube.com WEB + auth
        let mut hdrs = web_hdr(&cookie, &sapisid_www, "https://www.youtube.com");
        hdrs.insert("X-YouTube-Client-Name", HeaderValue::from_static("1"));
        hdrs.insert("X-YouTube-Client-Version", HeaderValue::from_static("2.20241216.01.00"));
        vs.push(("WWW WEB auth", "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36",
            "https://www.youtube.com/youtubei/v1/player?prettyPrint=false".into(), hdrs,
            serde_json::json!({"videoId": vid, "context": {"client": {"clientName": "WEB", "clientVersion": "2.20241216.01.00", "hl": "ru", "gl": "RU"}}})));

        // 3. ANDIOS + visitor-ish headers + cookie only, www
        let mut hdrs = HeaderMap::new();
        hdrs.insert(COOKIE, HeaderValue::from_str(&cookie).unwrap());
        hdrs.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        hdrs.insert("X-YouTube-Client-Name", HeaderValue::from_static("3"));
        hdrs.insert("X-YouTube-Client-Version", HeaderValue::from_static("21.03.36"));
        vs.push(("WWW ANDROID cookie", "com.google.android.youtube/21.03.36 (Linux; U; Android 11) gzip",
            "https://www.youtube.com/youtubei/v1/player?prettyPrint=false".into(), hdrs,
            serde_json::json!({"videoId": vid, "context": {"client": {"clientName": "ANDROID", "clientVersion": "21.03.36", "androidSdkVersion": 30, "hl": "ru", "gl": "RU"}}})));

        for (name, ua, url, hdrs, body) in vs {
            let r = http.post(&url).header(reqwest::header::USER_AGENT, HeaderValue::from_static(ua)).headers(hdrs).json(&body).send().await;
            match r {
                Ok(r) => {
                    let st = r.status();
                    let txt = r.text().await.unwrap_or_default();
                    let v: serde_json::Value = serde_json::from_str(&txt).unwrap_or(serde_json::Value::Null);
                    let play = v.pointer("/playabilityStatus/status").and_then(|x| x.as_str()).unwrap_or("?");
                    let reason = v.pointer("/playabilityStatus/reason").and_then(|x| x.as_str()).unwrap_or("");
                    let na = v.pointer("/streamingData/adaptiveFormats").and_then(|x| x.as_array()).map(|a| a.len()).unwrap_or(0);
                    let nf = v.pointer("/streamingData/formats").and_then(|x| x.as_array()).map(|a| a.len()).unwrap_or(0);
                    let sig = if txt.contains("signatureCipher") { "SIG" } else { "" };
                    println!("  {name:34} HTTP={st} status={play} reason={reason} adaptive={na} formats={nf} {sig}");
                }
                Err(e) => println!("  {name:34} ОШИБКА: {e}"),
            }
        }
    }
}

fn web_hdr(cookie: &str, auth: &str, origin: &str) -> HeaderMap {
    let origin = origin.to_string();
    let mut h = HeaderMap::new();
    h.insert(COOKIE, HeaderValue::from_str(cookie).unwrap());
    h.insert(AUTHORIZATION, HeaderValue::from_str(auth).unwrap());
    h.insert("X-Goog-AuthUser", HeaderValue::from_static("0"));
    h.insert(ORIGIN, HeaderValue::from_str(&origin).unwrap());
    h.insert(REFERER, HeaderValue::from_str(&origin).unwrap());
    h.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    h
}
