//! Иконка youmz в системном трее с мини-меню для выбора плейлиста.
//!
//! Общается с запущенным демоном youmz через D-Bus (org.youmz.Control):
//!  - список плейлистов пользователя (микс, понравившаяся музыка, свои плейлисты)
//!  - переключение плейлиста на лету
//!  - управление воспроизведением (следующий трек, пауза)

use std::sync::Arc;
use std::time::Duration;

use ksni::menu::{MenuItem, RadioGroup, RadioItem, StandardItem};
use ksni::{Tray as KsniTray, TrayMethods};

/// Прокси к интерфейсу управления демоном
#[zbus::proxy(
    interface = "org.youmz.Control",
    default_service = "org.mpris.MediaPlayer2.youmz",
    default_path = "/org/youmz/Control"
)]
trait YoumzControl {
    async fn list_playlists(&self) -> zbus::Result<Vec<(String, String)>>;
    async fn set_playlist(&self, id: &str) -> zbus::Result<()>;
    async fn current_playlist(&self) -> zbus::Result<String>;
    async fn now_playing(&self) -> zbus::Result<(String, String)>;
}

/// Прокси к MPRIS-интерфейсу плеера
#[zbus::proxy(
    interface = "org.mpris.MediaPlayer2.Player",
    default_service = "org.mpris.MediaPlayer2.youmz",
    default_path = "/org/mpris/MediaPlayer2",
    gen_blocking = false
)]
trait MprisPlayer {
    async fn next(&self) -> zbus::Result<()>;
    async fn play_pause(&self) -> zbus::Result<()>;
}

struct YoumzTray {
    /// Список плейлистов: (id, название)
    playlists: Vec<(String, String)>,
    /// Индекс текущего плейлиста в playlists
    current: usize,
    /// Что сейчас играет (для тултипа и заголовка меню)
    now_playing: String,
    /// Демон недоступен
    no_daemon: bool,
    /// D-Bus подключение для запросов из колбэков меню
    conn: zbus::Connection,
}

impl YoumzTray {
    /// Отправить демону команду через D-Bus, не блокируя меню
    fn fire<F, E>(&self, f: F)
    where
        F: std::future::Future<Output = Result<(), E>> + Send + 'static,
        E: std::fmt::Display + Send + 'static,
    {
        tokio::spawn(async move {
            if let Err(e) = f.await {
                log::error!("Ошибка D-Bus из трея: {e}");
            }
        });
    }
}

impl KsniTray for YoumzTray {
    const MENU_ON_ACTIVATE: bool = true;

    fn id(&self) -> String {
        "youmz".to_string()
    }

    fn title(&self) -> String {
        "youmz".to_string()
    }

    fn icon_name(&self) -> String {
        "audio-x-generic".to_string()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        if self.no_daemon {
            ksni::ToolTip {
                title: "youmz не запущен".to_string(),
                description: "Запустите демон youmz".to_string(),
                icon_name: String::new(),
                icon_pixmap: vec![],
            }
        } else {
            ksni::ToolTip {
                title: "youmz — YouTube Music".to_string(),
                description: self.now_playing.clone(),
                icon_name: String::new(),
                icon_pixmap: vec![],
            }
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        if self.no_daemon {
            return vec![StandardItem {
                label: "Демон youmz не запущен".to_string(),
                enabled: false,
                ..Default::default()
            }
            .into()];
        }

        let mut items = vec![
            StandardItem {
                label: self.now_playing.clone(),
                enabled: false,
                disposition: ksni::menu::Disposition::Informative,
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
        ];

        // Выбор плейлиста
        if !self.playlists.is_empty() {
            items.push(
                RadioGroup {
                    selected: self.current,
                    select: Box::new(|tray: &mut Self, idx: usize| {
                        let Some((id, title)) = tray.playlists.get(idx).cloned() else {
                            return;
                        };
                        log::info!("Выбран плейлист: {title} ({id})");
                        tray.current = idx;
                        let conn = tray.conn.clone();
                        tray.fire(async move {
                            let proxy = YoumzControlProxy::new(&conn).await?;
                            proxy.set_playlist(&id).await?;
                            Ok::<_, zbus::Error>(())
                        });
                    }),
                    options: self
                        .playlists
                        .iter()
                        .map(|(_, title)| RadioItem {
                            label: title.clone(),
                            ..Default::default()
                        })
                        .collect(),
                }
                .into(),
            );
            items.push(MenuItem::Separator);
        }

        // Управление воспроизведением
        items.push(
            StandardItem {
                label: "Следующий трек".to_string(),
                icon_name: "media-skip-forward".to_string(),
                activate: Box::new(|tray: &mut Self| {
                    let conn = tray.conn.clone();
                    tray.fire(async move {
                        let proxy = MprisPlayerProxy::new(&conn).await?;
                        proxy.next().await
                    });
                }),
                ..Default::default()
            }
            .into(),
        );
        items.push(
            StandardItem {
                label: "Пауза / Воспроизведение".to_string(),
                icon_name: "media-playback-pause".to_string(),
                activate: Box::new(|tray: &mut Self| {
                    let conn = tray.conn.clone();
                    tray.fire(async move {
                        let proxy = MprisPlayerProxy::new(&conn).await?;
                        proxy.play_pause().await
                    });
                }),
                ..Default::default()
            }
            .into(),
        );

        items.push(MenuItem::Separator);
        items.push(
            StandardItem {
                label: "Выйти".to_string(),
                icon_name: "application-exit".to_string(),
                activate: Box::new(|_tray: &mut Self| {
                    // Останавливаем демон через systemctl: он выключит звук,
                    // отпустит D-Bus-имя и остановит сам трей (юниты связаны).
                    log::info!("Выход: systemctl --user stop youmz.service");
                    match std::process::Command::new("systemctl")
                        .args(["--user", "stop", "youmz.service"])
                        .spawn()
                    {
                        Ok(mut child) => {
                            // Не блокируем меню: systemd остановит нас за пару секунд
                            std::thread::spawn(move || {
                                let _ = child.wait();
                            });
                        }
                        Err(e) => {
                            log::error!("Не удалось вызвать systemctl: {e}");
                            // systemctl недоступен — просто выходим сами
                            std::process::exit(0);
                        }
                    }
                }),
                ..Default::default()
            }
            .into(),
        );

        items
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let conn = zbus::Connection::session().await?;

    // Подключаемся к демону и забираем список плейлистов
    let tray = match YoumzControlProxy::new(&conn).await {
        Ok(proxy) => {
            match (proxy.list_playlists().await, proxy.current_playlist().await) {
                (Ok(playlists), Ok(current_id)) => {
                    let current = playlists
                        .iter()
                        .position(|(id, _)| *id == current_id)
                        .unwrap_or(0);
                    log::info!(
                        "Плейлистов доступно: {}, текущий: {:?}",
                        playlists.len(),
                        playlists.get(current).map(|(_, t)| t.as_str()).unwrap_or("?")
                    );
                    YoumzTray {
                        playlists,
                        current,
                        now_playing: "загрузка…".to_string(),
                        no_daemon: false,
                        conn: conn.clone(),
                    }
                }
                _ => {
                    log::warn!("Демон youmz не отвечает: покажу заглушку");
                    YoumzTray {
                        playlists: vec![],
                        current: 0,
                        now_playing: String::new(),
                        no_daemon: true,
                        conn: conn.clone(),
                    }
                }
            }
        }
        Err(e) => {
            log::warn!("Демон youmz не запущен ({e}): покажу заглушку");
            YoumzTray {
                playlists: vec![],
                current: 0,
                now_playing: String::new(),
                no_daemon: true,
                conn: conn.clone(),
            }
        }
    };

    let handle = Arc::new(tray.spawn().await?);
    log::info!("Иконка youmz зарегистрирована в трее");

    // Периодически обновляем информацию о текущем треке и переподключаемся
    // к демону, если он был недоступен на старте (трей мог запуститься раньше).
    let h = handle.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(3)).await;
            let ctrl = match YoumzControlProxy::new(&conn).await {
                Ok(p) => p,
                Err(_) => {
                    // Демон исчез — показываем заглушку, ждём его возвращения
                    let _ = h
                        .update(|t| {
                            t.no_daemon = true;
                            t.playlists.clear();
                            t.now_playing = String::new();
                        })
                        .await;
                    continue;
                }
            };

            // (Пере)загружаем список плейлистов и текущий выбор
            if let (Ok(playlists), Ok(current_id)) =
                (ctrl.list_playlists().await, ctrl.current_playlist().await)
            {
                let current = playlists
                    .iter()
                    .position(|(id, _)| *id == current_id)
                    .unwrap_or(0);
                let _ = h
                    .update(move |t| {
                        t.no_daemon = false;
                        t.playlists = playlists;
                        t.current = current;
                    })
                    .await;
            }

            let (artist, title) = match ctrl.now_playing().await {
                Ok(v) => v,
                Err(_) => continue,
            };
            let now_playing = if title.is_empty() {
                "Ничего не играет".to_string()
            } else if artist.is_empty() {
                title
            } else {
                format!("{artist} — {title}")
            };
            let _ = h.update(move |t| t.now_playing = now_playing).await;
        }
    });

    // Держим процесс живым
    loop {
        tokio::time::sleep(Duration::from_secs(3600)).await;
    }
}
