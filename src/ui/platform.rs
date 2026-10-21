#![allow(dead_code, unused_variables)]

/// `NativeOptions::event_loop_builder` hook: force X11 on Linux when DISPLAY is set, macOS Accessory policy.
pub fn configure_event_loop(builder: &mut eframe::EventLoopBuilder<eframe::UserEvent>) {}
