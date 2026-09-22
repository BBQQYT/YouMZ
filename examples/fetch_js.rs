#[tokio::main]
async fn main() {
    let http = reqwest::Client::builder()
        .proxy(reqwest::Proxy::all("socks5h://localhost:2080").unwrap())
        .timeout(std::time::Duration::from_secs(30)).build().unwrap();
    for (name, path) in [
        ("ias_en", "https://www.youtube.com/s/player/4fd832e7/player_ias.vflset/en_US/base.js"),
        ("ias_ru", "https://www.youtube.com/s/player/4fd832e7/player_ias.vflset/ru_RU/base.js"),
    ] {
        match http.get(path).header(reqwest::header::USER_AGENT, "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36").send().await {
            Ok(r) => { let t = r.text().await.unwrap(); std::fs::write(format!("/tmp/{name}.js"), &t).unwrap(); println!("{name}: {}", t.len()); }
            Err(e) => println!("{name}: ОШИБКА {e}"),
        }
    }
}
