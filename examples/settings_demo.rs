use eframe::egui;
use hammurabi::audio::{DeviceInfo, DeviceList};
use hammurabi::models::ModelId;
use hammurabi::settings::Settings;
use hammurabi::ui::settings_window::{ModelsStatus, SettingsAction, SettingsWindow};

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
            .with_inner_size([520.0, 960.0])
            .with_position([40.0, 20.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Settings demo",
        opts,
        Box::new(|_| {
            Ok(Box::new(Demo {
                win: SettingsWindow::default(),
                settings: Settings {
                    translate: true,
                    ..Settings::default()
                },
                status: ModelsStatus {
                    cached: vec![ModelId::WhisperTurbo, ModelId::Madlad3bQ4k],
                    downloading: Vec::new(),
                    devices: DeviceList {
                        inputs: vec![dev("a", "Microphone (USB)", true)],
                        outputs: vec![dev("b", "Speakers (Realtek)", true)],
                        system_audio: Ok(()),
                    },
                    compute: "CUDA 0".into(),
                    autostart: false,
                },
            }))
        }),
    )
}
