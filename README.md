# Hammurabi

**Live captions and translation for anything your computer plays or hears, running entirely on your machine.**

Hammurabi lives in your system tray. Start it, and it writes captions for videos, films, streams, and calls. Optionally, it translates them into your language. No account, no API key, and no audio leaves your computer.

Website: https://hammurabi.menzo.dev

Hammurabi is the desktop sibling of [Babil](https://babil.menzo.dev), the browser extension that does the same for web videos.

> **Status: early development.** Prebuilt builds are published on the [Releases page](https://github.com/canmenzo/hammurabi/releases). macOS and Linux have not been tested by the author yet.

## Download

Get the latest build from the [Releases page](https://github.com/canmenzo/hammurabi/releases/latest):

| OS | File |
|---|---|
| Windows (x64) | `hammurabi-windows-x64.zip` |
| macOS (Apple silicon) | `hammurabi-macos-arm64.zip` |
| Linux (x64) | `hammurabi-linux-x64.tar.gz` |

Unzip it and run `hammurabi.exe` on Windows, `Hammurabi.app` on macOS, or `hammurabi` on Linux. `hammurabi-cli` is included too.

First run:

- Hammurabi downloads its speech model the first time it needs one (about 300 MB by default). The built-in translation model is about 1.7 GB more.
- **macOS:** the app is not notarized. Right-click `Hammurabi.app`, choose **Open**, and confirm. Then allow **System Audio Recording** in System Settings > Privacy & Security.
- **Windows:** SmartScreen may warn that the app is unrecognized. Click **More info**, then **Run anyway**.

## What it does

- Captions any system audio (videos, films, streams, calls) and/or your microphone
- Optional translation into 26 languages
- Fully local: speech recognition and translation run on your computer
- Lives in the system tray with a small caption overlay that sits on top of your other windows
- Works with CPU only, and uses your graphics card when the build supports it

## Install

1. Get a build: either a CI artifact from the [Actions page](https://github.com/canmenzo/hammurabi/actions), or [build from source](#build-from-source).
2. Run `hammurabi` (`hammurabi.exe` on Windows).
3. Pick an audio source from the tray menu, then choose **Start captions**.

## Using it

Right-click the tray icon:

| Menu item | What it does |
|---|---|
| Start / Stop captions | Turns captions on or off |
| Source | System audio, Microphone, or Both |
| Translate | Turns translation on or off |
| Target language | The language captions are translated into |
| Move captions | Drag the caption box to a new place; double-click to finish |
| Settings | Audio, speech model, translation, caption style, and folders |
| Quit | Closes Hammurabi |

Keyboard shortcuts:

- `Alt+Shift+T`: start or stop captions
- `Alt+Shift+M`: move captions

Windows uses `Alt+Shift` to switch keyboard layouts, so these shortcuts can clash with that. If a shortcut does not work, change it in the config file: `hotkey_toggle` and `hotkey_move` (the defaults are `alt+shift+KeyT` and `alt+shift+KeyM`).

## First run downloads

Hammurabi downloads its models the first time you need them. Downloads resume if they are interrupted. Captions start untranslated while the translation model downloads.

Speech models (pick one in Settings, under Speech):

| Model | Download size | Notes |
|---|---|---|
| tiny | 151 MB | Fastest, least accurate |
| base | 290 MB | Good default on CPU |
| small | 967 MB | Good default with a GPU build |
| turbo | 1.62 GB | Most accurate, needs a capable machine |
| Auto | depends | `base` on CPU, `small` on GPU builds (CUDA or Metal) |

Translation model (used when Translation is set to Built-in):

| Model | Download size |
|---|---|
| MADLAD-400 3B (q4k) | ~1.7 GB |

Files are stored in your OS's standard folders:

- Config: `config.toml` in the config folder. On Windows, for example, `%APPDATA%\menzo\Hammurabi\config\config.toml`.
- Models: a `models` folder inside the cache folder. On Windows, for example, `%LOCALAPPDATA%\menzo\Hammurabi\cache\models`.

Settings has an **Open models folder** button if you want to find them quickly.

## Translation

Hammurabi offers two ways to translate:

- **Built-in:** MADLAD-400 runs inside Hammurabi. Nothing else to install, but it needs the ~1.7 GB download.
- **Local LLM:** Use any OpenAI-compatible server running on your machine. Set the URL and model in Settings, under Translation:
  - Ollama: `http://localhost:11434/v1`
  - LM Studio: `http://localhost:1234/v1`

## System audio by OS

Hammurabi captures what your computer plays through a loopback of the output device. How that works depends on your OS:

- **Windows:** WASAPI loopback. Works over borderless fullscreen windows. Exclusive fullscreen games are not captured.
- **macOS 14.6 and newer:** Core Audio taps. The first time you use system audio, macOS asks for the **System Audio Recording** permission. Allow it.
- **macOS older than 14.6:** Install [BlackHole](https://existential.audio/blackhole/), route your output through it, and select BlackHole as the microphone device in Settings.
- **Linux:** Uses the monitor source of your default output through PulseAudio or PipeWire (with `pipewire-pulse`). Without a Pulse-compatible server, only the microphone is available.
- **Linux display server:** X11 is recommended. Wayland does not support always-on-top windows or global hotkeys, so the caption overlay and shortcuts will not work as expected there.

## Build from source

You need the [Rust toolchain](https://rustup.rs) (stable).

**Windows:** install the MSVC Build Tools (the "Desktop development with C++" workload). No CMake or clang needed.

**Linux:** install the build dependencies:

```sh
sudo apt install pkg-config libasound2-dev libxkbcommon-dev libx11-dev libxcursor-dev libxrandr-dev libxi-dev libgl1-mesa-dev libwayland-dev libdbus-1-dev
```

**macOS:** install Xcode Command Line Tools (`xcode-select --install`).

Then build:

```sh
cargo build --release
```

GPU builds are optional:

- `--features metal`: Apple GPUs (macOS)
- `--features cuda`: NVIDIA GPUs. Needs the CUDA toolkit. RTX 50 series cards need CUDA 12.8 or newer.

```sh
cargo build --release --features cuda
```

## Command line

`hammurabi-cli` is built next to the app and is handy for testing:

```sh
hammurabi-cli devices                                    # list audio devices
hammurabi-cli record --source system --secs 10 out.wav   # record what the computer plays
hammurabi-cli transcribe out.wav --translate tr          # caption a WAV file, translated to Turkish
hammurabi-cli transcribe out.wav --realtime              # feed it at 1x through the live engine
hammurabi-cli download base                              # tiny, base, small, turbo or madlad
```

`transcribe` also takes `--model`, `--lang`, `--llm <url> --llm-model <name>`, `--no-trim` and `--live`.

`hammurabi --start` starts captions as soon as the app launches.

## Limitations

- macOS and Linux have not been tested by the author yet. Testers are very welcome: please open an issue with what you saw.
- Right-to-left and Indic scripts are not shaped perfectly in the caption overlay.
- Speed depends on your CPU or GPU and the model you pick. On a slower machine, use `tiny` or `base`.
- Windows exclusive fullscreen games cannot be captured (see above).

## Credits and license

Hammurabi is released under the [MIT License](LICENSE). Third-party components:

- [Whisper](https://github.com/openai/whisper) speech recognition models: MIT
- [MADLAD-400](https://huggingface.co/jbochi/madlad400-3b-mt) translation model: Apache-2.0
- [candle](https://github.com/huggingface/candle) machine learning framework: MIT or Apache-2.0

Mel filter data is copied from candle. See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for details.
