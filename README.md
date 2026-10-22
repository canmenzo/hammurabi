# Hammurabi

**Live captions and translation for anything your computer plays or hears. Fully local.**

A tray app that captions videos, films, streams, calls, or the person talking to you, and optionally translates them. No account, no API key, no audio leaves your machine. Desktop sibling of the [Babil](https://babil.menzo.dev) browser extension.

[![The Hammurabi caption overlay over a video](https://hammurabi.menzo.dev/img/overlay.webp)](https://hammurabi.menzo.dev)

## Download

| Your computer | File |
|---|---|
| Windows 10/11 | [`hammurabi-windows-x64.zip`](https://github.com/canmenzo/hammurabi/releases/latest/download/hammurabi-windows-x64.zip) |
| Windows + NVIDIA RTX 50 series | [`hammurabi-windows-cuda.zip`](https://github.com/canmenzo/hammurabi/releases/latest/download/hammurabi-windows-cuda.zip) |
| macOS, Apple silicon | [`hammurabi-macos-arm64.zip`](https://github.com/canmenzo/hammurabi/releases/latest/download/hammurabi-macos-arm64.zip) |
| Linux x64 | [`hammurabi-linux-x64.tar.gz`](https://github.com/canmenzo/hammurabi/releases/latest/download/hammurabi-linux-x64.tar.gz) |

The builds are not code signed yet:

- **Windows:** unzip, run `hammurabi.exe`. On "Windows protected your PC", click **More info > Run anyway**. The CUDA zip needs only a recent NVIDIA driver.
- **macOS:** move `Hammurabi.app` to Applications, then run `xattr -dr com.apple.quarantine /Applications/Hammurabi.app` (or **System Settings > Privacy & Security > Open Anyway**). Allow **System Audio Recording** when you first start captions. Each update needs this permission re-allowed.
- **Linux:** `tar -xzf hammurabi-linux-x64.tar.gz && ./hammurabi-linux-x64/hammurabi`. System audio needs PulseAudio or `pipewire-pulse`. Use X11; Wayland blocks the always-on-top overlay and global hotkeys.

## Features

- Captions system audio, your microphone, or both
- Translates into 26 languages with a built-in model (MADLAD-400) or any local OpenAI-compatible LLM (Ollama, LM Studio)
- Reads finished captions aloud (the translation when translation is on) with the voices built into your OS
- Runs on any CPU, Apple GPUs (Metal), and RTX 50 GPUs (CUDA: ~50 ms per caption, ~0.3 s per translated sentence)
- Can use Windows Live Captions as the speech engine (Windows 11 22H2+)
- Click-through overlay that stays on top; tray menu and global hotkeys

## Usage

Right-click the tray icon to start/stop captions, pick the source, toggle translation, move the caption box, or open Settings.

| Shortcut | Action |
|---|---|
| `Alt+Shift+T` | Start / stop captions |
| `Alt+Shift+M` | Move captions (double-click to finish) |

Shortcuts are `hotkey_toggle` and `hotkey_move` in `config.toml`. `hammurabi --start` starts captions on launch.

**Read aloud** (tray menu or Settings) speaks each finished caption in its language with an installed system voice. Hammurabi stops listening while it speaks, so its own voice is never captioned again; whatever plays during that time is skipped. Add voices in **Windows Settings > Time & language > Speech** or **macOS System Settings > Accessibility > Spoken Content**. Linux needs `speech-dispatcher` or `espeak-ng`.

<img src="docs/img/settings.webp" alt="The Hammurabi settings window" width="360">

## Models

Downloaded on first use, resumable, verified by checksum.

| Model | Size | |
|---|---|---|
| tiny | 151 MB | Fastest |
| base | 290 MB | Auto default on CPU |
| small | 967 MB | More accurate |
| turbo | 1.62 GB | Auto default on GPU |
| MADLAD-400 3B | 1.7 GB | Built-in translation |

Config lives in `%APPDATA%\menzo\Hammurabi\config` on Windows (the OS equivalent elsewhere), models in the cache folder (override with `HAMMURABI_MODELS_DIR`). Settings has **Open models folder** and **Open logs**.

## Privacy

Audio is processed locally and never uploaded. Read aloud uses your OS's offline voices. No telemetry. Network use is limited to model downloads from Hugging Face, a GitHub release check, and the LLM URL you configure.

## Build from source

Needs stable [Rust](https://rustup.rs) plus MSVC Build Tools (Windows), Xcode CLT (macOS), or on Linux:

```sh
sudo apt install pkg-config libasound2-dev libxkbcommon-dev libx11-dev libxcursor-dev libxrandr-dev libxi-dev libgl1-mesa-dev libwayland-dev libdbus-1-dev
```

```sh
cargo build --release                    # CPU
cargo build --release --features metal   # Apple GPU
cargo build --release --features cuda    # NVIDIA, needs CUDA toolkit 12.8+
```

For CUDA on Windows, build from a Visual Studio developer prompt with `CUDA_PATH` set and `CUDA_COMPUTE_CAP` set to your GPU (120 = RTX 50, 89 = RTX 40, 86 = RTX 30). Keep the toolkit's `bin` on `PATH` or copy `cudart64_12`, `cublas64_12`, `cublasLt64_12` and `curand64_10` DLLs next to the exe. See [CONTRIBUTING.md](CONTRIBUTING.md) for tests.

## CLI

`hammurabi-cli` ships with every build:

```sh
hammurabi-cli devices                                    # list audio devices
hammurabi-cli record --source system --secs 10 out.wav   # record system audio
hammurabi-cli transcribe out.wav --translate tr          # caption + translate a WAV
hammurabi-cli download base                              # tiny, base, small, turbo, madlad
hammurabi-cli voices --lang tr                           # voices for reading aloud
hammurabi-cli speak --lang tr "Merhaba dünya"            # read text aloud
```

## Limitations

- Exclusive fullscreen games on Windows are not captured and cover the overlay
- RTL and Indic scripts are not shaped perfectly
- macOS older than 14.6 needs [BlackHole](https://existential.audio/blackhole/) for system audio
- Prebuilt CUDA supports RTX 50 only; build your own for other cards

Bugs and logs: [open an issue](https://github.com/canmenzo/hammurabi/issues/new/choose).

## License

[MIT](LICENSE). Whisper (MIT), MADLAD-400 (Apache-2.0), candle (MIT/Apache-2.0), Phosphor Icons (MIT). See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md), [CHANGELOG.md](CHANGELOG.md), [SECURITY.md](SECURITY.md).
