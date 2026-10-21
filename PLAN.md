# Hammurabi implementation plan

Native, tray-resident port of Babil (live local captions + translation). Crate/binary `hammurabi`. Nobody touches `docs/` (the website, owned elsewhere). Reference projects SakiRinn/LiveCaptions-Translator and KazKozDev/live-translation have no license: ideas only, no code.

## 0. Verified facts (Oct 2026)
- Versions: candle 0.11.0, cpal 0.18.2, eframe/egui 0.37.0 (winit 0.30.13, glutin 0.32), tray-icon 0.26.1, muda 0.21, global-hotkey 0.9.0, rubato 5.0.1, ureq 3.4.2, directories 6, toml 1.x. hf-hub 1.0 rejected (async reqwest/tokio).
- cpal 0.18.2 loopback:
  - Windows: building an input stream on a render device sets AUDCLNT_STREAMFLAGS_LOOPBACK, so `default_output_device()` + `build_input_stream` = WASAPI loopback.
  - macOS: native loopback via Core Audio process taps (macOS > 14.6).
  - Linux: `pulseaudio` feature (pure-Rust protocol client, no libpulse) lists sinks and sources including monitors.
  - `Device::name()` removed; use `id()` and `description()`.
- candle-transformers 0.11 has `whisper::{model, quantized_model, audio}`, `quantized_t5`, `t5`, `marian`; no m2m100/NLLB/VAD. The Whisper encoder narrows the positional embedding to the input length. `pcm_to_mel(cfg, samples, filters)` needs the mel filter bytes from candle-examples.
- candle-core 0.11 depends on `tokenizers ^0.22` with `onig`: pin tokenizers 0.22, not 0.23.
- Native C (onig, ring) builds via `cc` with VS 2022 BuildTools (present). No cmake/clang needed. CUDA toolkit NOT installed.
- eframe 0.37: `App::ui(&mut self, ui: &mut Ui, frame)` replaces `update`; `App::logic(&mut self, ctx, frame)` runs even while hidden if `request_repaint()` was called; `ViewportCommand::RequestPaintWhileHidden` (one frame); ViewportBuilder has `with_transparent/with_always_on_top/with_mouse_passthrough/with_taskbar/with_has_shadow`. Default renderer is wgpu; `default_fonts` is no longer default; `system_fonts` feature gives CJK fallback.
- tray-icon 0.26 `ksni` feature: pure-Rust D-Bus SNI tray on its own thread (no GTK on Linux). Its `examples/egui.rs` builds the tray in the eframe creation closure on Win/mac.
- global-hotkey 0.9: manager must be created on the event-loop/main thread on Win/mac; Linux uses x11rb (X11 only).
- HF files: `openai/whisper-{tiny,base,small,large-v3-turbo}` (`model.safetensors`, `config.json`, `tokenizer.json`; base 290 MB, small 967 MB, turbo 1.62 GB); `lmz/candle-whisper` (`model-tiny-q80.gguf`, `config-tiny.json`, `tokenizer-tiny.json`); `jbochi/madlad400-3b-mt` (`model-q4k.gguf` 1.65 GB, `model-q3k.gguf` 1.26 GB, `tokenizer.json` 16.6 MB, `config.json`).
- Ollama and LM Studio are installed on the dev machine.

## 1. Decisions
| Area | Choice |
|---|---|
| ASR | candle Whisper with our own greedy decoder. Default `base` on CPU, `small` on GPU builds; also `tiny` (q80 GGUF) and `turbo`. |
| Built-in translation | MADLAD-400-3B q4k via `candle_transformers::models::quantized_t5`. One multilingual model, Apache-2.0. |
| Optional translation | OpenAI-compatible local endpoint (Ollama `http://localhost:11434/v1`, LM Studio `:1234/v1`), last 3 sentences as context. |
| Whisper X->EN translate | Not used in phase 1. |
| VAD/chunking | Port of the extension's RMS gate + adaptive noise floor, plus Whisper no-speech and language confidence. |
| Downloads | Own `ureq` downloader: `huggingface.co/{repo}/resolve/{rev}/{file}`, `.part` + Range resume, progress, cancel. |
| GUI | eframe 0.37, **glow** renderer. Overlay = root viewport. Settings = immediate child viewport. |
| Tray | tray-icon + muda; created in eframe creation closure on Win/mac; `ksni` on Linux. |
| Audio | cpal 0.18 (`pulseaudio` feature on Linux) + rubato 5 Fft resample to 16 kHz mono. |
| macOS system audio | cpal process tap on 14.6+; older macOS: documented BlackHole fallback (pick it as the mic device). |
| Linux system audio | PulseAudio host, monitor of default sink (works on PipeWire via pipewire-pulse). No Pulse server: mic only via ALSA. |
| Linux display | Force winit X11 backend when `DISPLAY` is set. Pure Wayland documented as degraded. |
| Windows Live Captions | Optional phase-2 backend (`win-livecaptions` feature, `uiautomation` crate). |

## 2. Architecture
```
main thread (winit/eframe loop, glow)
  App { settings, engine: EngineHandle, tray, hotkeys, overlay: OverlayState, settings_win, app_rx }
   ├─ MenuEvent / TrayIconEvent / GlobalHotKeyEvent ::set_event_handler
   │     → app_tx.send(AppEvent) + ctx.request_repaint()
   ├─ App::logic(): drain app_rx + engine events, update tray, show/hide root (runs while hidden)
   └─ App::ui():    paint overlay; if settings open → ctx.show_viewport_immediate(...)
          │ EngineCommand (crossbeam unbounded)          ▲ EngineEvent + notifier() (= ctx.request_repaint)
          ▼                                              │
engine thread "hm-engine" (pipeline::run): single-threaded loop like offscreen.js
  owns: AudioCapture (cpal Streams created/dropped on this thread), Frames16k, Segmenter, Scheduler, Assembler
  select! { cmd_rx, audio_rx, asr.events, tr.events, default(timeout = next flush/idle deadline) }
     │ AsrRequest (max 1 in flight)            │ TranslateRequest
     ▼                                         ▼
asr thread "hm-asr"                       translate thread "hm-tr"
  ensure model → load                       ensure MADLAD or ping LLM → load
  WhisperEngine::transcribe                 Translate::translate
cpal callback threads → mono f32 at device rate → audio_tx.try_send(AudioChunk) (drop if full)
```
Engine rules:
- Silence padding: no audio for >300 ms while running → inject silent 100 ms frames (WASAPI loopback sends nothing when nothing plays).
- Both sources: per-source 16 kHz FIFO; frame = (a+b)*0.5; if one has >300 ms buffered and the other is empty, the empty one is silence.
- Workers survive Stop/Start; rebuilt only when model/translator changes. Shutdown drops senders and joins with timeout.

Event loop:
- `main()` builds `NativeOptions { renderer: Glow }`, viewport = overlay builder with `with_visible(false)`, `event_loop_builder` for platform tweaks (`platform.rs`).
- Creation closure (main thread, loop running) creates `GlobalHotKeyManager`, `TrayIconBuilder`, event handlers, `Engine::spawn(notifier)`. Linux: same; ksni has its own thread, no `gtk::init`.
- Root visibility: `visible = running || settings_open || move_mode || overlay.has_status()`. Idle with settings open → empty transparent click-through root. To show from `logic`: send `Visible(true)` + `RequestPaintWhileHidden`, then `request_repaint()`. Fallback if flaky: root always visible, empty, click-through.
- Quit: save config, `engine.shutdown(1s)`, `std::process::exit(0)`.

Overlay:
- `with_decorations(false)`, `with_transparent(true)`, `with_always_on_top()`, `with_mouse_passthrough(true)`, `with_taskbar(false)`, `with_resizable(false)`, `with_has_shadow(false)`, `with_active(false)`. `App::clear_color` returns `[0.0; 4]`; no panel fill.
- Fixed size: 80% monitor width, height for (cues + 1) blocks; text bottom-aligned. Monitor size from `ctx.input(|i| i.viewport().monitor_size)` on first visible pass, then `OuterPosition` + `InnerSize`. Default bottom centre, 8% of height from bottom. Saved `overlay_pos` (logical px) wins.
- Move mode (tray check "Move captions" or Alt+Shift+M): `MousePassthrough(false)`, dashed frame + hint "Drag to move · double-click to finish"; primary press → `StartDrag`; on exit save outer position, passthrough back on.
- macOS (`platform.rs`): first frame, NSWindow via raw window handle (`objc2-app-kit`): `collectionBehavior = CanJoinAllSpaces | FullScreenAuxiliary | Stationary`, `level = NSStatusWindowLevel`.
- Windows: works over borderless fullscreen, not exclusive fullscreen (documented).

## 3. Cargo.toml (only T0 and T10 edit it)
```toml
[package]
name = "hammurabi"
version = "0.1.0"
edition = "2024"
license = "MIT"
repository = "https://github.com/canmenzo/hammurabi"
default-run = "hammurabi"

[features]
default = []
cuda  = ["candle-core/cuda", "candle-nn/cuda", "candle-transformers/cuda"]   # CUDA >= 12.8 for RTX 50xx (sm_120)
cudnn = ["cuda", "candle-core/cudnn"]
metal = ["candle-core/metal", "candle-nn/metal", "candle-transformers/metal"]
win-livecaptions = ["dep:uiautomation"]

[dependencies]
candle-core = "0.11"
candle-nn = "0.11"
candle-transformers = "0.11"
tokenizers = { version = "0.22", default-features = false, features = ["onig"] }
cpal = "0.18"
rubato = "5"
audioadapter-buffers = "5.1"
hound = "3.5"
eframe = { version = "0.37", default-features = false, features = ["glow", "default_fonts", "system_fonts", "x11", "wayland"] }
tray-icon = { version = "0.26", default-features = false }
global-hotkey = "0.9"
ureq = "3"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "1"
directories = "6"
crossbeam-channel = "0.5"
anyhow = "1"
thiserror = "2"
log = "0.4"
env_logger = "0.11"
regex = "1"
clap = { version = "4", features = ["derive"] }
single-instance = "0.3"
sys-locale = "0.3"
open = "5"

[target.'cfg(target_os = "linux")'.dependencies]
tray-icon = { version = "0.26", default-features = false, features = ["ksni"] }
cpal = { version = "0.18", features = ["pulseaudio"] }

[target.'cfg(target_os = "macos")'.dependencies]
objc2 = "0.6"
objc2-app-kit = { version = "0.3", default-features = false, features = ["std", "NSWindow", "NSView", "NSResponder", "NSGraphics"] }

[target.'cfg(windows)'.dependencies]
uiautomation = { version = "0.25", optional = true }

[profile.dev.package."*"]
opt-level = 3

[profile.release]
lto = "thin"
```
Use winit via `eframe::egui_winit::winit`.

## 4. Layout and frozen interfaces
T0 creates every file with compiling stubs (`anyhow::bail!("unimplemented")` / empty). Each task fills only its own files.
```
assets/melfilters.bytes, assets/melfilters128.bytes   vendored from candle-examples/whisper (MIT/Apache-2.0)
THIRD_PARTY_NOTICES.md
src/main.rs            GUI entry: logging, single-instance, config::load, NativeOptions, run_native(ui::App::new)
src/lib.rs             pub mods
src/types.rs           [T0] cross-thread messages and traits
src/settings.rs        [T0] Settings, CaptionStyle, defaults, SOURCE_LANGS (extension's 28), TARGET_LANGS (extension's 26), lang_name()
src/device.rs          [T0] best_device(pref) -> (candle_core::Device, String)  cuda → metal → cpu
src/paths.rs           [T4] ProjectDirs::from("dev","menzo","Hammurabi"): config_file(), models_dir() (= cache_dir()/models)
src/config.rs          [T4] load() -> Settings (serde(default); corrupt → defaults + backup), save(&Settings) (atomic)
src/models/mod.rs      [T0 data, T4 logic] ModelId, ModelSpec table, ModelPaths, spec(), is_cached(), ensure()
src/models/download.rs [T4] fetch(repo, rev, file, dest, &mut progress, &AtomicBool cancel)
src/audio/mod.rs       [T1] AudioCapture::start/stop, list_devices()
src/audio/capture.rs   [T1] cpal stream builders, every SampleFormat → mono f32
src/audio/loopback.rs  [T1] per-OS system-audio device pick
src/audio/resample.rs  [T1] Frames16k: per-source rubato Fft + mixer → 1600-sample frames
src/asr/mod.rs         [T2] AsrWorker (thread, ensure+load, progress events)
src/asr/whisper.rs     [T2] WhisperEngine (normal/quantized enum, prompt logic, greedy decode)
src/asr/mel.rs         [T2] pad to 30 s, pcm_to_mel, filters by num_mel_bins (80/128)
src/asr/languages.rs   [T2] Whisper's 99 language codes
src/asr/livecaptions.rs[T12] cfg(all(windows, feature="win-livecaptions"))
src/translate/mod.rs   [T3] TranslatorWorker, dispatch, EN glossary
src/translate/madlad.rs[T3] quantized T5 MADLAD
src/translate/llm.rs   [T3b] OpenAI-compatible chat client
src/pipeline/mod.rs    [T5] Engine::spawn, EngineHandle, run loop
src/pipeline/segmenter.rs [T5] pure; port of onFrame/closeSegment
src/pipeline/scheduler.rs [T5] finals queue, partial cadence, backlog drop (pump)
src/pipeline/text.rs      [T5] clean(): fillers, interjections, hallucinations, repeats
src/pipeline/assembler.rs [T5] partial/finish/flush/emit/dedupe (now: Instant injected)
src/ui/mod.rs          [T6] App (eframe::App), AppEvent/UiAction routing, root visibility
src/ui/tray.rs         [T6] muda menu, MenuId→UiAction, sync checks/labels/tooltip
src/ui/hotkey.rs       [T6] register Alt+Shift+T / Alt+Shift+M, re-register on change
src/ui/platform.rs     [T6] configure_event_loop (X11 force, macOS Accessory), macOS window tweaks
src/ui/overlay.rs      [T7] OverlayState + paint + viewport_builder + layout_for_monitor
src/ui/settings_window.rs [T8] SettingsWindow::show
src/ui/icons.rs        [T9c] procedural RGBA icons (ziggurat, idle/active)
src/bin/hammurabi-cli.rs  [T10] devices | record | transcribe | translate | download
examples/record.rs [T1], examples/transcribe_wav.rs [T2], examples/translate.rs [T3], examples/overlay_demo.rs [T7]
tests/pipeline_sim.rs  [T5]
.github/workflows/ci.yml [T9a]   README.md [T9b]   LICENSE (MIT), .gitignore (target/, *.wav) [T0]
```

`types.rs` (frozen by T0):
```rust
pub const SAMPLE_RATE: u32 = 16_000;
pub const FRAME_SAMPLES: usize = 1_600;               // 100 ms
pub type Notifier = std::sync::Arc<dyn Fn() + Send + Sync>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)] pub enum AudioSource { System, Microphone, Both }
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum SourceKind { System, Microphone }
pub struct AudioChunk { pub source: SourceKind, pub samples: Vec<f32>, pub sample_rate: u32 } // mono, device rate

pub enum EngineCommand { Start(Settings), Stop, Reconfigure(Settings), Prefetch(ModelId), Shutdown }
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum EngineState { Idle, Loading, Running, Error }
#[derive(Clone, Copy, Debug)] pub enum StatusLevel { Info, Warning, Error }
pub struct Status { pub level: StatusLevel, pub text: String, pub ttl: Option<Duration> }
#[derive(Clone, Copy, Debug)] pub enum ProgressPhase { Downloading, Loading }
pub struct Progress { pub model: ModelId, pub phase: ProgressPhase, pub done: u64, pub total: Option<u64> }
pub enum CaptionUpdate {
    Final { id: u64, original: String, translated: Option<String>, lang: String }, // sent twice: without, then with translation
    Live  { original: String, translated: Option<String>, lang: Option<String> },  // empty original = clear live line
    Clear,
}
pub enum EngineEvent { State(EngineState), Status(Status), Progress(Progress), Caption(CaptionUpdate), Stats { asr_ms: u32, device: String } }

pub struct AsrRequest  { pub id: u64, pub segment: u64, pub is_final: bool, pub audio: Vec<f32>, pub language: Option<String>, pub prior: HashMap<String, f32> }
pub struct AsrResult   { pub id: u64, pub text: String, pub language: String, pub no_speech: f32, pub lang_conf: f32, pub ms: u32, pub error: Option<String> }
pub enum   AsrEvent    { Progress(Progress), Ready { model: String, device: String }, Result(AsrResult), Error(String) }
pub struct TranslateRequest { pub id: u64, pub text: String, pub source: String, pub target: String, pub context: Vec<(String, String)> }
pub enum   TranslatorEvent  { Progress(Progress), Ready, Result { id: u64, text: Option<String> }, Error(String) }

pub trait Recognizer: Send { fn transcribe(&mut self, audio: &[f32], language: Option<&str>, prior: &HashMap<String, f32>) -> anyhow::Result<Transcript>; }
pub struct Transcript { pub text: String, pub language: String, pub no_speech: f32, pub lang_conf: f32 }
pub trait Translate: Send { fn translate(&mut self, text: &str, source: &str, target: &str, context: &[(String, String)]) -> anyhow::Result<String>; }
```

`settings.rs` (all `#[serde(default)]`):
- `Settings`: `source: AudioSource` (System); `input_device`, `output_device: Option<String>` (cpal device id; None = OS default); `spoken_language: Option<String>` (None = auto); `asr_backend: AsrBackend { Whisper, WindowsLiveCaptions }`; `model: ModelChoice { Auto, Tiny, Base, Small, Turbo }`; `translate: bool` (true); `target_language: String` (OS locale via sys-locale if in TARGET_LANGS, else "en"); `translator: TranslatorChoice { Builtin, Llm { base_url, model } }`; `instant: bool` (false); `caption: CaptionStyle`; `overlay_pos: Option<[f32; 2]>`; `hotkey_toggle: String` ("alt+shift+KeyT"), `hotkey_move: String` ("alt+shift+KeyM"), parsed with `str::parse::<HotKey>()`.
- `CaptionStyle`: `trans_size` 30, `orig_size` 20, `trans_color` "#ffffff", `orig_color` "#ffe066", `bg_opacity` 0.75, `show_original` true, `original_first` false, `cues` 2, `hold_secs` 4.0, `edge: Edge { None, Shadow, Outline }`.

Entry points (stubbed in T0):
- audio: `AudioCapture::start(source, input: Option<&str>, output: Option<&str>, tx: Sender<AudioChunk>) -> Result<AudioCapture>`, `stop(self)`; `list_devices() -> DeviceList { inputs, outputs: Vec<DeviceInfo{ id, name, is_default }>, system_audio: Result<(), String> }`; `Frames16k::new()`, `push(&mut self, AudioChunk, out: &mut Vec<Vec<f32>>)`, `push_silence(&mut self, out)`.
- asr: `AsrWorker::spawn(choice: ModelChoice) -> AsrWorker { pub events: Receiver<AsrEvent> }`, `submit(AsrRequest)`; `WhisperEngine::load(&ModelPaths, &Device) -> Result<Self>` + `impl Recognizer`.
- translate: `TranslatorWorker::spawn(choice: TranslatorChoice) -> TranslatorWorker { pub events }`, `submit(TranslateRequest)`.
- models: `ModelId { WhisperTiny, WhisperBase, WhisperSmall, WhisperTurbo, Madlad3bQ4k }`, `ModelSpec { repo, revision, files, approx_bytes, label }`, `ModelPaths { weights, config, tokenizer, quantized }`, `ensure(id, &mut dyn FnMut(u64, Option<u64>), &AtomicBool) -> Result<ModelPaths>`. Cache: `models_dir()/<repo with '/'→'--'>/<file>`.
- pipeline: `Engine::spawn(notifier: Notifier) -> EngineHandle`; `EngineHandle::{send(cmd), try_recv() -> Option<EngineEvent>, shutdown(timeout)}`.
- ui::overlay: `OverlayState::{apply(CaptionUpdate, &Settings, now), set_status(Status, now), next_deadline(), paint(&mut Ui, &CaptionStyle, move_mode) -> OverlayResponse, is_empty(), has_status()}`, `viewport_builder(&Settings)`, `layout_for_monitor(..)`.
- ui::settings_window: `SettingsWindow::show(&mut self, ctx, &mut Settings, &ModelsStatus) -> Vec<SettingsAction>`, `SettingsAction { Changed, Download(ModelId), OpenModelsFolder, RefreshDevices }`.

Model registry:
| Choice | Repo | Files | Size |
|---|---|---|---|
| tiny | lmz/candle-whisper | model-tiny-q80.gguf, config-tiny.json, tokenizer-tiny.json | ~40 MB |
| base | openai/whisper-base | model.safetensors, config.json, tokenizer.json | 290 MB |
| small | openai/whisper-small | same | 967 MB |
| turbo | openai/whisper-large-v3-turbo | same (128 mel bins) | 1.62 GB |
| MADLAD | jbochi/madlad400-3b-mt | model-q4k.gguf, config.json, tokenizer.json | 1.67 GB |

`Auto` = `small` if best_device is CUDA/Metal, else `base`. T4 pins revisions to commit SHAs.

## 5. Chunking / streaming / commit (port of babil/src/offscreen.js + asr-worker.js; MIT, the owner's own)
Segmenter:
- 100 ms frames at 16 kHz. `noise = rms < noise ? 0.9*noise + 0.1*rms : noise*1.002`; speech if `rms > max(0.004, 2.5*noise)`.
- Preroll 3 quiet frames.
- Close (port `onFrame` exactly): 3 quiet frames once length >= 8; 12 quiet frames regardless; at 50 frames cut hard at the quietest frame in the last 15.
- Frames after the cut start the next segment if any are loud. No overlap; `joinCarry` handles split words.

Scheduler (`pump`):
- One ASR job in flight. Finals first.
- Partial re-run of the open segment after 6 new frames if it has >= 3 speech frames.
- >2 finals waiting: drop oldest, keep its last partial text, status "Your computer can't keep up — choose a smaller model" at most every 10 s.
- Drop segments with < 3 speech frames. Ignore partials of closed segments (compare segment ids).

Whisper decode:
1. Pad PCM to 30 s (`N_SAMPLES`), `pcm_to_mel`, take exactly 3000 frames, `encoder.forward(mel, true)`.
2. Step 0 feeds `[SOT]` only. `no_speech` = softmax prob of `<|nospeech|>`/`<|nocaptions|>`. Over language tokens `lang_conf = (top1 + top2) / Σ`. Language = fixed one, else argmax of `ln p + ln(prior.get(lang).unwrap_or(0.01))`.
3. Force `[SOT, lang, <|transcribe|>, <|notimestamps|>]`, greedy decode. Suppress no-speech, all timestamp tokens, `config.suppress_tokens`, blank/EOT at the first sampled position. Feed the full sequence each step, `flush_kv_cache` true only at step 0. `max_new_tokens = min(128, ceil(secs*7)+8)`. Stop at EOT. Decode with `skip_special_tokens`.
4. Normal and quantized models behind one enum (`encoder_forward`, `decoder_forward`, `final_linear`, `reset_kv_cache`). F32 everywhere in phase 1.

`text::clean` (word-level; `regex` has no backrefs/lookahead):
- Empty if `no_speech > 0.5` or `lang_conf < 0.7`.
- Strip `[...]`, `(...)`, `*...*`, `♪♫`. Collapse a word repeated >3 times to 3. Drop filler tokens (keep their sentence-ending punctuation; en/tr/other lists from the extension). Drop results with no letters. Hallucination list (altyazı m.k., amara.org, "thanks for watching", "abone ol"). Drop interjection-only utterances. If `no_speech > 0.12` or `lang_conf < 0.85`, drop results of <= 2 words.

Assembler:
- Partials: a word is stable once two consecutive partials of the same segment agree (common prefix). Complete sentences in the stable words (ending `[.!?…。！？]` + closing quotes) are emitted immediately; record `s.emitted`, `s.lastEmitted`.
- Finals: skip already-emitted text by searching ±3 words around `emitted` for `lastEmitted`; `joinCarry` drops a duplicated boundary word (normalized); emit complete sentences; rest stays in `carry`.
- Live line: unfinished tail as `CaptionUpdate::Live`.
- Flush: after 1.5 s silence (select timeout) or carry > 30 words, emit carry as a sentence.
- Dedupe: same lowercased sentence within 5 s is dropped.
- Translation: each sentence is sent as `Final{translated: None}` plus a `TranslateRequest` with the last 3 (original, translated) pairs; result re-sends the same id with translation. source == target → original as translation. Live line translated only when `instant`, stale versions dropped.
- Language prior: finals with >= 3 words: multiply all by 0.7, add 0.3 to detected language. Reset when `spoken_language` changes.

Overlay display: last `cues` finals + live line at bottom. Each final shows `max(hold_secs, chars/15)` s, then 250 ms fade. Hide original when language == target.

## 6. Tasks
Wave 0: **T0 (opus)** scaffold + frozen interfaces: Cargo.toml exactly as §3, every file in §4 with compiling stubs, full `types.rs`, `settings.rs`, `device.rs`, ModelSpec table, mel bytes + notice, LICENSE, .gitignore. Done when `cargo build`, `cargo build --release`, `cargo test` pass. Commit.

Wave 1 (parallel; each task edits only its files):
| Task | Tier | Files | Done when |
|---|---|---|---|
| T1 Audio | opus | `audio/*`, `examples/record.rs` | `cargo run --release --example record -- --source system --secs 10 out.wav` while a video plays gives clear 16 kHz mono speech; mic and both work; device listing works; Linux/macOS cfg code compiles in CI. |
| T2 Whisper | opus | `asr/{mod,whisper,mel,languages}.rs`, `examples/transcribe_wav.rs` | Transcribes a WAV in 5 s chunks printing text, language, no_speech, lang_conf, ms for tiny/base/turbo; TTS test file correct. |
| T3 MADLAD + worker | opus | `translate/{mod,madlad}.rs`, `examples/translate.rs` | `--to tr "How are you, my friend?"` gives correct Turkish; prints tokens/s; `<2zh_Hant>` falls back to `<2zh>` if missing; EN glossary applied on MADLAD path. |
| T3b LLM translator | sonnet | `translate/llm.rs` | POST `{base}/chat/completions`, `stream:false`, 10 s timeout, temp 0.2, system prompt "subtitle translator; translate only the last line; output only the translation"; strips `<think>…</think>`; works with local Ollama. |
| T4 Downloads + config | sonnet | `models/{mod,download}.rs` (logic), `paths.rs`, `config.rs` | Resumable `.part` download with progress/cancel; is_cached/ensure; TOML round-trip tolerant of unknown/missing fields; unit tests. |
| T5 Pipeline | opus | `pipeline/*`, `tests/pipeline_sim.rs` | Unit tests for segmenter, text cleaning, assembler; sim test with fake Recognizer/Translate checks CaptionUpdate sequence. |
| T6 UI shell | opus | `main.rs`, `ui/{mod,tray,hotkey,platform}.rs` | Spike hidden root first. Tray: Start/Stop, Source (3 radio checks), Translate, Target language submenu (26), Move captions, Settings…, Quit. Hotkey, single instance, tooltip status, macOS collection behaviour, X11 forced on Linux. |
| T7 Overlay | sonnet | `ui/overlay.rs`, `examples/overlay_demo.rs` | Transparent, on top, click-through, bottom-aligned demo; move mode drags and saves; fades/holds; edge styles. |
| T8 Settings window | sonnet | `ui/settings_window.rs` | Audio, Speech (model + size + downloaded marker + Download + progress), Translation (toggle, target, Built-in / Local LLM URL + model), Captions, hotkey (read-only), Open models folder. Returns actions. |
| T9a CI | haiku | `.github/workflows/ci.yml` | windows/macos/ubuntu latest, fail-fast false, `dtolnay/rust-toolchain@stable`, `Swatinem/rust-cache@v2`; Linux apt: `pkg-config libasound2-dev libxkbcommon-dev libx11-dev libxcursor-dev libxrandr-dev libxi-dev libgl1-mesa-dev libwayland-dev libdbus-1-dev`; fmt --check, clippy --all-targets, build --release --all-targets, test --release; macOS also `--features metal`; upload binaries. No CUDA. |
| T9b README | haiku | `README.md` | Install/run, first-run download sizes, per-OS system audio (macOS 14.6+ permission, BlackHole fallback, Linux Pulse/PipeWire, Wayland limits), cuda/metal features, CUDA >= 12.8 note, translation options, limitations, license. |
| T9c Icons | haiku | `ui/icons.rs` | `fn icon_rgba(active: bool, size: u32) -> (Vec<u8>, u32, u32)`: stepped ziggurat, white idle, amber active. |

Wave 2: **T10 (opus)** integration + CLI: wire App/Engine/workers/settings; live Reconfigure; 500 ms debounced config save; first-run download flow (overlay status pill + settings progress; captions start untranslated while MADLAD downloads); `hammurabi-cli` (`devices`, `record`, `transcribe in.wav [--model] [--lang] [--translate] [--realtime]` through the real pipeline, `download`); CPU perf pass; CI green.

Phase 2 (optional): T12 Windows Live Captions backend (opus); T13 cosmic-text overlay for RTL/complex scripts (sonnet); T14 CUDA after installing CUDA 12.8+ (sonnet); T15 release workflow + macOS .app Info.plist (haiku).

Rules: only T0/T10 edit `Cargo.toml` and `lib.rs` (report needed deps). Nobody edits `types.rs` after T0 without coordinator approval. Nobody touches `docs/`.

## 7. Risks / checks
1. Builds without cmake/clang: only onig + ring via `cc`/MSVC. CUDA opt-in (needs toolkit; `CUDA_COMPUTE_CAP=120`, cl.exe on PATH).
2. Hidden root window in eframe 0.37: T6 spikes it; fallback specified.
3. Transparent glow window on NVIDIA/Windows: check overlay_demo; black bg = clear_color/panel fill wrong.
4. CPU speed: base ~0.3-0.6 s/job with 30 s padding; fallbacks tiny or encoder-trim behind a setting.
5. MADLAD CPU speed: target < 2 s/sentence; fallbacks q3k, LLM, off. ~2.2 GB RAM with base.
6. WASAPI gaps/odd formats: silence padding; convert every SampleFormat; check with record example.
7. Hallucination on music/silence: ported filters; check with a music clip.
8. Test audio: PowerShell `System.Speech.Synthesis.SpeechSynthesizer.SetOutputToWaveFile`. Model-dependent tests `#[ignore]`.
9. Alt+Shift switches keyboard layout on Windows: registration failure → status; configurable.
10. macOS/Linux runtime untested; CI compile only; README asks for testers.
11. Text rendering: CJK via system_fonts; RTL/Indic shaping imperfect until T13.
12. Licensing: Whisper MIT, MADLAD Apache-2.0, candle mel bytes MIT/Apache with notice, Babil MIT (own).
