#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use hammurabi::{config, ui};

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let instance = single_instance::SingleInstance::new("dev.menzo.hammurabi");
    if instance.as_ref().is_ok_and(|i| !i.is_single()) {
        log::info!("Hammurabi is already running");
        return Ok(());
    }
    let settings = config::load();
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        viewport: ui::overlay::viewport_builder(&settings).with_visible(false),
        event_loop_builder: Some(Box::new(ui::platform::configure_event_loop)),
        ..Default::default()
    };
    eframe::run_native(
        "Hammurabi",
        options,
        Box::new(|cc| Ok(Box::new(ui::App::new(cc, settings)))),
    )
}
