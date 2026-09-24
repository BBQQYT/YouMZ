# youmz (YouTube Music Zero)

<p align="center">
  <a href="README.md"><img src="https://img.shields.io/badge/Language-English-lightgrey?style=for-the-badge" alt="English version" /></a>
  <a href="README_RU.md"><img src="https://img.shields.io/badge/Язык-Русский-blue?style=for-the-badge" alt="Русский" /></a>
</p>


<p align="center">
  <img src="https://img.shields.io/badge/Language-Rust-dea584?style=for-the-badge&logo=rust" alt="Rust" />
  <img src="https://img.shields.io/badge/Platform-Linux-1793d1?style=for-the-badge&logo=linux" alt="Linux" />
  <img src="https://img.shields.io/badge/Memory-~25MB_RSS-brightgreen?style=for-the-badge" alt="RAM" />
  <img src="https://img.shields.io/badge/License-MIT-blue?style=for-the-badge" alt="License" />
</p>

Ультралегковесный headless-клиент для **YouTube Music**, написанный на Rust. Играет бесконечный персональный поток **«Мой джем»** (RDMM), не тащит за собой Electron/Chromium, нативно интегрируется в окружение через **MPRIS v2** и управляется стандартными системными средствами (`playerctl`, Waybar, виджеты панелей, медиаклавиши).

Да, написан ИИ, я этого не скрываю!

Есть так же аналог но для Yandex Music, [YMZ!](https://github.com/BBQQYT/YMZ)

---

### Особенности

* **Zero-bloat:** потребление памяти в пределах **20–25 МБ RSS** (против ~700 МБ у браузера или десктопных сборок).
* **Полноценный MPRIS v2:**
  * Название, артист и обложка трека в реальном времени.
  * Локальное кэширование обложек в высоком разрешении (`~/.cache/youmz/covers/`, `file://` URI) для мгновенного отображения в шторках окружений, виджетах и на экранах блокировки.
  * Синхронизация времени: отображение длительности (`mpris:length`) и шкалы воспроизведения (`Position`).
  * Полная поддержка перемотки по клику на ползунок (`Seek`, `SetPosition`).
* **Мгновенные скипы (Gapless Preload):** следующий трек и его обложка предзагружаются в память и кэш в фоне, пока играет текущий трек.
* **Фильтрация рекламы:** рекламные слоты, промо-вставки и недоступные треки автоматически отсекаются до воспроизведения.
* **Защита от зацикливания:** история проигранных треков фильтрует повторы внутри присылаемых партий.
* **Поддержка прокси:** полная работа через SOCKS5/HTTP-прокси (`socks5h://...`) для обхода сетевых ограничений и блокировок YouTube.
* **Безопасность:** изолированное хранение сессии в `~/.config/youmz/cookie` с правами доступа `600`.
* **Сетевая устойчивость:** автоматический retry с экспоненциальной задержкой при сбоях сети или проверках на бота.
* **Универсальность:** работает с PipeWire, PulseAudio и чистой ALSA на любых дистрибутивах Linux.
* **Иконка в трее (опционально):** `youmz-tray` с меню выбора плейлистов аккаунта и сменой очереди на лету через D-Bus (`org.youmz.Control`).
* **Графический вход (опционально):** `youmz-login` открывает WebKit-окно YouTube Music для автоматического сохранения cookie-сессии.

---

### Системные зависимости

Для сборки требуются заголовочные файлы ALSA и `pkg-config`. Для надежного воспроизведения аудиопотоков YouTube используется `yt-dlp`:

* **Arch Linux / Manjaro / CachyOS:**
  ```bash
  sudo pacman -S alsa-lib pkgconf base-devel yt-dlp deno
  ```

* **Ubuntu / Debian / Linux Mint / Pop!_OS:**
  ```bash
  sudo apt install libasound2-dev pkg-config build-essential yt-dlp
  ```

* **Fedora / RHEL / AlmaLinux:**
  ```bash
  sudo dnf install alsa-lib-devel pkgconf-pkg-config gcc yt-dlp
  ```

* **openSUSE (Tumbleweed / Leap):**
  ```bash
  sudo zypper install alsa-devel pkg-config gcc yt-dlp
  ```

* **Void Linux:**
  ```bash
  sudo xbps-install -S alsa-lib-devel base-devel yt-dlp
  ```

---

### Сборка и установка

1. **Клонирование репозитория:**
   ```bash
   git clone https://github.com/BBQQYT/youmz.git
   cd youmz
   ```

2. **Компиляция релизного бинарника:**
   ```bash
   # Базовая сборка демона:
   cargo build --release

   # Либо со встроенным треем и графическим входом:
   cargo build --release --features "gui-login,tray"
   ```

3. **(Опционально) Установка в систему:**
   ```bash
   sudo install -Dm755 target/release/youmz /usr/local/bin/youmz
   # Если собирались дополнительные компоненты:
   sudo install -Dm755 target/release/youmz-login /usr/local/bin/youmz-login
   sudo install -Dm755 target/release/youmz-tray /usr/local/bin/youmz-tray
   ```

---

### Настройка

1. **Авторизация (Cookie):**
   Сохраните cookie авторизованной сессии YouTube Music в файл с правами `600`:
   ```bash
   mkdir -p ~/.config/youmz
   echo "ВАШ_COOKIE" > ~/.config/youmz/cookie
   chmod 600 ~/.config/youmz/cookie
   ```
   > Либо воспользуйтесь командой `youmz login` для графического входа через браузерное окно.

2. **Прокси (при необходимости):**
   Если доступ к YouTube ограничен, укажите адрес прокси (рекомендуется протокол `socks5h` с удалённым DNS):
   ```bash
   echo "socks5h://localhost:2080" > ~/.config/youmz/proxy
   ```

3. **Выбор потока / плейлиста:**
   По умолчанию воспроизводится личный супермикс **«Мой джем»** (`RDMM`). При желании можно указать другой идентификатор:
   ```bash
   echo "RDMM" > ~/.config/youmz/playlist
   ```

---

### Автозапуск

#### Вариант 1: systemd user service (рекомендуется)

Создайте файл `~/.config/systemd/user/youmz.service`:

```ini
[Unit]
Description=YouTube Music Zero Daemon
After=network-online.target pipewire.service wireplumber.service sound.target
Wants=network-online.target

[Service]
Type=dbus
BusName=org.mpris.MediaPlayer2.youmz
ExecStart=/usr/local/bin/youmz
Restart=on-failure
RestartSec=3

[Install]
WantedBy=default.target
```

Активация и запуск:

```bash
systemctl --user daemon-reload
systemctl --user enable --now youmz.service
```

#### Вариант 2: Запуск без systemd (Hyprland / Sway / AwesomeWM / i3)

Добавьте запуск в автозагрузку вашего оконного менеджера или композитора:

* **Hyprland** (`hyprland.conf`):
  ```ini
  exec-once = youmz
  ```

* **Sway** (`config`):
  ```ini
  exec youmz
  ```

* **AwesomeWM** (`rc.lua`):
  ```lua
  awful.spawn.with_shell("youmz")
  ```

---

### Управление

```bash
# Плей / Пауза
playerctl -p youmz play-pause

# Следующий трек (скачок по «Мой джем»)
playerctl -p youmz next

# Перемотка вперед/назад на 10 секунд
playerctl -p youmz position 10+
playerctl -p youmz position 10-

# Перейти на конкретную секунду (например, 1:15)
playerctl -p youmz position 75

# Текущие метаданные и статус
playerctl -p youmz metadata
```

#### Интеграция с Waybar (`config.jsonc`):

```jsonc
"mpris": {
    "player": "youmz",
    "format": "{player_icon} {artist} — {title} [{position}/{length}]",
    "player-icons": {
        "default": "▶",
        "playing": "▶",
        "paused": "⏸"
    }
}
```

---

### Лицензия

Проект распространяется под лицензией [MIT](LICENSE).
