# Hammurabi

**Live captions and translation for anything your computer plays or hears. Fully local.**

A tray app that captions videos, films, streams, calls, or the person talking to you, and optionally translates them. No account, no API key, no audio leaves your machine. Desktop sibling of the [Babil](https://babil.menzo.dev) browser extension.

[![The Hammurabi caption overlay over a video](https://hammurabi.menzo.dev/img/overlay.webp)](https://hammurabi.menzo.dev)

## Download

| Your computer | File |
|---|---|
| Windows 10/11 | [`hammurabi-windows-x64.zip`](https://github.com/canmenzo/hammurabi/releases/latest/download/hammurabi-windows-x64.zip) |
| Windows + NVIDIA RTX 50 series | [`hammurabi-windows-cuda.zip`](https://github.com/canmenzo/hammurabi/releases/latest/download/hammurabi-windows-cuda.zip) |
| Windows + any GPU, with Speaker's voice | [`hammurabi-windows-gpu.zip`](https://github.com/canmenzo/hammurabi/releases/latest/download/hammurabi-windows-gpu.zip) |
| macOS, Apple silicon | [`hammurabi-macos-arm64.zip`](https://github.com/canmenzo/hammurabi/releases/latest/download/hammurabi-macos-arm64.zip) |
| Linux x64 | [`hammurabi-linux-x64.tar.gz`](https://github.com/canmenzo/hammurabi/releases/latest/download/hammurabi-linux-x64.tar.gz) |

The builds are not code signed yet:

- **Windows:** unzip, run `hammurabi.exe`. On "Windows protected your PC", click **More info > Run anyway**. The CUDA zip needs only a recent NVIDIA driver.
- **macOS:** move `Hammurabi.app` to Applications, then run `xattr -dr com.apple.quarantine /Applications/Hammurabi.app` (or **System Settings > Privacy & Security > Open Anyway**). Allow **System Audio Recording** when you first start captions. Each update needs this permission re-allowed.
- **Linux:** `tar -xzf hammurabi-linux-x64.tar.gz && ./hammurabi-linux-x64/hammurabi`. System audio needs PulseAudio or `pipewire-pulse`. Use X11; Wayland blocks the always-on-top overlay and global hotkeys.

## Features

- Captions system audio, your microphone, or both
- Translates into 26 languages with a built-in model (MADLAD-400) or any local OpenAI-compatible LLM (Ollama, LM Studio)
- Reads finished captions aloud (the translation when translation is on) with the voices built into your OS or downloadable Supertonic 3 neural voices, in six voice personas you can preview
- Speaker's voice (Windows GPU build): reads the translation in the voice of the person speaking, learned on your computer from a few seconds of their speech
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

For natural voices, set **Voice engine** in Settings > Read aloud to **Supertonic 3** and press **Download** (about 210 MB, once). Supertonic 3 by Supertone runs on the CPU, reads 22 languages (Turkish, English, Spanish, French, German, Italian, Portuguese, Russian, Arabic, Hindi, Japanese, Korean, Indonesian, Vietnamese, Dutch, Polish, Ukrainian, Greek, Swedish, Romanian, Czech, Hungarian) and needs no installed voices. Other languages, and the Custom persona, keep using system voices.

**Speaker's voice** (the `hammurabi-windows-gpu.zip` build) reads each caption in the voice of whoever is speaking, so a translated film sounds like its actors. Set **Voice engine** to **Speaker's voice**, agree that the voice model is for personal, non-commercial use only, and press **Download** (about 945 MB, once). It learns the voice from the first finished caption with at least 3 s of speech (up to 6 s of it, with that caption's own transcript), learns it again every minute so it follows whoever talks now, and reads in a voice of its own until then. The tray's **Voice** submenu has **Speaker's voice** and **Learn the voice again**. It runs [OmniVoice](https://huggingface.co/k2-fsa/OmniVoice) through [omnivoice.cpp](https://github.com/ServeurpersoCom/omnivoice.cpp) on the GPU (Vulkan: NVIDIA, AMD or Intel), about 0.5 to 1 s per sentence on an RTX 5080. Without a Vulkan GPU, or before the download, Supertonic 3 or system voices read instead. Only use it for voices you may use, and never to pass yourself off as someone.

**Voice personas** give Read aloud a character. In Settings > Read aloud (or the tray's **Voice** submenu), pick Lale (clear and neutral, the default), Ayla (bright and friendly), Deniz (calm and steady), Ateş (warm and deep), Bilge (slow and patient) or Rüzgar (quick, keeps up with fast talkers), and press its play button to hear a sample. Each persona uses an installed voice of its preferred gender for the language (any voice for it otherwise) at its own speed and pitch, so with only one voice installed they differ by speed and pitch alone. With Supertonic 3 each persona has its own voice, the same one it has in Babil. **Custom** keeps your own voice and speed, and is where you start if you had already picked one. Everything stays on your computer.

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
| Supertonic 3 | 210 MB | Neural voices for Read aloud |
| OmniVoice Q8_0 | 945 MB | Speaker's voice (GPU build); personal, non-commercial use only |

Config lives in `%APPDATA%\menzo\Hammurabi\config` on Windows (the OS equivalent elsewhere), models in the cache folder (override with `HAMMURABI_MODELS_DIR`). Settings has **Open models folder** and **Open logs**.

## Privacy

Audio is processed locally and never uploaded. Read aloud uses your OS's offline voices or Supertonic 3, which also runs on your computer. Speaker's voice learns the voice on your computer from the last few seconds of captured audio and keeps it in memory only; nothing is saved to disk. No telemetry. Network use is limited to model downloads from Hugging Face, a GitHub release check, and the LLM URL you configure.

## Build from source

Needs stable [Rust](https://rustup.rs) plus MSVC Build Tools (Windows), Xcode CLT (macOS), or on Linux:

```sh
sudo apt install pkg-config libasound2-dev libxkbcommon-dev libx11-dev libxcursor-dev libxrandr-dev libxi-dev libgl1-mesa-dev libwayland-dev libdbus-1-dev
```

```sh
cargo build --release                    # CPU
cargo build --release --features metal   # Apple GPU
cargo build --release --features cuda    # NVIDIA, needs CUDA toolkit 12.8+
cargo build --release --features omnivoice   # Windows only: Speaker's voice on a Vulkan GPU
```

For `omnivoice`, run `git submodule update --init --recursive` first (it builds `third_party/omnivoice.cpp` and its ggml), and install [CMake](https://cmake.org) and the [Vulkan SDK](https://vulkan.lunarg.com/sdk/home#windows) with `VULKAN_SDK` set (the installer sets it) and `cmake` on `PATH`. The C++ part builds into `target/ov-release` to stay under Windows' 260-character path limit; keep the checkout path short too. The result links statically and needs only a GPU driver with Vulkan.

For CUDA on Windows, build from a Visual Studio developer prompt with `CUDA_PATH` set and `CUDA_COMPUTE_CAP` set to your GPU (120 = RTX 50, 89 = RTX 40, 86 = RTX 30). Keep the toolkit's `bin` on `PATH` or copy `cudart64_12`, `cublas64_12`, `cublasLt64_12` and `curand64_10` DLLs next to the exe. See [CONTRIBUTING.md](CONTRIBUTING.md) for tests.

## CLI

`hammurabi-cli` ships with every build:

```sh
hammurabi-cli devices                                    # list audio devices
hammurabi-cli record --source system --secs 10 out.wav   # record system audio
hammurabi-cli transcribe out.wav --translate tr          # caption + translate a WAV
hammurabi-cli download base                              # tiny, base, small, turbo, madlad, supertonic
hammurabi-cli download omnivoice --accept-license        # GPU build: Speaker's voice (non-commercial)
hammurabi-cli voices --lang tr                           # voices (with gender) and each persona's pick
hammurabi-cli speak --lang tr "Merhaba dünya"            # read text aloud
hammurabi-cli speak --lang tr --persona ayla "Merhaba"   # read it as a persona
hammurabi-cli speak --lang tr --persona ayla --engine supertonic --out tr.wav "Merhaba"   # Supertonic to a WAV
hammurabi-cli speak --engine omnivoice --lang tr --ref ref.wav --ref-text "What ref.wav says" --out out.wav "Merhaba"   # in ref.wav's voice
hammurabi-cli transcribe talk.wav --realtime --translate tr --speak omnivoice   # read the translation in the speaker's voice
```

## Limitations

- Exclusive fullscreen games on Windows are not captured and cover the overlay
- RTL and Indic scripts are not shaped perfectly
- macOS older than 14.6 needs [BlackHole](https://existential.audio/blackhole/) for system audio
- Prebuilt CUDA supports RTX 50 only; build your own for other cards
- Speaker's voice is Windows only and needs a GPU with Vulkan and about 1 to 2.5 GB of free VRAM. Its first start on a new install compiles GPU shaders for up to about 20 s. The OmniVoice weights are CC-BY-NC-4.0, so it is for personal, non-commercial use only

Bugs and logs: [open an issue](https://github.com/canmenzo/hammurabi/issues/new/choose).

## License

[MIT](LICENSE). Whisper (MIT), MADLAD-400 (Apache-2.0), candle (MIT/Apache-2.0), ONNX Runtime (MIT), Phosphor Icons (MIT).

The downloadable voices have their own license: [Supertonic 3](https://huggingface.co/supertone-oss-archive/supertonic-3) by Supertone (22 languages) is OpenRAIL-M. Hammurabi downloads a [copy with an 8-bit vector estimator](https://huggingface.co/payam1394/traxlate-supertonic-3-q8ve), or the original if that copy is gone. Hammurabi's Supertonic code is ported from Supertone's MIT sample code. Speaker's voice uses [OmniVoice](https://huggingface.co/k2-fsa/OmniVoice) by k2-fsa (code Apache-2.0, weights [CC-BY-NC-4.0](https://huggingface.co/k2-fsa/OmniVoice#license), for personal, non-commercial use only) in the [GGUF conversion](https://huggingface.co/Serveurperso/OmniVoice-GGUF) for omnivoice.cpp (MIT) and ggml (MIT), with the Higgs Audio v2 codec (Apache-2.0). See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md), [CHANGELOG.md](CHANGELOG.md), [SECURITY.md](SECURITY.md).
