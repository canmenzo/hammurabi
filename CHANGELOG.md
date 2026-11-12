# Changelog

All notable changes to Hammurabi are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- A Natural translator (Settings > Translate), on Windows: a language model (Gemma 4) that Hammurabi downloads and runs itself. It reads slang, idiom and tone where MADLAD translates word for word, and it is tuned for English to Turkish. It needs a graphics card with 8 GB of memory free (a 7 GB download), or 3.5 GB with its smaller model (5 GB).
- Captions as a web page, for streams: turn it on in Settings > Captions and add `http://127.0.0.1:7460` to OBS as a Browser source. The caption window itself is not part of a captured game or capture card.
- Lock in place (Settings > Captions): captions cannot be dragged, so a click on one always reaches the game under it.
- Words to expect (Settings > Listen): names and unusual words, so they are heard and spelled right.

### Changed

- Speech is found by a voice detector instead of by loudness. Talk under music, engines and gunfire is now captioned, and loud sound that is not speech is left alone. Sound too loud for the detector to make out is still recognized, and kept only when it reads as speech.
- Speech is recognized in longer stretches, with the quiet end of each word kept, so fewer words are cut or misheard.
- On a GPU, captions come sooner, by a tenth to a quarter of a second on a fast card. The open sentence is listened to up to ten times a second instead of twice, a run starts from what is already shown instead of hearing it again, and a fifth of a second of quiet is enough to end a sentence that two runs agree on.
- A caption comes with its translation instead of a moment before it, a third of a second sooner in all: a sentence is translated while it is still being said, and the translator gives up one that was heard differently since.
- A caption that grows keeps the translation it has. Only the sentence added is translated, so what is on the screen is not worded anew.
- The Natural translator lets everything else on the graphics card go first: speech recognition, which ran three to five times slower while a translation was written, a game, a stream being encoded. When that leaves it nothing, it takes its turn like before.
- Each sentence is a caption of its own, and one that follows within a second and a half joins the caption before it instead of pushing it away (not while captions are read aloud). A speaker cutting in ends the other's unfinished sentence.
- A translation that has waited four seconds for its turn is skipped; before, the third in line was, however fast the translator.
- With Show original text off, captions show translations only. The original no longer flashes first.
- The speech model is warmed up while it loads, so the first captions are not late.
- On NVIDIA cards the speech model runs in half precision: it takes 2 GB of graphics memory instead of 4, and is faster.

### Fixed

- Captions could stop for good without saying so. Now the speech model is loaded again when it fails or stops answering, a lost audio device is waited for instead of giving up or switching to the microphone, and a failing translator is tried again after a pause instead of staying off.
- A translation that arrives after its caption left no longer pops up out of order.
- A sentence shown and then heard differently could hold back every caption after it until the speaker paused, and then show them all at once, some for the second time.
- Words on both sides of a trailed-off sentence ("maybe... ...Tanisha") ran together.
- Speech cut into two stretches could show the sentences around the cut twice, and so could a recognizer that started a stretch over.

## [0.5.0] - 2026-11-09

### Changed

- Click and hold any caption to drag it anywhere. The Move captions item, its shortcut and the dashed frame are gone. Reset position stays in Settings > Captions.
- With system voices and none installed for the language being read, Supertonic 3 reads instead, if it is downloaded.
- Voice personas now have the same names and descriptions as the Babil extension: English names by default, and Turkish or Spanish ones on those systems. Saved choices are kept.
- Settings is now split into pages (Listen, Translate, Read aloud, Captions, General) with the site's colors. A header shows whether captions are on, has a Start/Stop button, shows warnings, and says what the first start will download.
- On Windows 11, system audio keeps being captioned while a voice reads: Hammurabi leaves its own sound out of what it captures. Everywhere, the same goes when the voice plays on another device than the one captioned. Otherwise, and for the microphone, listening still pauses.
- Read aloud no longer drops captions as soon as it falls behind: waiting captions are read together and up to 30% faster.
- Speech already in the target language is no longer read over itself. Turn this back on in Settings > Read aloud.
- On Windows, left-clicking the tray icon opens Settings; the menu stays on the right button. Tray submenus show the current source, target language and voice, and are disabled when their feature is off.

### Added

- A Windows installer for each Windows build. It installs for your user with no administrator rights, adds Start Menu and desktop shortcuts, upgrades in place, and removes itself from Settings > Apps. It offers to delete your settings and models on the way out. The portable zips stay.
- Read aloud volume, and a stop button on voice samples.
- Read aloud can play on its own speaker or headset (Settings > Read aloud > Read on). On macOS, system voices then keep their speed but lose their pitch; on Linux they need `espeak-ng`.
- Windows: other apps can be turned down while a voice reads (Settings > Read aloud > Other apps while reading). They come back when it ends, unless you moved their volume meanwhile, and after a crash on the next start.
- The Custom persona can use any of the 10 Supertonic 3 voices.
- Voice samples in every supported language.
- Shortcuts can be changed in Settings > General.
- A sample caption on screen while Settings > Captions is open, plus Move captions, Reset position and Reset look buttons.
- Settings opens with a short welcome on first launch.

### Fixed

- Windows: captions stayed invisible until Move captions had been used once, because Windows does not show a click-through window whose layering was never set up.
- If the engine ever crashes, it now restarts and captions carry on. Before, Start and every setting silently did nothing until the app was restarted.
- Opening Settings while captions were off crashed the app, so the tray icon vanished. This hit every tray click and the Settings menu item.
- Starting Hammurabi now opens Settings, and starting it again brings up the running copy instead of doing nothing. `--background`, which Start at login now uses, starts it with no window.
- macOS: opening Hammurabi again from Finder, Spotlight or Launchpad while it runs now brings up Settings. Before, nothing happened, because macOS does not start a second copy.
- On Windows 11, the invisible caption window no longer shows a faint outline on screen.
- Settings no longer flashes a large white window as it opens.
- Crashes are now written to the log.
- Supertonic 3 audio fades at each end, to avoid clicks.
- Captions could stop for good until the next start when the speech model revised a line to something shorter than what was already on screen.
- Linux: the end of every reading was cut off, up to a few seconds of it, because playback stopped while PulseAudio still had the audio buffered.
- Speaker's voice now reads faster when captions pile up, like the other voices.
- Settings no longer warns about a missing system voice when Supertonic 3 or Speaker's voice reads that language.

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

[Unreleased]: https://github.com/canmenzo/hammurabi/compare/v0.5.0...HEAD
[0.5.0]: https://github.com/canmenzo/hammurabi/releases/tag/v0.5.0
[0.4.0]: https://github.com/canmenzo/hammurabi/releases/tag/v0.4.0
[0.3.0]: https://github.com/canmenzo/hammurabi/releases/tag/v0.3.0
[0.2.0]: https://github.com/canmenzo/hammurabi/releases/tag/v0.2.0
[0.1.1]: https://github.com/canmenzo/hammurabi/releases/tag/v0.1.1
[0.1.0]: https://github.com/canmenzo/hammurabi/releases/tag/v0.1.0
