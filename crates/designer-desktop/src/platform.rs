//! The desktop window's own settings that DrawnUI for Rust does not cover. Same approach as
//! AgentSignals (`src/platform`): the window is found by this process and its title.


/// Takes maximizing away from the designer window. Called once, when the window exists.
/// - Windows: no maximize button; without it Windows also skips maximizing on a title bar
///   double-click and on a drag to the top of the screen.
/// - macOS: the green button is disabled and full screen is refused.
/// - Linux: window managers decide the buttons; nothing is changed there.
pub fn no_maximize() {
    #[cfg(windows)]
    windows::no_maximize();
    #[cfg(target_os = "macos")]
    macos::no_maximize();
}

/// The window's last size in points, for its first size at start (Windows; elsewhere the default).
pub fn saved_size() -> Option<(f64, f64)> {
    #[cfg(windows)]
    return windows::saved().map(|w| (w.width, w.height));
    #[cfg(not(windows))]
    None
}

/// Puts the window back where it was last (when that place is still on a screen) and remembers
/// where it goes from now on: at the end of each move or resize, and when it closes. Windows only:
/// DrawnUI for Rust has no window position yet.
pub fn remember_window_place() {
    #[cfg(windows)]
    windows::remember_place();
}

/// Closes the window as its close button does (File, Exit): the place is saved, the program ends.
pub fn close_window() {
    #[cfg(windows)]
    windows::close();
    #[cfg(not(windows))]
    std::process::exit(0);
}

#[cfg(windows)]
mod windows {
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GWL_STYLE, GetWindowLongPtrW, GetWindowTextW, GetWindowThreadProcessId, SWP_FRAMECHANGED, SWP_NOACTIVATE,
        SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetWindowLongPtrW, SetWindowPos, WS_MAXIMIZEBOX,
    };

    fn own_window() -> Option<HWND> {
        unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> i32 {
            let found = unsafe { &mut *(lparam as *mut Option<HWND>) };
            let mut pid = 0u32;
            unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
            if pid != unsafe { GetCurrentProcessId() } {
                return 1;
            }
            let mut buf = [0u16; 64];
            let n = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) }.max(0) as usize;
            if String::from_utf16_lossy(&buf[..n]) == crate::ui::TITLE {
                *found = Some(hwnd);
                return 0;
            }
            1
        }
        let mut found: Option<HWND> = None;
        unsafe { EnumWindows(Some(each), &mut found as *mut _ as LPARAM) };
        found
    }

    /// Where the window was last: its normal (not minimized) rect in screen pixels, and its size
    /// in points.
    #[derive(serde::Serialize, serde::Deserialize)]
    pub struct Saved {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
        pub width: f64,
        pub height: f64,
    }

    fn path() -> Option<std::path::PathBuf> {
        std::env::var_os("APPDATA").map(|d| std::path::PathBuf::from(d).join("DrawnUi.Designer").join("window.json"))
    }

    pub fn saved() -> Option<Saved> {
        serde_json::from_str(&std::fs::read_to_string(path()?).ok()?).ok()
    }

    fn save(hwnd: HWND) {
        use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowPlacement, WINDOWPLACEMENT};
        let mut placement: WINDOWPLACEMENT = unsafe { std::mem::zeroed() };
        placement.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
        if unsafe { GetWindowPlacement(hwnd, &mut placement) } == 0 {
            return;
        }
        use windows_sys::Win32::Foundation::RECT;
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetClientRect, IsIconic};
        let r = placement.rcNormalPosition;
        let scale = unsafe { GetDpiForWindow(hwnd) }.max(96) as f64 / 96.0;
        // The drawn area in points, the window's first size at start (minimized it has none: the last one stays).
        let mut client = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        let (width, height) = if unsafe { IsIconic(hwnd) } == 0 && unsafe { GetClientRect(hwnd, &mut client) } != 0 && client.right > 0 {
            (client.right as f64 / scale, client.bottom as f64 / scale)
        } else {
            match saved() {
                Some(s) => (s.width, s.height),
                None => return,
            }
        };
        let saved = Saved { left: r.left, top: r.top, right: r.right, bottom: r.bottom, width, height };
        if let (Some(path), Ok(text)) = (path(), serde_json::to_string(&saved)) {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(path, text);
        }
    }

    unsafe extern "system" fn watch(hwnd: HWND, msg: u32, wparam: usize, lparam: isize, _id: usize, _data: usize) -> isize {
        use windows_sys::Win32::UI::Shell::DefSubclassProc;
        use windows_sys::Win32::UI::WindowsAndMessaging::{WM_CLOSE, WM_DESTROY, WM_EXITSIZEMOVE};
        // The close request too: DrawnUI ends the program on it, before WM_DESTROY.
        if msg == WM_EXITSIZEMOVE || msg == WM_CLOSE || msg == WM_DESTROY {
            save(hwnd);
        }
        unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
    }

    pub fn remember_place() {
        use windows_sys::Win32::Foundation::RECT;
        use windows_sys::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONULL, MonitorFromRect};
        use windows_sys::Win32::UI::Shell::SetWindowSubclass;
        use windows_sys::Win32::UI::WindowsAndMessaging::{SWP_NOACTIVATE, SWP_NOZORDER};
        let Some(hwnd) = own_window() else { return };
        if let Some(s) = saved() {
            let rect = RECT { left: s.left, top: s.top, right: s.right, bottom: s.bottom };
            // A place on a screen that is gone (a monitor unplugged) is skipped: the window stays where Windows put it.
            if s.right > s.left && s.bottom > s.top && !unsafe { MonitorFromRect(&rect, MONITOR_DEFAULTTONULL) }.is_null() {
                unsafe { SetWindowPos(hwnd, std::ptr::null_mut(), s.left, s.top, s.right - s.left, s.bottom - s.top, SWP_NOZORDER | SWP_NOACTIVATE) };
            }
        }
        unsafe { SetWindowSubclass(hwnd, Some(watch), 1, 0) };
    }

    pub fn close() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};
        match own_window() {
            Some(hwnd) => unsafe {
                PostMessageW(hwnd, WM_CLOSE, 0, 0);
            },
            None => std::process::exit(0),
        }
    }

    pub fn no_maximize() {
        let Some(hwnd) = own_window() else { return };
        unsafe {
            let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
            SetWindowLongPtrW(hwnd, GWL_STYLE, style & !(WS_MAXIMIZEBOX as isize));
            SetWindowPos(hwnd, std::ptr::null_mut(), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED | SWP_NOACTIVATE);
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSWindowButton, NSWindowCollectionBehavior};

    /// Runs on the main thread: the canvas callback that calls it is the window's thread.
    pub fn no_maximize() {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let app = NSApplication::sharedApplication(mtm);
        let Some(window) = app.windows().into_iter().find(|w| w.title().to_string() == crate::ui::TITLE) else { return };
        let keep = window.collectionBehavior()
            & !(NSWindowCollectionBehavior::FullScreenPrimary | NSWindowCollectionBehavior::FullScreenAuxiliary);
        window.setCollectionBehavior(keep | NSWindowCollectionBehavior::FullScreenNone);
        if let Some(zoom) = window.standardWindowButton(NSWindowButton::ZoomButton) {
            zoom.setEnabled(false);
        }
    }
}
