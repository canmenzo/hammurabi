# Contributing to Hammurabi

Thanks for helping. Bug reports, testing on macOS and Linux, and small focused pull requests are all welcome.

## Reporting bugs

Open an issue with the bug report form. It asks for your OS, which build you run (CPU, CUDA or Metal), the speech model, and a log excerpt. Those four things answer most questions before they are asked.

Security problems go through a private advisory instead. See [SECURITY.md](SECURITY.md).

## Building

You need the [Rust toolchain](https://rustup.rs) (stable) plus MSVC Build Tools (Windows), Xcode CLT (macOS), or on Linux:

```sh
sudo apt install pkg-config libasound2-dev libxkbcommon-dev libx11-dev libxcursor-dev libxrandr-dev libxi-dev libgl1-mesa-dev libwayland-dev libdbus-1-dev
```

```sh
cargo build --release          # app and hammurabi-cli
cargo run --release            # run the tray app
cargo run --release --example overlay_demo    # the caption overlay with scripted lines
cargo run --release --example settings_demo   # the settings window with fake data
```

The Windows installer wraps a folder holding the files to install, and downloads Inno Setup the first time:

```powershell
packaging/windows/build-installer.ps1 -Stage dist/hammurabi-windows-x64   # writes dist/hammurabi-windows-x64-setup.exe
```

GPU builds:

```sh
cargo build --release --features metal       # Apple GPU
cargo build --release --features cuda        # NVIDIA, needs CUDA toolkit 12.8+
cargo build --release --features omnivoice   # Windows only: Speaker's voice on a Vulkan GPU
```

For `omnivoice`, run `git submodule update --init --recursive` first (it builds `third_party/omnivoice.cpp` and its ggml), and install [CMake](https://cmake.org) and the [Vulkan SDK](https://vulkan.lunarg.com/sdk/home#windows) with `VULKAN_SDK` set (the installer sets it) and `cmake` on `PATH`. The C++ part builds into `target/ov-release` to stay under Windows' 260-character path limit; keep the checkout path short too. The result links statically and needs only a GPU driver with Vulkan.

For CUDA on Windows, build from a Visual Studio developer prompt with `CUDA_PATH` set and `CUDA_COMPUTE_CAP` set to your GPU (120 = RTX 50, 89 = RTX 40, 86 = RTX 30). Keep the toolkit's `bin` on `PATH` or copy `cudart64_12`, `cublas64_12`, `cublasLt64_12` and `curand64_10` DLLs next to the exe.

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

## Testing

CI runs these on Windows, macOS and Linux. Run them before you open a pull request:

```sh
cargo fmt --check
cargo clippy --all-targets
cargo test --release
```

Some tests need downloaded models and are skipped when they are missing. `hammurabi-cli transcribe some.wav --realtime` is a quick way to check the live pipeline without the UI.

## Pull requests

- Keep each pull request to one change, and describe what it fixes and how you tested it (OS, build, model).
- Match the existing style: `cargo fmt`, no new warnings from clippy, comments only where the logic is not obvious.
- Do not add new dependencies or network calls without explaining why in the pull request. Hammurabi promises that audio never leaves the computer; anything that touches the network gets extra review.
- UI changes: include a screenshot.
- By contributing you agree that your work is released under the [MIT License](LICENSE).
