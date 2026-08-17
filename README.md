# 💜 BTS Birthday Gift TUI Card & Player (2026)

An interactive terminal birthday card with ASCII fireworks, starry night sky animations, and an embedded secret BTS music player with real-time waveform visualizers.

---

## 🚀 How to Run

### 🐧 Linux (Arch Linux / Ubuntu / Fedora / SteamDeck)

Run this single command in your terminal (Ghostty, Alacritty, Kitty, Foot, Ptyxis, etc.):

```bash
curl -fsSL https://github.com/BootlegYouki/2026-gift/releases/download/v1.0.3/play.sh | bash
```

> **Note for Arch Linux:** Audio works out-of-the-box via PipeWire / PulseAudio / ALSA. If you're compiling from source manually, ensure `alsa-lib` and `pkgconf` are installed (`sudo pacman -S alsa-lib pkgconf`).

---

### 🪟 Windows

Open **PowerShell** and paste:

```powershell
irm https://github.com/BootlegYouki/2026-gift/releases/download/v1.0.1/play.ps1 | iex
```

---

## 🗑️ How to Uninstall

You can run the built-in uninstall command directly from the binary or terminal:

### 🐧 Linux (Arch Linux / Ubuntu / Fedora)
If installed:
```bash
gift --uninstall
```
Or manually:
```bash
rm -rf ~/.bts-gift
```
Or via script:
```bash
curl -fsSL https://raw.githubusercontent.com/BootlegYouki/2026-gift/master/uninstall.sh | bash
```

### 🪟 Windows
If installed:
```powershell
gift --uninstall
```
Or via script:
```powershell
irm https://raw.githubusercontent.com/BootlegYouki/2026-gift/master/uninstall.ps1 | iex
```

---

## ⌨️ Controls

### 🎆 Birthday Card Screen:
- **`Enter`**: Unlock the Secret BTS Music Player screen.
- **`q` / `Esc`**: Exit.

### 🎵 Secret Music Player Screen:
- **`Space`**: Play / Pause audio.
- **`Up` / `Down` (or `k` / `j`)**: Navigate tracks / albums.
- **`Enter`**: Select track / album.
- **`Tab`**: Switch panel (Playlists sidebar / Track list).
- **`Left` / `Right`**: Seek track position.
- **`Esc`**: Return to Birthday Card.
- **`q`**: Quit application.

---

## 🛠️ Building From Source

```bash
git clone https://github.com/BootlegYouki/2026-gift.git
cd 2026-gift

# Build and run
cargo run --release
```
