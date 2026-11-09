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

/// First frame: on macOS keep the overlay on every Space and above fullscreen apps; on Windows remove its outline.
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
    // winit keeps a 1px frame row above a borderless window, and Windows 11 outlines the window,
    // so a transparent overlay still shows a faint box. A window region cuts off that row, and
    // Windows draws no outline around a window that has one. It is far larger than any monitor,
    // so resizing the overlay needs no update.
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::COLORREF;
        use windows::Win32::Foundation::HWND;
        use windows::Win32::Graphics::Gdi::{CreateRectRgn, SetWindowRgn};
        use windows::Win32::UI::WindowsAndMessaging::{LWA_ALPHA, SetLayeredWindowAttributes};
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

        const FAR: i32 = 1 << 20;
        let Ok(handle) = frame.window_handle() else {
            return;
        };
        let RawWindowHandle::Win32(h) = handle.as_raw() else {
            return;
        };
        // SAFETY: the handle is a live window on this thread, and Windows owns the region from here on.
        let hwnd = HWND(h.hwnd.get() as *mut _);
        unsafe {
            let region = CreateRectRgn(-FAR, 1, FAR, FAR);
            SetWindowRgn(hwnd, Some(region), true);
            // winit makes a click-through window layered without ever setting these, and Windows
            // shows nothing of a layered window until something does.
            if let Err(e) = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA) {
                log::warn!("overlay window attributes: {e}");
            }
        }
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    let _ = frame;
}

/// Brings the app to the front so a newly opened window is not hidden behind the active app.
/// A menu bar (accessory) app is not activated by clicks on its status item menu.
pub fn activate_app() {
    #[cfg(target_os = "macos")]
    {
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};
        // SAFETY: called on the main thread; NSApplication exists once the event loop runs.
        unsafe {
            let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
            let _: () = msg_send![app, activateIgnoringOtherApps: true];
        }
    }
}
