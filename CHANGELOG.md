# Changelog

All notable changes to Hammurabi are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

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

[Unreleased]: https://github.com/canmenzo/hammurabi/compare/v0.1.1...HEAD
[0.2.0]: https://github.com/canmenzo/hammurabi/releases/tag/v0.2.0
[0.1.1]: https://github.com/canmenzo/hammurabi/releases/tag/v0.1.1
[0.1.0]: https://github.com/canmenzo/hammurabi/releases/tag/v0.1.0
