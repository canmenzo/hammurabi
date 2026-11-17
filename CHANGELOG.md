# Changelog

All notable changes to Hammurabi are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.7.9] - 2026-11-17

### Added

- Subtitles on screen can show only the translation: with **Cover the game's subtitles** (Settings > Listen, under the picture of what is read; on by default) the translation stands in a black box right over the game's own subtitle and hides it, on the screen and on the captions web page. The box comes up with the subtitle, empty until its translation is in, follows it, and goes after the usual time on screen once the subtitle is gone.

## [0.7.8] - 2026-11-17

### Changed

- Settings recommends models by the graphics card in the computer, not only by the build. A card with room for it beside the speech model gets the Natural translator (the large one from about 9 GB free, the small one from 6.5 GB) instead of the Fast translator.

### Added

- The regular Windows build installed on an NVIDIA RTX 50 series card says in Settings that the RTX 50 build hears much faster, and switches to it in one click: the updater downloads and installs it over this copy, keeping settings and models, and restarts it. A copy that was not installed with the setup program links to the download instead.

## [0.7.7] - 2026-11-17

### Fixed

- Read aloud keeps up by time, not by count. A caption that waited more than 10 seconds to be read is skipped, so what is read stays with what is on screen however long each one is. Up to ten captions now wait their turn instead of six: a burst of short lines lost its oldest ones (14 in four minutes of fast GTA dialogue; none now).
- Read aloud no longer reads a line twice when the speech model gives it twice in a row ("Man. Fucked it. Man. Fucked it."): a line read in the last 10 seconds is not read again.
- A sentence shown early ("Get out.") and then heard to its end ("Get out of here.") is read once, whole, when its start has not been read yet; its caption is put right in place instead of a second caption under it. The translation of the start no longer shows on the whole sentence for a moment.
- When no system voice reads the caption language, the warning says that Natural voices can read it without one.

## [0.7.6] - 2026-11-17

### Fixed

- Hammurabi starts on a Windows PC without the Visual C++ Redistributable. It stopped with "MSVCP140.dll was not found"; the Windows builds now carry the four runtime files beside the program.

## [0.7.5] - 2026-11-17

### Added

- Hammurabi speaks Turkish, Spanish, Portuguese and Chinese as well as English: Settings, the tray menu, the welcome note and the messages over the captions. It opens in the system's language when it is one of these, and Settings > General has a Language choice.

### Fixed

- The CUDA build on a graphics card it was not built for (anything but the RTX 50 series) runs on the CPU instead of failing at the first caption.

## [0.7.4] - 2026-11-17

### Added

- Hammurabi updates itself. A copy installed with the Windows installer checks for a new version every 6 hours, downloads the installer for its own build, checks it against the release's checksums and installs it while captions are off. The installer's progress shows, Hammurabi starts again where it was, in the tray or with Settings open, and says which version it is now. Settings shows the download next to the version, and Settings > General has a Check for updates button. If an install fails, that version is offered by hand instead of tried again. The portable zips, macOS and Linux keep saying when a new version is out.
- Settings > General marks the speech model and translator that suit this PC.

### Changed

- On an English system, translation starts off, so a first start downloads only the speech model instead of 1.7 GB more.
- A model download is a slim bar with its percentage and megabytes, the same height as the Download button. Deleting a model asks first in a proper dialog that says how much space it frees.
- The status line under the captions switch wraps instead of cutting off what is below it, and lists downloads as "Downloading: speech model 2%, translation model 0%".
- Sizes no longer say "about", the words field says "Add a word", and the Settings window is titled Hammurabi with Settings as its heading.
- The update check reads the latest release from GitHub's release page instead of its API, which allows only 60 requests an hour from one address.

## [0.7.3] - 2026-11-16

### Added

- Subtitles on screen shows what it reads: under the subtitle area in Settings > Listen there is now a small live picture of the window or display, with the part that is read lit. Its edges can be dragged, so the area is set by looking instead of by guessing a percentage.
- The area has sides too. A video in a web page does not fill its window, and its subtitles are centred in the video, not in the window, so they were passed over as something standing to a side; with the sides dragged to the video's they are read.
- Subtitles on screen reads captions that grow word by word from a left edge, as a video site's automatic ones do. They are not centred and a row of them is not whole until the next begins, so they were passed over. Such captions are told by a row that grows and then moves up as the next starts under it; each row is then given on once, when the next one starts under it or when no word has joined it for a moment. A game's subtitles, which stand whole, are read as before.
- Hammurabi can be packaged for the Microsoft Store. A copy installed from there is updated by the Store, so it does not ask GitHub for new versions, and it starts at login through Windows' own startup setting. The site has a privacy policy and a code signing policy.
- A downloaded model can be deleted from Settings, with a second click to confirm, and Settings > General lists every model with what it takes to download or delete it. A download shows how many megabytes of how many are in, beside its percentage.
- Settings says whether captions keep up: how long a line takes to hear (Listen) and to translate (Translate), in orange with what helps when that is too long to keep up with speech. It stands where the model's name was repeated as its status.

### Changed

- Words to expect are tags: Enter or a comma adds what was typed, the cross on a tag removes it, and the list opens and closes. A long list in one line could not be read.
- While a display is read, only Hammurabi's captions are kept out of pictures of it. Its Settings window was kept out too, so it vanished from every screenshot.
- "Runs on" says what the speech model runs on in words ("Your NVIDIA graphics card") instead of "CUDA 0".
- Settings > Listen begins with the choice it hid as the third of three "speech engines": captions from listening, from Windows Live Captions, or from the subtitles on screen. What the chosen one does not use is not shown, and the language of the subtitles stands right under the choice.
- Settings speaks plainly. The translators are "Fast", "Natural" and "Your own AI server", each with what it is under it instead of in a hover; models are named by what they are for ("Fast translator", "Natural voices") and sized in gigabytes; status lines say "the speech model" where they gave a model's code name.
- While captions run, a translator or speech model that is not downloaded yet cannot be picked: it says "Download first" beside it. Picking one dropped the model in use and started a download of several gigabytes. The same holds for turning translation on and for going back to listening; the Natural translator can be picked with only its smaller model downloaded, and then uses that.
- The captions page for a streaming app shows the sample caption while Settings > Captions is open, until a real caption comes, so its size and place can be set before anyone speaks, and Settings says how to add it to OBS.
- A shortcut is Ctrl or Alt with a function key, or two of Ctrl, Alt and Shift with a letter, a digit or Space: Ctrl+C could be taken from every other app.

### Fixed

- Starting Subtitles on screen after Hammurabi had sat idle for hours took hundreds of megabytes at once, for silence nothing listened to.
- "Waiting for the window" and "is minimized" stayed on the captions after the window was back. A status in Settings stayed for hours after it had ended, and an error stayed over the game until captions were toggled: it now leaves after 20 seconds and stays in Settings.
- A translator that failed to load, for a download that stalled or a server not started yet, was not tried again while captions ran.
- One picture that could not be read, or the display's capture failing once, ended Subtitles on screen for good. The picture is passed over, and the display is waited for as a window is; only when nothing can be read for about five seconds does it stop with an error.
- The speech model stayed in the graphics card's memory after switching to Subtitles on screen, which uses none.
- Changing the subtitle language while subtitles were read reached the translator but not the reader.
- Stopping and starting captions quickly could leave Settings and the tray saying they were off while they ran.
- The oldest caption was cut off at the top when a status and long captions were up together. It is left out, and the newest is always shown.
- A word typed into Words to expect was dropped unless Enter was pressed, and recording a shortcut went on after its page was left.
- Subtitles on screen says so when Windows cannot read the chosen language, where it read it as another. The warning goes once the language is added in Windows.

## [0.7.2] - 2026-11-13

### Changed

- The Natural translator says what the speaker said. It used to turn the N-word into a friendly form of address or "that word", and a filter replaced it wherever the model had kept it; both are gone. Profanity and slurs keep their strength, nothing is softened or swapped for a politer word, and the N-word stays in the caption as it was said.
- Subtitles on screen says so when the window it reads is minimized, where it used to go quiet: a minimized window is not drawn, so nothing can be read from it until it is brought back.

### Fixed

- Subtitles on screen: a subtitle of two full rows came out untranslated beside a stream, because the language model takes longer over that many words than a line was given. A long subtitle is now translated sentence by sentence, the first shown while the rest is written, and a long sentence is given more time.
- Subtitles on screen no longer takes a number plate in the middle of the picture ("FC1988") for a subtitle: a line has to have a word made of letters.

## [0.7.1] - 2026-11-13

### Changed

- Subtitles on screen reads thin, off-white and coloured subtitles too, where the game sets them on a dark ground. It kept only what was white, which broke the letters of a game like Red Dead Redemption 2 ("where is Illy son, l)utch van cler Linde?"); each picture is now also read as it is, and how white the letters are decides which reading is followed. That game's subtitles come out whole.
- A subtitle that is up for under a second is no longer missed for never reading the same twice: it is given on as it goes, with what was read most. On the recorded game missions 7 of 290 subtitles are missed, from 12.
- A window that is closed, or not open yet when captions start, is waited for and read when it is there, instead of captions stopping with an error.

## [0.7.0] - 2026-11-13

### Added

- Subtitles on screen (Settings > Listen), on Windows: instead of listening, Hammurabi reads the subtitles a game or film shows and translates those. Pick the window to read (it may be covered by others, but not minimized) or the main display, and how far down the picture the subtitles are. Slang, names and talk under gunfire come out as the game wrote them. On three recorded game missions one word in nineteen of the subtitles was read wrong; held against what is said in those scenes, one word in ten is off, where listening has one in six with Turbo and one in eight with Large. A subtitle is read a tenth to half a second after it appears, usually before its line has been said, and no speech model runs, which leaves a stream's graphics card that much freer. It reads white subtitles at the bottom middle of the picture, in English or any language Windows has text recognition for. A window of any size is read at 1920 wide (one word in ten wrong from a window 1280 wide, one in fifteen from 2560). White subtitles over a white wall or sky are what it reads badly: where a game can put a dark background behind its subtitles, turn that on. While a display is read, Hammurabi's own windows are kept out of pictures taken of that display, so its captions are not read as subtitles.

## [0.6.0] - 2026-11-12

### Added

- A Natural translator (Settings > Translate), on Windows: a language model (Gemma 4) that Hammurabi downloads and runs itself. It reads slang, idiom and tone where MADLAD translates word for word, and it is tuned for English to Turkish. It needs a graphics card with 8 GB of memory free (a 7 GB download), or 3.5 GB with its smaller model (5 GB).
- Captions as a web page, for streams: turn it on in Settings > Captions and add `http://127.0.0.1:7460` to OBS as a Browser source. The caption window itself is not part of a captured game or capture card.
- Lock in place (Settings > Captions): captions cannot be dragged, so a click on one always reaches the game under it.
- Words to expect (Settings > Listen): names and unusual words, so they are heard and spelled right.
- A Large speech model (Settings > Listen): the full Whisper large-v3. In fast, slangy speech it hears about a third fewer words wrong than Turbo. Captions come about a tenth of a second later with it, and it takes 3.5 GB of graphics memory instead of 2 (a 3 GB download). When the graphics card is too busy for it and captions fall behind, Turbo takes over until another model is chosen.

### Changed

- Speech is found by a voice detector instead of by loudness. Talk under music, engines and gunfire is now captioned, and loud sound that is not speech is left alone. Sound too loud for the detector to make out is still recognized, and kept only when it reads as speech.
- Speech is recognized in longer stretches, with the quiet end of each word kept, so fewer words are cut or misheard. On a GPU a stretch now goes on through a breath (it ends after 0.6 s of quiet, not 0.3) and may be 20 s long: in our game scenes about one wrong word in twenty fewer, and captions no later.
- On a GPU, captions come sooner, by a tenth to a quarter of a second on a fast card. The open sentence is listened to up to ten times a second instead of twice, a run starts from what is already shown instead of hearing it again, and a fifth of a second of quiet is enough to end a sentence that two runs agree on.
- The recognizer no longer writes out again what it wrote a moment ago. It is shown the last run's words as a guess and takes a step only where it now hears something else, so a run takes a third to a half of the time for the same words: captions come up to 0.1 s sooner with Turbo and 0.2 to 0.3 s sooner with Large.
- A caption comes with its translation instead of a moment before it, a third of a second sooner in all: a sentence is translated while it is still being said, and the translator gives up one that was heard differently since.
- A caption that grows keeps the translation it has. Only the sentence added is translated, so what is on the screen is not worded anew.
- The Natural translator lets everything else on the graphics card go first: speech recognition, which ran three to five times slower while a translation was written, a game, a stream being encoded. When that leaves it nothing, it takes its turn like before.
- Each sentence is a caption of its own, and one that follows within a second and a half joins the caption before it instead of pushing it away (not while captions are read aloud). A speaker cutting in ends the other's unfinished sentence.
- A translation that has waited four seconds for its turn is skipped; before, the third in line was, however fast the translator.
- With Show original text off, captions show translations only. The original no longer flashes first.
- The speech model is warmed up while it loads, so the first captions are not late.
- Speech the recognizer is sure of word by word is kept even when its first impression was "no speech" or another language, which it often is over music.
- On NVIDIA cards the speech model runs in half precision: it takes 2 GB of graphics memory instead of 4, and is faster.

### Fixed

- Captions could stop for good without saying so. Now the speech model is loaded again when it fails or stops answering, a lost audio device is waited for instead of giving up or switching to the microphone, and a failing translator is tried again after a pause instead of staying off.
- A translation that arrives after its caption left no longer pops up out of order.
- A sentence shown and then heard differently could hold back every caption after it until the speaker paused, and then show them all at once, some for the second time.
- Words on both sides of a trailed-off sentence ("maybe... ...Tanisha") ran together.
- With the Natural translator or a local LLM, on NVIDIA cards, Hammurabi took half a gigabyte more memory for every hour of captions.
- While a caption faded in or out, its window could be painted thousands of times a second, which kept a processor core busy.
- A few words said twice in a row ("he don't wanna be a gangster, he wanna be a gangster") were taken for the recognizer going in circles, and the rest of what was said until the next pause was lost.
- In long talk without a pause, a whole stretch of speech could go missing now and then, more often on a busy graphics card.
- A sentence shown before it was over ("He is a risk.") came again in full after it ("He is a risk we cannot take."). The full one now takes its place in the caption.
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

[Unreleased]: https://github.com/canmenzo/hammurabi/compare/v0.7.3...HEAD
[0.7.3]: https://github.com/canmenzo/hammurabi/releases/tag/v0.7.3
[0.7.2]: https://github.com/canmenzo/hammurabi/releases/tag/v0.7.2
[0.7.1]: https://github.com/canmenzo/hammurabi/releases/tag/v0.7.1
[0.7.0]: https://github.com/canmenzo/hammurabi/releases/tag/v0.7.0
[0.6.0]: https://github.com/canmenzo/hammurabi/releases/tag/v0.6.0
[0.5.0]: https://github.com/canmenzo/hammurabi/releases/tag/v0.5.0
[0.4.0]: https://github.com/canmenzo/hammurabi/releases/tag/v0.4.0
[0.3.0]: https://github.com/canmenzo/hammurabi/releases/tag/v0.3.0
[0.2.0]: https://github.com/canmenzo/hammurabi/releases/tag/v0.2.0
[0.1.1]: https://github.com/canmenzo/hammurabi/releases/tag/v0.1.1
[0.1.0]: https://github.com/canmenzo/hammurabi/releases/tag/v0.1.0
