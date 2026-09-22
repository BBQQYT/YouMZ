//! Графический вход в YouTube Music.
//!
//! Открывает обычное окно с настоящей веб-страницей music.youtube.com
//! (WebKit, тот же движок, что у Safari/GNOME Web): пользователь входит
//! в аккаунт привычным путём, а программа забирает cookie из собственного
//! хранилища. Никакого чтения сторонних браузеров.
//!
//! Запускается основным бинарником по команде `youmz login`.

use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoop, EventLoopBuilder};
use std::os::unix::fs::PermissionsExt;
use tao::window::WindowBuilder;
use wry::WebViewBuilder;

const TITLE: &str = "youmz — войдите в YouTube Music";
const URL: &str = "https://music.youtube.com";
const UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) \
                  Chrome/126.0.0.0 Safari/537.36";
/// Пользовательские события event loop
#[derive(Clone, Copy)]
enum UserEvent {
    /// Проверить cookie: вошёл пользователь или нет
    CheckCookie,
    /// Закрыть окно
    Close,
}

fn main() -> wry::Result<()> {
    let proxy = youmz::config::load().map(|c| c.proxy).unwrap_or(None);

    let event_loop: EventLoop<UserEvent> = EventLoopBuilder::with_user_event().build();

    // Опрос cookie из потока: просим event loop проверять каждые 3с
    let proxy_tick = event_loop.create_proxy();
    let proxy_close = proxy_tick.clone();
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(3));
        let _ = proxy_tick.send_event(UserEvent::CheckCookie);
    });

    let window = WindowBuilder::new()
        .with_title(TITLE)
        .with_inner_size(LogicalSize::new(1100.0, 820.0))
        .build(&event_loop)
        .expect("Не удалось создать окно");

    let mut builder = WebViewBuilder::new()
        .with_url(URL)
        .with_user_agent(UA)
        .with_devtools(false);

    if let Some(proxy_url) = proxy.as_deref() {
        if let Some((host, port)) = parse_socks(proxy_url) {
            builder = builder.with_proxy_config(wry::ProxyConfig::Socks5(wry::ProxyEndpoint {
                host,
                port,
            }));
        }
    }

    #[cfg(unix)]
    let webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        let vbox = window
            .default_vbox()
            .expect("GTK vbox недоступен — окно создано с with_default_vbox(false)");
        builder.build_gtk(vbox)?
    };
    #[cfg(not(unix))]
    let webview = builder.build(&window)?;

    println!("Окно входа открыто. Войдите в аккаунт Google в нём.");
    println!("После входа программа сама закроется и сохранит сессию.");

    let mut saved = false;

    event_loop.run(move |event, _target, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            // Периодическая проверка: вошёл ли пользователь
            Event::UserEvent(UserEvent::Close) => {
                *control_flow = ControlFlow::Exit;
            }

            Event::UserEvent(UserEvent::CheckCookie) if !saved => {
                match session_cookie(&webview) {
                    Ok(Some(cookie)) => {
                        println!("Вход выполнен, сохраняю сессию…");
                        match save_cookie(&cookie) {
                            Ok(path) => {
                                println!("Cookie сохранён: {}", path.display());
                                saved = true;
                                // Даём пользователю увидеть, что всё готово
                                let _ = webview.evaluate_script(
                                    "document.body.insertAdjacentHTML('beforeend', \
                                     '<div style=\"position:fixed;inset:0;z-index:99999;\
                                     display:flex;align-items:center;justify-content:center;\
                                     background:#0d0d0d;color:#1ed760;font:700 28px system-ui\">\
                                     Готово! Можно закрывать окно</div>');"
                                );
                            }
                            Err(e) => eprintln!("Не удалось сохранить cookie: {e}"),
                        }
                        // Закрываем окно через 4 секунды — пользователь увидит подтверждение
                        let proxy_exit = proxy_close.clone();
                        std::thread::spawn(move || {
                            std::thread::sleep(Duration::from_secs(4));
                            let _ = proxy_exit.send_event(UserEvent::Close);
                        });
                    }
                    Ok(None) => {}
                    Err(e) => eprintln!("Не удалось прочитать cookie: {e}"),
                }
            }

            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                if !saved {
                    // На случай, если пользователь закрывает окно сразу после входа
                    if let Ok(Some(cookie)) = session_cookie(&webview) {
                        if let Ok(path) = save_cookie(&cookie) {
                            println!("Cookie сохранён: {}", path.display());
                        }
                    }
                }
                *control_flow = ControlFlow::Exit;
            }

            _ => {}
        }
    });
}

/// Вытащить cookie youtube.com из хранилища WebView
fn session_cookie(webview: &wry::WebView) -> wry::Result<Option<String>> {
    let cookies = webview.cookies()?;

    // Интересуют только cookie youtube.com
    let mut pairs: Vec<String> = Vec::new();
    let mut has_sapisid = false;
    for c in cookies {
        let domain = c.domain().unwrap_or("");
        if !domain.contains("youtube.com") {
            continue;
        }
        if c.name() == "SAPISID" || c.name() == "__Secure-3PAPISID" {
            has_sapisid = true;
        }
        pairs.push(format!("{}={}", c.name(), c.value()));
    }

    // SAPISID есть только у авторизованного пользователя
    if !has_sapisid {
        return Ok(None);
    }

    Ok(Some(pairs.join("; ")))
}

fn save_cookie(cookie: &str) -> Result<PathBuf, String> {
    let dir = youmz::auth::config_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;

    let path = dir.join(youmz::auth::COOKIE_FILE);
    std::fs::write(&path, cookie).map_err(|e| format!("{}: {e}", path.display()))?;

    let mut perms = std::fs::metadata(&path).map_err(|e| e.to_string())?.permissions();
    perms.set_mode(0o600);
    std::fs::set_permissions(&path, perms).map_err(|e| e.to_string())?;

    Ok(path)
}

/// "socks5h://localhost:2080" -> ("localhost", "2080")
fn parse_socks(url: &str) -> Option<(String, String)> {
    let url = url.trim();
    let rest = url
        .strip_prefix("socks5h://")
        .or_else(|| url.strip_prefix("socks5://"))
        .or_else(|| url.strip_prefix("socks://"))?;
    let (host, port) = rest.split_once(':')?;
    let host = host.trim();
    let port = port.trim();
    if host.is_empty() || port.is_empty() {
        return None;
    }
    Some((host.to_string(), port.to_string()))
}
