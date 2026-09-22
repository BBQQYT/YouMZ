use reqwest::header::RANGE;
const UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

#[tokio::main]
async fn main() {
    let url = std::env::args().nth(1).expect("нужен URL");
    let mut b = reqwest::Client::builder().user_agent(UA).timeout(std::time::Duration::from_secs(30));
    if let Ok(p) = std::env::var("YOUMZ_PROXY") { b = b.proxy(reqwest::Proxy::all(&p).unwrap()); }
    let client = b.build().unwrap();
    // 1)plain GET
    let r = client.get(&url).send().await.unwrap();
    println!("plain GET  -> {} ({} B)", r.status(), r.bytes().await.unwrap().len());
    // 2) range 512K
    let r = client.get(&url).header(RANGE, "bytes=0-524287").send().await.unwrap();
    println!("range 512K -> {} ({} B)", r.status(), r.bytes().await.unwrap().len());
}
