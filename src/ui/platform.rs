/// `NativeOptions::event_loop_builder` hook: force X11 on Linux when DISPLAY is set, macOS Accessory policy.
pub fn configure_event_loop(builder: &mut eframe::EventLoopBuilder<eframe::UserEvent>) {
    #[cfg(target_os = "linux")]
    if std::env::var_os("DISPLAY").is_some() {
        use winit::platform::x11::EventLoopBuilderExtX11;
        builder.with_x11();
    }
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        builder.with_activation_policy(ActivationPolicy::Accessory);
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let _ = builder;
}

/// First frame: keep the overlay on every Space and above fullscreen apps (macOS only).
pub fn configure_overlay_window(frame: &eframe::Frame) {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::{NSStatusWindowLevel, NSView, NSWindowCollectionBehavior};
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};

        let Ok(handle) = frame.window_handle() else {
            return;
        };
        let RawWindowHandle::AppKit(h) = handle.as_raw() else {
            return;
        };
        // SAFETY: winit's AppKit handle points to a live NSView owned by the window, on the main thread.
        let view: &NSView = unsafe { h.ns_view.cast().as_ref() };
        if let Some(window) = view.window() {
            window.setCollectionBehavior(
                NSWindowCollectionBehavior::CanJoinAllSpaces
                    | NSWindowCollectionBehavior::FullScreenAuxiliary
                    | NSWindowCollectionBehavior::Stationary,
            );
            window.setLevel(NSStatusWindowLevel);
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = frame;
}
