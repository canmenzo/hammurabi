use eframe::egui;
use hammurabi::audio::{DeviceInfo, DeviceList};
use hammurabi::models::ModelId;
use hammurabi::settings::{AsrBackend, Settings, TranslatorChoice};
use hammurabi::tts::{Gender, Voice};
use hammurabi::ui::settings_window::{
    ModelsStatus, Page, SettingsAction, SettingsWindow, apply_theme,
};

struct Demo {
    win: SettingsWindow,
    settings: Settings,
    status: ModelsStatus,
}

impl eframe::App for Demo {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        for a in self.win.show(&ctx, &mut self.settings, &self.status) {
            match a {
                SettingsAction::Changed => println!("changed"),
                SettingsAction::Download(id) => println!("download {id:?}"),
                SettingsAction::OpenModelsFolder => println!("open folder"),
                SettingsAction::RefreshDevices => println!("refresh"),
                SettingsAction::ToggleAutostart => println!("toggle autostart"),
                SettingsAction::OpenLogs => println!("open logs"),
                SettingsAction::Preview(id) => println!("preview {id}"),
                _ => println!("other action"),
            }
        }
    }
}

fn dev(id: &str, name: &str, d: bool) -> DeviceInfo {
    DeviceInfo {
        id: id.into(),
        name: name.into(),
        is_default: d,
    }
}

fn main() -> eframe::Result {
    // Where the window opens, as "x,y": for a display other than the one being worked on.
    let at = std::env::var("HAMMURABI_DEMO_AT").ok().and_then(|at| {
        let (x, y) = at.split_once(',')?;
        Some([x.trim().parse().ok()?, y.trim().parse().ok()?])
    });
    // Its size, as "width,height": a tall one shows a whole page in one picture.
    let size = std::env::var("HAMMURABI_DEMO_SIZE").ok().and_then(|size| {
        let (w, h) = size.split_once(',')?;
        Some([w.trim().parse().ok()?, h.trim().parse().ok()?])
    });
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Hammurabi")
            .with_inner_size(size.unwrap_or([840.0, 640.0]))
            .with_position(at.unwrap_or([40.0, 20.0]))
            .with_active(false),
        ..Default::default()
    };
    eframe::run_native(
        "Settings demo",
        opts,
        Box::new(|cc| {
            apply_theme(&cc.egui_ctx);
            // "light" or "dark", for a picture of the theme the system does not have.
            match std::env::var("HAMMURABI_DEMO_THEME").as_deref() {
                Ok("light") => cc.egui_ctx.set_theme(egui::ThemePreference::Light),
                Ok("dark") => cc.egui_ctx.set_theme(egui::ThemePreference::Dark),
                _ => {}
            }
            // The app's language as a locale, e.g. "tr": for screenshots of a translation.
            if let Ok(lang) = std::env::var("HAMMURABI_DEMO_LANG") {
                hammurabi::i18n::set(Some(hammurabi::i18n::from_locale(&lang)));
            }
            // The page to open, for screenshots: listen, translate, voice, captions or general.
            let page = match std::env::args().nth(1).as_deref() {
                Some("translate") => Page::Translate,
                Some("voice") => Page::ReadAloud,
                Some("captions") => Page::Captions,
                Some("general") => Page::General,
                _ => Page::Listen,
            };
            // A second word "screen" shows the page as it is when subtitles are read off the
            // screen, and a third is the window to read them from.
            let screen = std::env::args().nth(2).as_deref() == Some("screen");
            let mut win = SettingsWindow::default();
            win.page = page;
            win.welcome = std::env::var_os("HAMMURABI_DEMO_WELCOME").is_some();
            Ok(Box::new(Demo {
                win,
                settings: Settings {
                    translate: true,
                    target_language: "tr".into(),
                    speak: true,
                    hint: "Lamar, Los Santos, Vice City, Trevor Philips".into(),
                    asr_backend: if screen {
                        AsrBackend::ScreenText
                    } else {
                        AsrBackend::Whisper
                    },
                    screen_window: std::env::args().nth(3).unwrap_or_default(),
                    // "natural" or "own", for the translator that is not the default.
                    translator: match std::env::var("HAMMURABI_DEMO_TRANSLATOR").as_deref() {
                        Ok("natural") => TranslatorChoice::Natural { light: false },
                        Ok("own") => TranslatorChoice::Llm {
                            base_url: "http://localhost:11434/v1".into(),
                            model: "qwen2.5:3b".into(),
                        },
                        _ => TranslatorChoice::Builtin,
                    },
                    ..Settings::default()
                },
                status: ModelsStatus {
                    cached: vec![ModelId::WhisperTurbo, ModelId::Madlad3bQ4k],
                    downloading: vec![(ModelId::WhisperLarge, 1_200_000_000, Some(3_090_000_000))],
                    devices: DeviceList {
                        inputs: vec![dev("a", "Microphone (USB)", true)],
                        outputs: vec![dev("b", "Speakers (Realtek)", true)],
                        system_audio: Ok(()),
                    },
                    compute: std::env::var("HAMMURABI_DEMO_COMPUTE")
                        .unwrap_or_else(|_| "CUDA 0".into()),
                    gpu: hammurabi::device::gpu(),
                    asr_ms: Some(210.0),
                    tr_ms: Some(1700.0),
                    autostart: false,
                    voices: vec![
                        Voice {
                            id: "tr-1".into(),
                            name: "Microsoft Tolga".into(),
                            lang: "tr-TR".into(),
                            gender: Some(Gender::Male),
                        },
                        Voice {
                            id: "tr-2".into(),
                            name: "Microsoft Emel".into(),
                            lang: "tr-TR".into(),
                            gender: Some(Gender::Female),
                        },
                        Voice {
                            id: "en-1".into(),
                            name: "Microsoft Aria".into(),
                            lang: "en-US".into(),
                            gender: Some(Gender::Female),
                        },
                    ],
                    // Captions running: the only way to see "Download first" and "in use".
                    running: std::env::var_os("HAMMURABI_DEMO_RUNNING").is_some(),
                    loading: false,
                    status: String::new(),
                    alert: false,
                    reading: None,
                    previewing: None,
                    voice_learned: false,
                    update: if std::env::var_os("HAMMURABI_DEMO_IDLE").is_some() {
                        hammurabi::ui::update::Status::Idle
                    } else {
                        hammurabi::ui::update::Status::Downloading("0.7.5".into(), 42)
                    },
                },
            }))
        }),
    )
}
