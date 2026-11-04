// macOS and Linux versions of the few OS calls eframe doesn't cover (the Windows ones are in os_windows.rs).
use eframe::egui::Rect;

/// Mouse position in physical screen pixels, even when the cursor isn't over our window. `native_ppp` is the
/// display's pixels per point: macOS reports points, X11 reports pixels.
#[cfg(target_os = "macos")]
pub fn cursor(native_ppp: f32) -> Option<(f32, f32)> {
    use core_graphics::event::CGEvent;
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
    let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState).ok()?;
    let p = CGEvent::new(source).ok()?.location(); // points, from the top-left of the main display
    Some((p.x as f32 * native_ppp, p.y as f32 * native_ppp))
}

/// X11 through one display connection for the whole run. None without an X server (then she stays clickable).
#[cfg(not(target_os = "macos"))]
pub fn cursor(_native_ppp: f32) -> Option<(f32, f32)> {
    use x11_dl::xlib;
    struct X {
        lib: xlib::Xlib,
        display: *mut xlib::Display,
    }
    thread_local! {
        static X11: Option<X> = xlib::Xlib::open().ok().and_then(|lib| {
            let display = unsafe { (lib.XOpenDisplay)(std::ptr::null()) };
            (!display.is_null()).then_some(X { lib, display })
        });
    }
    X11.with(|x| {
        let x = x.as_ref()?;
        let (mut root, mut child, mut rx, mut ry, mut wx, mut wy, mut mask) = (0, 0, 0, 0, 0, 0, 0);
        let ok = unsafe {
            let screen = (x.lib.XDefaultScreen)(x.display);
            (x.lib.XQueryPointer)(x.display, (x.lib.XRootWindow)(x.display, screen), &mut root, &mut child, &mut rx, &mut ry, &mut wx, &mut wy, &mut mask)
        };
        (ok != 0).then_some((rx as f32, ry as f32))
    })
}

/// Unknown here: the caller asks egui instead.
pub fn mouse_down() -> Option<bool> {
    None
}

/// Whether her see-through parts can let clicks through. Not on a Wayland session: XWayland only reports the cursor
/// over X windows, so once clicks pass through her she'd never notice the cursor coming back.
pub fn click_through() -> bool {
    cfg!(target_os = "macos") || (std::env::var_os("WAYLAND_DISPLAY").is_none() && std::env::var("XDG_SESSION_TYPE").map_or(true, |t| t != "wayland"))
}

/// The window you're typing in. Not tracked here: double-click to bring back the terminal is Windows-only.
pub fn foreground() -> isize {
    0
}

pub fn focus(_window: isize) {}

/// Last input time. Not tracked here, so typing never shows: she cheers only when a prompt is sent.
pub fn last_input() -> u32 {
    0
}

/// Borderless windows have no extra frame to remove here.
pub fn remove_border(_window: isize) {}

/// Where to put her the first time; None lets the window manager decide.
pub fn work_area_corner() -> Option<(f32, f32)> {
    None
}

/// The usable desktop area, to keep her on screen; None skips that check.
pub fn work_area(_window: isize) -> Option<Rect> {
    None
}
