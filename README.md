# youmz (YouTube Music Zero)

<p align="center">
  <a href="README.md"><img src="https://img.shields.io/badge/Language-English-blue?style=for-the-badge" alt="English" /></a>
  <a href="README_RU.md"><img src="https://img.shields.io/badge/Язык-Русский-lightgrey?style=for-the-badge" alt="Русская версия" /></a>
</p>

<p align="center">
  <b>English</b> | <a href="README_RU.md">Русский</a>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Language-Rust-dea584?style=for-the-badge&logo=rust" alt="Rust" />
  <img src="https://img.shields.io/badge/Platform-Linux-1793d1?style=for-the-badge&logo=linux" alt="Linux" />
  <img src="https://img.shields.io/badge/Memory-~25MB_RSS-brightgreen?style=for-the-badge" alt="RAM" />
  <img src="https://img.shields.io/badge/License-MIT-blue?style=for-the-badge" alt="License" />
</p>

An ultra-lightweight headless **YouTube Music** client written in Rust. Plays your continuous personalized **“My Supermix”** (RDMM) stream, carries zero Electron/Chromium bloat, natively integrates into the desktop environment via **MPRIS v2**, and is controlled using standard system utilities (`playerctl`, Waybar, panel widgets, media hotkeys).

Yes, it was written with AI, I don't hide it!

There is also a similar client for Yandex Music: [YMZ!](https://github.com/BBQQYT/YMZ)

---

### Features

* **Zero-bloat:** memory footprint stays strictly within **20–25 MB RSS** (compared to ~700 MB for browser tabs or desktop Electron apps).
* **Full MPRIS v2 Support:**
  * Real-time track title, artist, and cover art.
  * Local high-resolution artwork caching (`~/.cache/youmz/covers/`, `file://` URI) for instant rendering in notification centers, widgets, and lockscreens.
  * Time synchronization: track length (`mpris:length`) and playback progress (`Position`).
  * Seeking support via slider clicks (`Seek`, `SetPosition`).
* **Instant Skips (Gapless Preload):** the upcoming track and its album art are prefetched into memory and disk cache in the background while the current track plays.
* **Ad & Unavailable Track Filtering:** advertising segments, promos, and unplayable tracks are automatically discarded prior to playback.
* **Loop Protection:** played track history filters out repetitions within received radio batches.
* **Proxy Support:** full support for SOCKS5/HTTP proxies (`socks5h://...`) to circumvent network restrictions and YouTube blocks.
* **Security:** isolated session storage at `~/.config/youmz/cookie` with `600` file permissions.
* **Network Resilience:** automatic retry with exponential backoff on network failures or bot checks.
* **Versatility:** works seamlessly with PipeWire, PulseAudio, and bare ALSA across any Linux distribution.
* **System Tray Icon (optional):** `youmz-tray` featuring account playlist selection and on-the-fly queue switching via D-Bus (`org.youmz.Control`).
* **Graphical Login (optional):** `youmz-login` opens a lightweight WebKit window for YouTube Music to automatically capture and save cookie sessions.

---

### System Dependencies

Building requires ALSA development headers and `pkg-config`. For reliable YouTube audio extraction and streaming, `yt-dlp` is used:

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

### Build & Installation

1. **Clone the repository:**
   ```bash
   git clone https://github.com/BBQQYT/youmz.git
   cd youmz
   ```

2. **Compile the release binary:**
   ```bash
   # Base daemon build:
   cargo build --release

   # Or with built-in tray and graphical login:
   cargo build --release --features "gui-login,tray"
   ```

3. **(Optional) Install to system:**
   ```bash
   sudo install -Dm755 target/release/youmz /usr/local/bin/youmz
   # If optional components were built:
   sudo install -Dm755 target/release/youmz-login /usr/local/bin/youmz-login
   sudo install -Dm755 target/release/youmz-tray /usr/local/bin/youmz-tray
   ```

---

### Configuration

1. **Authentication (Cookie):**
   Save your authenticated YouTube Music session cookie into a file with `600` permissions:
   ```bash
   mkdir -p ~/.config/youmz
   echo "YOUR_COOKIE" > ~/.config/youmz/cookie
   chmod 600 ~/.config/youmz/cookie
   ```
   > Alternatively, run `youmz login` for interactive login through a webview window.

2. **Proxy (optional):**
   If YouTube access is restricted, provide a proxy address (a `socks5h` protocol with remote DNS resolution is recommended):
   ```bash
   echo "socks5h://localhost:2080" > ~/.config/youmz/proxy
   ```

3. **Stream / Playlist Selection:**
   By default, your personal supermix **"My Supermix"** (`RDMM`) is played. You can optionally specify another playlist or radio ID:
   ```bash
   echo "RDMM" > ~/.config/youmz/playlist
   ```

---

### Autostart

#### Option 1: systemd user service (recommended)

Create `~/.config/systemd/user/youmz.service`:

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

Enable and start the service:

```bash
systemctl --user daemon-reload
systemctl --user enable --now youmz.service
```

#### Option 2: Running without systemd (Hyprland / Sway / AwesomeWM / i3)

Add `youmz` to your window manager or compositor startup config:

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

### Controls

```bash
# Play / Pause
playerctl -p youmz play-pause

# Next track (skip in My Supermix)
playerctl -p youmz next

# Seek forward/backward by 10 seconds
playerctl -p youmz position 10+
playerctl -p youmz position 10-

# Jump to a specific second (e.g. 1:15)
playerctl -p youmz position 75

# Display current metadata and playback status
playerctl -p youmz metadata
```

#### Waybar Integration (`config.jsonc`):

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

### License

This project is licensed under the [MIT](LICENSE) License.
