# Changelog

All notable changes to Hammurabi are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [0.4.0] - 2026-10-23

### Added

- Supertonic 3 neural voices for Read aloud, the same voices the Babil extension uses. Pick **Voice engine: Supertonic 3** in Settings > Read aloud and download it once (about 210 MB, resumable, checked by SHA-256). It runs locally on the CPU through ONNX Runtime and reads 22 languages; each persona gets its own Supertonic voice and speed (Lale F3, Ayla F2, Deniz M4, Ateş M2, Bilge F5, Rüzgar M1). Other languages, the Custom persona, and Supertonic before it is downloaded use system voices as before.
- `hammurabi-cli download supertonic`, and `hammurabi-cli speak --engine supertonic` with `--out file.wav` to write the audio instead of playing it. It prints the synthesis time and real-time factor; `--verbose` logs it for played speech.
- Speaker's voice, a new opt-in Read aloud engine in the Windows GPU build (`hammurabi-windows-gpu.zip`, cargo feature `omnivoice`): it reads each caption in the voice of the person speaking, using OmniVoice through omnivoice.cpp on the GPU (GGML Vulkan, so NVIDIA, AMD and Intel). It learns the voice from the first finished caption with at least 3 s of speech, pairing up to 6 s of it with that caption's own transcript, and again every minute; until then it reads in a voice of its own. About 0.5 to 1 s per sentence on an RTX 5080 at 16 steps. Pick it in Settings > Read aloud or the tray's Voice submenu, which also has Learn the voice again. Without a Vulkan GPU or before its download it falls back to Supertonic 3 or system voices.
- The OmniVoice download (about 945 MB, resumable, checked by SHA-256) is CC-BY-NC-4.0, for personal, non-commercial use only, and starts only after you agree to that once in Settings or with `hammurabi-cli download omnivoice --accept-license`.
- `hammurabi-cli speak --engine omnivoice` with `--ref voice.wav --ref-text "..."` and `--steps`, and `hammurabi-cli transcribe --realtime --speak <engine>` to hear the captions read aloud as the app would.

## [0.3.0] - 2026-10-22

### Added

- Voice personas for Read aloud: Lale (the default), Ayla, Deniz, Ateş, Bilge and Rüzgar, each with a character and a play button in Settings > Read aloud that speaks a sample. A persona picks an installed voice of its preferred gender for the language, falling back to any voice for it, and applies its own speed and pitch. With only one installed voice for a language, personas differ by speed and pitch alone.
- Custom persona that keeps your own voice and speed. Configs that already had a voice or speed picked start on Custom.
- A Voice submenu in the tray menu to switch persona.
- `hammurabi-cli speak --persona <id>`. `hammurabi-cli voices` now shows each voice's gender, and with `--lang` the voice each persona gets.

## [0.2.0] - 2026-10-22

### Added

- Read aloud: speaks each finished caption with the voices built into the OS (WinRT voices on Windows, AVSpeechSynthesizer on macOS, speech-dispatcher or espeak-ng on Linux). It reads the translation when translation is on, in the caption's language, and never the live line. Off by default; turn it on from the tray menu or Settings, where you can also pick a voice and the speed.
- Hammurabi stops listening while it reads a caption and for a moment after, so its own voice is not captioned and read again. Anything said in the audio during that time is skipped.
- When speech falls behind, the oldest waiting captions are dropped so it stays close to real time.
- `hammurabi-cli voices` lists the installed voices and `hammurabi-cli speak` reads text aloud.

## [0.1.1] - 2026-10-22

### Fixed

- Settings window no longer shows black bars along the right and bottom edges.
- On GPU builds, Settings shows the GPU and the turbo model as soon as it opens, instead of CPU and base until captions first start.
- Hotkeys in Settings read `Alt+Shift+T` instead of `alt+shift+KeyT`.

## [0.1.0] - 2026-10-22

First public release.

### Added

- System tray app for Windows, macOS and Linux that captions system audio, the microphone, or both.
- Local speech recognition with Whisper (tiny, base, small, turbo, or Auto: turbo on GPU, base on CPU).
- Optional translation with the built-in MADLAD-400 3B model, or a local LLM through any OpenAI-compatible server (Ollama, LM Studio).
- Optional Windows Live Captions speech engine on Windows 11 22H2 or newer.
- Start at login, a tray notice when a new release is out, and an Open logs button in Settings.
- Always-on-top caption overlay with adjustable font, size, colors, background and position.
- Global hotkeys: `Alt+Shift+T` starts or stops captions, `Alt+Shift+M` moves the overlay.
- Settings window for audio devices, speech model, translation and caption style.
- First-run model downloads that resume after interruptions, with progress in the overlay.
- `hammurabi-cli` for listing devices, recording, transcribing WAV files and downloading models.
- Release builds: Windows x64 (CPU), Windows CUDA for NVIDIA RTX 50 series, macOS Apple silicon (Metal) and Linux x64.

[Unreleased]: https://github.com/canmenzo/hammurabi/compare/v0.3.0...HEAD
[0.4.0]: https://github.com/canmenzo/hammurabi/releases/tag/v0.4.0
[0.3.0]: https://github.com/canmenzo/hammurabi/releases/tag/v0.3.0
[0.2.0]: https://github.com/canmenzo/hammurabi/releases/tag/v0.2.0
[0.1.1]: https://github.com/canmenzo/hammurabi/releases/tag/v0.1.1
[0.1.0]: https://github.com/canmenzo/hammurabi/releases/tag/v0.1.0
