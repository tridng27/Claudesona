// Small Windows calls eframe doesn't cover (the macOS and Linux versions are in os_unix.rs).
use std::sync::atomic::{AtomicIsize, Ordering::Relaxed};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use eframe::egui::{Rect, pos2};
use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow};
use windows_sys::Win32::Graphics::Dwm::{DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE, DwmSetWindowAttribute};
use windows_sys::Win32::UI::HiDpi::GetDpiForSystem;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, GetLastInputInfo, LASTINPUTINFO, VK_LBUTTON, VK_RBUTTON};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, GWLP_WNDPROC, GetCursorPos, GetForegroundWindow, IsIconic, SW_RESTORE, SetForegroundWindow, ShowWindow, SPI_GETWORKAREA, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SWP_NOZORDER, SetWindowLongPtrW, SetWindowPos, SystemParametersInfoW, WM_NCCALCSIZE, WNDPROC,
};

/// Mouse position in physical screen pixels, even when the cursor isn't over our window.
pub fn cursor(_native_ppp: f32) -> Option<(f32, f32)> {
    let mut p = POINT { x: 0, y: 0 };
    (unsafe { GetCursorPos(&mut p) } != 0).then_some((p.x as f32, p.y as f32))
}

/// Whether a mouse button is held. The OS drag loop swallows the release, so egui never sees it.
pub fn mouse_down() -> Option<bool> {
    // Both buttons, because "swap mouse buttons" makes the right one primary.
    Some(unsafe { GetAsyncKeyState(VK_LBUTTON as i32) < 0 || GetAsyncKeyState(VK_RBUTTON as i32) < 0 })
}

/// Her see-through parts always let clicks through on Windows.
pub fn click_through() -> bool {
    true
}

/// The window you're typing in right now.
pub fn foreground() -> isize {
    unsafe { GetForegroundWindow() as isize }
}

/// Tick count of the last keyboard or mouse input anywhere. Says nothing about which key.
pub fn last_input() -> u32 {
    let mut info = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    unsafe { GetLastInputInfo(&mut info) };
    info.dwTime
}

/// Bring a window to the front, restoring it if minimized. Windows allows this because the
/// user just clicked our window, which gives our process the last input event.
pub fn focus(hwnd: isize) {
    unsafe {
        if IsIconic(hwnd as HWND) != 0 {
            ShowWindow(hwnd as HWND, SW_RESTORE);
        }
        SetForegroundWindow(hwnd as HWND);
    }
}

/// Windows 11 draws a thin border and rounds the corners of borderless windows; turn both off.
/// egui-winit also forces winit's "undecorated shadow" on, which keeps a 1px non-client strip
/// at the top so DWM draws a drop shadow. Our window proc makes the client area the whole window.
pub fn remove_border(hwnd: isize) {
    let none: u32 = 0xFFFF_FFFE; // DWMWA_COLOR_NONE
    let square: u32 = 1; // DWMWCP_DONOTROUND
    unsafe {
        DwmSetWindowAttribute(hwnd as HWND, DWMWA_BORDER_COLOR as _, &none as *const u32 as _, 4);
        DwmSetWindowAttribute(hwnd as HWND, DWMWA_WINDOW_CORNER_PREFERENCE as _, &square as *const u32 as _, 4);
        OLD_PROC.store(SetWindowLongPtrW(hwnd as HWND, GWLP_WNDPROC, no_frame_proc as *const () as isize), Relaxed);
        // Re-run WM_NCCALCSIZE now that our proc is in place.
        SetWindowPos(hwnd as HWND, std::ptr::null_mut(), 0, 0, 0, 0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
    }
}

static OLD_PROC: AtomicIsize = AtomicIsize::new(0);

unsafe extern "system" fn no_frame_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_NCCALCSIZE && wparam != 0 {
        return 0; // leave rgrc[0] as the window rect: no non-client area at all
    }
    unsafe { CallWindowProcW(std::mem::transmute::<isize, WNDPROC>(OLD_PROC.load(Relaxed)), hwnd, msg, wparam, lparam) }
}

/// Bottom-right corner of the desktop above the taskbar, in points.
pub fn work_area_corner() -> Option<(f32, f32)> {
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    let ok = unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut r as *mut RECT as _, 0) } != 0;
    let scale = unsafe { GetDpiForSystem() } as f32 / 96.0;
    ok.then_some((r.right as f32 / scale, r.bottom as f32 / scale))
}

/// Desktop area (minus taskbar) of the monitor the window is on, in points.
pub fn work_area(hwnd: isize) -> Option<Rect> {
    let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    let ok = unsafe { GetMonitorInfoW(MonitorFromWindow(hwnd as HWND, MONITOR_DEFAULTTONEAREST), &mut info) } != 0;
    let scale = unsafe { GetDpiForSystem() } as f32 / 96.0;
    let r = info.rcWork;
    ok.then(|| Rect::from_min_max(pos2(r.left as f32, r.top as f32) / scale, pos2(r.right as f32, r.bottom as f32) / scale))
}
