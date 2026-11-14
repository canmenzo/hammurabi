use eframe::egui;
use hammurabi::audio::{DeviceInfo, DeviceList};
use hammurabi::models::ModelId;
use hammurabi::settings::{AsrBackend, Settings};
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
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Hammurabi Settings")
            .with_inner_size([740.0, 600.0])
            .with_position([40.0, 20.0])
            .with_active(false),
        ..Default::default()
    };
    eframe::run_native(
        "Settings demo",
        opts,
        Box::new(|cc| {
            apply_theme(&cc.egui_ctx);
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
                    compute: "CUDA 0".into(),
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
                    running: false,
                    loading: false,
                    status: String::new(),
                    alert: false,
                    reading: None,
                    previewing: None,
                    voice_learned: false,
                },
            }))
        }),
    )
}
