mod api;
mod auth;
mod control;
mod config;
mod decoder;
mod mpris;

use api::{is_bot_check, History, Track, YtClient};
use crate::mpris::{
    build_metadata_map, notify_changed, notify_seeked, MprisPlayer, MprisRoot, PlayerCommand,
};
use rodio::{OutputStream, Sink};
use crate::decoder::Decoder as SymphoniaDecoder;
use std::collections::{HashMap, VecDeque};
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::{mpsc, RwLock};
use tokio::time::{sleep, Duration};
use zbus::connection::Builder;
use zbus::zvariant::Value;

struct Queue {
    tracks: VecDeque<Track>,
}

impl Queue {
    fn new() -> Self {
        Self { tracks: VecDeque::new() }
    }

    fn next(&mut self) -> Option<Track> {
        self.tracks.pop_front()
    }

    fn peek(&self) -> Option<&Track> {
        self.tracks.front()
    }

    fn extend(&mut self, tracks: Vec<Track>) {
        self.tracks.extend(tracks);
    }
}

/// Сколько раз подряд партия может состоять только из уже проигранного,
/// прежде чем мы очистим историю и пройдём радио заново
const EMPTY_BATCHES_BEFORE_RESET: u32 = 3;

/// Учесть неудачу трека. Три неудачи — и трек отправляется в историю
/// (как уже сыгранный), чтобы демон не зацикливался на вечном 403.
fn mark_failure(failures: &mut HashMap<String, u32>, history: &mut History, track: &Track) {
    let n = failures.entry(track.video_id.clone()).or_default();
    *n += 1;
    if *n >= 3 {
        log::warn!(
            "Трек «{} — {}» не удалось проиграть 3 раза — вырезаю из ротации",
            track.artist, track.title
        );
        history.push(track.video_id.clone());
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args: Vec<String> = std::env::args().collect();
    // `youmz login` — вход в аккаунт через окно YouTube Music (как в limusic):
    // открывается отдельный бинарник youmz-login с настоящим WebKit-окном,
    // пользователь входит привычным путём, программа забирает cookie.
    if args.iter().any(|a| a == "login" || a == "--login") {
        return login_command().await;
    }

    let cfg = Arc::new(config::load()?);
    log::info!("Плейлист: {} (RDMM = Мой джем)", cfg.playlist_id);

    // Авторизация: готовый cookie → импорт сессии YouTube Music Desktop →
    // вход по ссылке при первом запуске. Cookie-сессия дополнительно
    // проверяется на авторизованность (иначе YouTube отдаёт не ваш микс).
    let auth = auth::resolve(
        &cfg.proxy,
        cfg.cookie.as_deref(),
        &auth::load_client_id(),
        &auth::load_client_secret(),
    )
    .await?;

    let yt = Arc::new(YtClient::new(cfg.clone(), auth).await);

    log::info!("Клиент rustypipe готов (cookie-сессия)");

    let (_stream, stream_handle) = OutputStream::try_default()?;
    let sink = Arc::new(Sink::try_new(&stream_handle)?);

    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<PlayerCommand>();

    let current_title = Arc::new(RwLock::new("Мой джем".to_string()));
    let current_artist = Arc::new(RwLock::new("YouTube Music".to_string()));
    let current_art_url = Arc::new(RwLock::new(String::new()));
    let current_track_id = Arc::new(RwLock::new("0".to_string()));
    let current_duration_us = Arc::new(RwLock::new(0i64));

    // Плейлист можно переключить из трея не перезапуская демон
    let playlist_id = Arc::new(RwLock::new(cfg.playlist_id.clone()));
    let (switch_tx, mut switch_rx) = mpsc::unbounded_channel::<String>();

    // Имя на шине может ещё висеть на прошлом процессе при быстром рестарте
    let mut conn = None;
    for attempt in 0..10 {
        let mpris_player = MprisPlayer {
            cmd_tx: cmd_tx.clone(),
            sink: sink.clone(),
            current_title: current_title.clone(),
            current_artist: current_artist.clone(),
            current_art_url: current_art_url.clone(),
            current_track_id: current_track_id.clone(),
            current_duration_us: current_duration_us.clone(),
        };
        let control = control::YoumzControl {
            yt: yt.as_ref().clone(),
            switch_tx: switch_tx.clone(),
            playlist_id: playlist_id.clone(),
            current_title: current_title.clone(),
            current_artist: current_artist.clone(),
        };

        match Builder::session()?
            .name("org.mpris.MediaPlayer2.youmz")?
            .serve_at("/org/mpris/MediaPlayer2", MprisRoot)?
            .serve_at("/org/mpris/MediaPlayer2", mpris_player)?
            .serve_at("/org/youmz/Control", control)?
            .build()
            .await
        {
            Ok(c) => {
                conn = Some(c);
                break;
            }
            Err(e) if e.to_string().contains("NameTaken") => {
                log::warn!("Имя org.mpris.MediaPlayer2.youmz занято, попытка {}...", attempt + 1);
                sleep(Duration::from_secs(1)).await;
            }
            Err(e) => return Err(e.into()),
        }
    }
    let conn = conn.ok_or("Не удалось занять D-Bus имя после 10 попыток")?;

    log::info!("D-Bus шина org.mpris.MediaPlayer2.youmz зарегистрирована");

    let skip_flag = Arc::new(AtomicBool::new(false));

    // Обработчик команд MPRIS
    {
        let sink_ctrl = sink.clone();
        let skip_ctrl = skip_flag.clone();
        let conn_ctrl = conn.clone();
        let duration_ctrl = current_duration_us.clone();

        tokio::spawn(async move {
            while let Some(cmd) = cmd_rx.recv().await {
                match cmd {
                    PlayerCommand::PlayPause => {
                        let status = if sink_ctrl.is_paused() {
                            sink_ctrl.play();
                            "Playing"
                        } else {
                            sink_ctrl.pause();
                            "Paused"
                        };
                        let mut changed = HashMap::new();
                        changed.insert("PlaybackStatus", Value::from(status));
                        notify_changed(&conn_ctrl, changed).await;
                    }
                    PlayerCommand::Next => {
                        skip_ctrl.store(true, Ordering::SeqCst);
                        sink_ctrl.stop();
                    }
                    PlayerCommand::Stop => {
                        sink_ctrl.stop();
                        let mut changed = HashMap::new();
                        changed.insert("PlaybackStatus", Value::from("Stopped"));
                        notify_changed(&conn_ctrl, changed).await;
                    }
                    PlayerCommand::Seek(offset_us) => {
                        let cur = sink_ctrl.get_pos().as_micros() as i64;
                        let total = *duration_ctrl.read().await;
                        let target = (cur + offset_us).clamp(0, total);
                        if sink_ctrl.try_seek(Duration::from_micros(target as u64)).is_ok() {
                            notify_seeked(&conn_ctrl, target).await;
                        }
                    }
                    PlayerCommand::SetPosition(pos_us) => {
                        let total = *duration_ctrl.read().await;
                        let target = pos_us.clamp(0, total);
                        if sink_ctrl.try_seek(Duration::from_micros(target as u64)).is_ok() {
                            notify_seeked(&conn_ctrl, target).await;
                        }
                    }
                }
            }
        });
    }

    // Единый сигнал завершения: SIGTERM/SIGINT от systemd (`systemctl stop`)
    // или от Ctrl-C. Канал асинхронный, поэтому цикл воспроизведения реагирует
    // на остановку даже посреди скачивания трека.
    let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
    {
        let shutdown_tx = shutdown_tx.clone();
        tokio::spawn(async move {
            let mut sigterm = match signal(SignalKind::terminate()) {
                Ok(s) => s,
                Err(e) => {
                    log::error!("Не установить обработчик SIGTERM: {e}");
                    return;
                }
            };
            let mut sigint = match signal(SignalKind::interrupt()) {
                Ok(s) => s,
                Err(e) => {
                    log::error!("Не установить обработчик SIGINT: {e}");
                    return;
                }
            };
            tokio::select! {
                _ = sigterm.recv() => log::info!("SIGTERM: останавливаю воспроизведение…"),
                _ = sigint.recv() => log::info!("SIGINT: останавливаю воспроизведение…"),
            }
            let _ = shutdown_tx.send(()).await;
        });
    }

    let mut queue = Queue::new();
    // История успешно заигравших треков — чтобы радио не ходило по кругу
    let mut history = History::new(100);
    // Счётчик неудач по трекам: если трек не заиграл с MAX_FAILURES попыток,
    // он отправляется в историю (чёрный список), чтобы не зациклиться на нём
    let mut failures: HashMap<String, u32> = HashMap::new();
    // Экспоненциальная пауза при бот-проверке: лучше ждать, чем штормить
    // запросами и углублять ограничения
    let mut player_backoff = Duration::from_secs(15);
    // Пауза и счётчик, когда партия состоит только из уже проигранного
    let mut empty_backoff = Duration::from_secs(10);
    let mut empty_streak = 0u32;

    log::info!("Запуск воспроизведения «Мой джем»");

    loop {
        // Подгружаем партию треков, когда очередь пуста
        if queue.tracks.is_empty() {
            let current_playlist = playlist_id.read().await.clone();
            let batch = tokio::select! {
                _ = shutdown_rx.recv() => {
                    sink.stop();
                    log::info!("Завершение работы youmz");
                    return Ok(());
                }
                res = yt.get_mix_tracks(&current_playlist) => match res {
                    Ok(t) => t,
                    Err(e) => {
                        log::error!("Ошибка получения микса: {e}. Повтор через 5с");
                        sleep(Duration::from_secs(5)).await;
                        continue;
                    }
                },
            };

            let fresh: Vec<Track> = history
                .filter_fresh(&batch)
                .into_iter()
                .cloned()
                .collect();

            if fresh.is_empty() {
                // Радио выдало всё, что есть — и всё это уже играло. Не
                // штормим API: ждём с нарастающей паузой, а если ситуация
                // не меняется — проходим круг заново (настоящее радио тоже
                // повторяется, и это лучше мёртвой петли)
                empty_streak += 1;
                if empty_streak >= EMPTY_BATCHES_BEFORE_RESET {
                    history.clear();
                    empty_streak = 0;
                    empty_backoff = Duration::from_secs(10);
                    log::info!("Радио пройдено целиком — очищаю историю, начинаю круг заново");
                } else {
                    log::warn!(
                        "Партия — только уже проигранное/рекламное; жду {:.0?} (подряд: {empty_streak})",
                        empty_backoff
                    );
                    tokio::select! {
                        _ = shutdown_rx.recv() => {
                            sink.stop();
                            log::info!("Завершение работы youmz");
                            return Ok(());
                        }
                        _ = sleep(empty_backoff) => {}
                    }
                    empty_backoff = (empty_backoff * 2).min(Duration::from_secs(300));
                }
                continue;
            }
            empty_streak = 0;
            empty_backoff = Duration::from_secs(10);

            queue.extend(fresh);
        }

        let track = match queue.next() {
            Some(t) => t,
            None => continue,
        };

        let duration_us = track.duration_us;

        // Берём предзагруженные байты или скачиваем через yt-dlp.
        // Скачивание может занимать время (медленный прокси), поэтому
        // оно прерывается по сигналу завершения.
        let fetch_res = tokio::select! {
            _ = shutdown_rx.recv() => {
                sink.stop();
                log::info!("Завершение работы youmz");
                return Ok(());
            }
            res = async {
                if let Some(b) = yt.take_cached(&track.video_id).await {
                    Ok(b)
                } else {
                    yt.fetch_audio(&track.video_id).await
                }
            } => res,
        };

        let bytes = match fetch_res {
            Ok(b) => {
                player_backoff = Duration::from_secs(15);
                b
            }
            Err(e) if is_bot_check(&e) => {
                // Бот-проверка: YouTube ограничил запросы с нашего IP.
                // Возвращаем трек в голову очереди и ждём с экспоненциальной
                // паузой — шторм запросов только усугубит бан.
                queue.tracks.push_front(track);
                log::warn!(
                    "Бот-проверка («{e}»): пауза {:.0?}, трек останется в очереди",
                    player_backoff
                );
                tokio::select! {
                    _ = shutdown_rx.recv() => {
                        sink.stop();
                        log::info!("Завершение работы youmz");
                        return Ok(());
                    }
                    _ = sleep(player_backoff) => {}
                }
                player_backoff = (player_backoff * 2).min(Duration::from_secs(600));
                continue;
            }
            Err(e) => {
                log::warn!("Ошибка скачивания «{} — {}»: {e}", track.artist, track.title);
                mark_failure(&mut failures, &mut history, &track);
                sleep(Duration::from_secs(1)).await;
                continue;
            }
        };

        let source = match SymphoniaDecoder::new(Cursor::new(bytes)) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("Ошибка декодирования «{} — {}»: {e}", track.artist, track.title);
                mark_failure(&mut failures, &mut history, &track);
                continue;
            }
        };

        // Трек скачался и декодируется — фиксируем в истории, чтобы
        // радио не предлагало его снова в этом круге
        history.push(track.video_id.clone());
        failures.remove(&track.video_id);

        // Скачиваем или берём из кэша локальную обложку трека (file:// URI),
        // чтобы шторка Dank Linux и системные виджеты могли отобразить превью
        let art_url = yt
            .fetch_and_cache_cover(&track.video_id, &track.art_url)
            .await
            .unwrap_or_else(|| track.art_url.clone());

        // Публикуем метаданные в MPRIS
        *current_title.write().await = track.title.clone();
        *current_artist.write().await = track.artist.clone();
        *current_track_id.write().await = track.video_id.clone();
        *current_art_url.write().await = art_url.clone();
        *current_duration_us.write().await = duration_us;

        let meta = build_metadata_map(
            &track.title,
            &track.artist,
            &art_url,
            &track.video_id,
            duration_us,
        );
        let mut changed = HashMap::new();
        changed.insert("Metadata", Value::from(meta));
        changed.insert("PlaybackStatus", Value::from("Playing"));
        notify_changed(&conn, changed).await;

        log::info!("▶ {} — {}", track.artist, track.title);

        // Предзагружаем следующий трек в фон — скип будет мгновенным
        if let Some(next) = queue.peek() {
            yt.try_preload(next.clone());
        }

        sink.stop();
        sink.append(source);
        sink.play();

        // Цикл ожидания конца трека / скипа / завершения
        loop {
            tokio::select! {
                _ = shutdown_rx.recv() => {
                    sink.stop();
                    log::info!("Завершение работы youmz");
                    return Ok(());
                }
                Some(new_id) = switch_rx.recv() => {
                    {
                        let mut w = playlist_id.write().await;
                        *w = new_id.clone();
                    }
                    queue.tracks.clear();
                    history.clear();
                    log::info!("🔀 Переключение на плейлист {new_id}");
                    sink.stop();
                    break;
                }
                _ = sleep(Duration::from_millis(150)) => {
                    if skip_flag.swap(false, Ordering::SeqCst) {
                        log::info!("⏭ Скип: {} — {}", track.artist, track.title);
                        sink.stop();
                        break;
                    }
                    if sink.empty() {
                        yt.forget(&track.video_id).await;
                        break;
                    }
                }
            }
        }
    }
}

/// Команда `youmz login`: открывает окно входа (youmz-login), а если
/// графический бинарник не собран/нет дисплея — fallback на device flow.
async fn login_command() -> Result<(), Box<dyn std::error::Error>> {
    let exe = std::env::current_exe()?;
    let gui_bin = exe.with_file_name("youmz-login");

    let display_ok = std::env::var("DISPLAY").is_ok() || std::env::var("WAYLAND_DISPLAY").is_ok();

    if gui_bin.exists() && display_ok {
        println!("Открываю окно входа в YouTube Music…");
        let status = std::process::Command::new(&gui_bin).status()?;
        if status.success() {
            println!("Готово! Теперь можно запускать youmz");
            return Ok(());
        }
        println!("Окно входа закрылось без успеха, пробую вход по ссылке…");
    } else if !gui_bin.exists() {
        log::warn!(
            "Бинарник {} не найден — соберите с фичей gui-login.              Использую вход по ссылке.",
            gui_bin.display()
        );
    } else if !display_ok {
        log::warn!("Нет графического дисплея (DISPLAY/WAYLAND_DISPLAY) — вход по ссылке.");
    }

    let proxy = config::load()?.proxy;
    auth::interactive_login(&proxy, &auth::load_client_id(), &auth::load_client_secret()).await?;
    println!("Готово! Cookie сохранён, можно запускать youmz");
    Ok(())
}
