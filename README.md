# Hammurabi

**Live captions and translation for anything your computer plays or hears. Fully local.**

[![The Hammurabi caption overlay over a video](https://hammurabi.menzo.dev/img/overlay.webp)](https://hammurabi.menzo.dev)

- 💬 Caption system audio, your microphone, or both
- 🌍 Translate into 26 languages
- 🔊 Hear captions read aloud in a natural voice
- 🔒 Audio stays on your computer. Free, with no account or API key

## 📦 Download

| Your computer | File |
|---|---|
| Windows 10/11 | [`hammurabi-windows-x64-setup.exe`](https://github.com/canmenzo/hammurabi/releases/latest/download/hammurabi-windows-x64-setup.exe) |
| Windows + NVIDIA RTX 50 series | [`hammurabi-windows-cuda-setup.exe`](https://github.com/canmenzo/hammurabi/releases/latest/download/hammurabi-windows-cuda-setup.exe) |
| Windows + any GPU, reads in the speaker's voice | [`hammurabi-windows-gpu-setup.exe`](https://github.com/canmenzo/hammurabi/releases/latest/download/hammurabi-windows-gpu-setup.exe) |
| macOS, Apple silicon | [`hammurabi-macos-arm64.zip`](https://github.com/canmenzo/hammurabi/releases/latest/download/hammurabi-macos-arm64.zip) |
| Linux x64 | [`hammurabi-linux-x64.tar.gz`](https://github.com/canmenzo/hammurabi/releases/latest/download/hammurabi-linux-x64.tar.gz) |

The builds are not code signed yet:

- **Windows:** run the installer. It installs for your user, no administrator rights needed. On "Windows protected your PC", click **More info > Run anyway**.
- **macOS:** move `Hammurabi.app` to Applications, run `xattr -dr com.apple.quarantine /Applications/Hammurabi.app`, and allow **System Audio Recording** when asked.
- **Linux:** run `tar -xzf hammurabi-linux-x64.tar.gz && ./hammurabi-linux-x64/hammurabi`. Use X11, and PulseAudio or `pipewire-pulse` for system audio.

## ▶️ Use

Press `Alt+Shift+T` to start or stop captions, and drag a caption to move them. Right-click the tray icon for everything else.

The first start downloads a speech model (up to about 1.6 GB). Translation downloads another 1.7 GB, or 5 to 7 GB for the Natural translator.

## 🎥 Streaming

To put captions on a stream, turn on **Settings > Captions > Captions as a web page** and add `http://127.0.0.1:7460` to OBS as a Browser source. Add `?scale=1.5` to the address to make them larger, and `&bottom=15` to lift them above a game's own subtitles.

For games and films, pick the **Natural** translator in Settings > Translate (Windows, needs a graphics card): it reads slang and tone where the fast one translates word for word.

Where every word counts and the graphics card has room, pick the **Large** speech model in Settings > Listen: it hears fast, slangy talk noticeably better, about 0.4 s later.

For a console on a capture card, set **Settings > Listen** to Microphone and pick the card, so that Hammurabi hears the game itself and not what the PC plays.

Captions share the graphics card with the stream's encoders. One NVENC encode, even at the slowest preset with two passes, costs them nothing. Several at once with look-ahead or adaptive quantization on (both run on the card's compute engine, like the speech model) made captions two to four times later in our tests and cost the stream frames, so send one encode to every site where the streaming app can, and leave those two options off on any extra ones.

Captions follow speech by a bit over half a second. A stream can hide that: delay the game in OBS by as much (a Render Delay filter on its video, Sync Offset under Advanced Audio Properties for its sound), and viewers get each caption as the line is said. Start Hammurabi before going live, since loading its models is the one moment it can cost a stream a few frames.

## 📄 License

[MIT](LICENSE). Some downloads have their own licenses:

- [Gemma 4](https://huggingface.co/google/gemma-4-12B-it-qat-q4_0-gguf) (Natural translator): Apache-2.0
- [Supertonic 3](https://huggingface.co/supertone-oss-archive/supertonic-3): OpenRAIL-M
- [OmniVoice](https://huggingface.co/k2-fsa/OmniVoice) (speaker's voice): personal, non-commercial use only

See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for the rest.
