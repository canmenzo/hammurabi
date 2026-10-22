# Contributing to Hammurabi

Thanks for helping. Bug reports, testing on macOS and Linux, and small focused pull requests are all welcome.

## Reporting bugs

Open an issue with the bug report form. It asks for your OS, which build you run (CPU, CUDA or Metal), the speech model, and a log excerpt. Those four things answer most questions before they are asked.

Security problems go through a private advisory instead. See [SECURITY.md](SECURITY.md).

## Building

You need the [Rust toolchain](https://rustup.rs) (stable) and the platform dependencies listed in the README under [Build from source](README.md#build-from-source).

```sh
cargo build --release          # app and hammurabi-cli
cargo run --release            # run the tray app
cargo run --release --example overlay_demo    # the caption overlay with scripted lines
cargo run --release --example settings_demo   # the settings window with fake data
```

GPU builds use `--features metal` (macOS) or `--features cuda` (NVIDIA; see the README for the toolkit setup).

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
