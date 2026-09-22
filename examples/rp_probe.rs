use rustypipe::client::{RustyPipeBuilder, RustyPipe};

#[tokio::main]
async fn main() {
    let mut cb = reqwest::Client::builder();
    cb = cb.proxy(reqwest::Proxy::all("socks5h://localhost:2080").unwrap());
    let rp: RustyPipe = RustyPipeBuilder::new()
        .storage_dir(std::path::PathBuf::from("/tmp/rp_cache"))
        .build_with_client(cb).unwrap();

    println!("=== cookie ===");
    let cookie = std::fs::read_to_string("/home/mb/.config/youmz/cookie").unwrap().trim().to_string();
    match rp.user_auth_set_cookie(cookie).await {
        Ok(_) => println!("cookie ok"),
        Err(e) => println!("cookie err: {e:?}"),
    }

    println!("=== player uVmiHKzNBEE (default) ===");
    match rp.query().player("uVmiHKzNBEE").await {
        Ok(p) => {
            println!("client={:?} audio_streams={}", p.client_type, p.audio_streams.len());
            for s in p.audio_streams.iter().take(3) {
                println!("  itag={} mime={} br={} url={}...", s.itag, s.mime, s.bitrate, &s.url[..60.min(s.url.len())]);
            }
        }
        Err(e) => println!("player err: {e:?}"),
    }

    for client in [rustypipe::client::ClientType::Ios, rustypipe::client::ClientType::Tv, rustypipe::client::ClientType::Android] {
        println!("=== player uVmiHKzNBEE ({client:?}) ===");
        match rp.query().player_from_client("uVmiHKzNBEE", client).await {
            Ok(p) => {
                println!("client={:?} audio_streams={}", p.client_type, p.audio_streams.len());
                for s in p.audio_streams.iter().take(3) {
                    println!("  itag={} mime={} br={} url={}...", s.itag, s.mime, s.bitrate, &s.url[..60.min(s.url.len())]);
                }
            }
            Err(e) => println!("player err: {e:?}"),
        }
    }
}
