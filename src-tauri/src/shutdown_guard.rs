//! Windows shutdown / restart / sign-out with servers running.
//!
//! Without this, Windows ends the app — and with it the game servers — the
//! moment the user shuts down, and a server killed mid-save corrupts its
//! world. Windows tells top-level windows first (WM_QUERYENDSESSION, then
//! WM_ENDSESSION); the webview window does not let us hook that, so a hidden
//! window of our own listens instead:
//!
//! - On WM_QUERYENDSESSION with servers running, it registers a shutdown
//!   block reason ("Saving game worlds…"), so Windows shows why it is waiting
//!   instead of a nameless "this app is preventing shutdown".
//! - On WM_ENDSESSION it stops every server cleanly and waits for them to
//!   finish saving (up to the grace in `stop_all`) before letting the session
//!   end. The user can still force it from the Windows screen.

use std::sync::{Arc, OnceLock};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Shutdown::{ShutdownBlockReasonCreate, ShutdownBlockReasonDestroy};
use windows_sys::Win32::System::Threading::SetProcessShutdownParameters;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassW, TranslateMessage, MSG,
    WM_ENDSESSION, WM_QUERYENDSESSION, WNDCLASSW, WS_OVERLAPPED,
};

use crate::service::InstanceService;

struct Guard {
    service: Arc<InstanceService>,
    runtime: tokio::runtime::Handle,
}

static GUARD: OnceLock<Guard> = OnceLock::new();

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn servers_running(guard: &Guard) -> bool {
    let service = guard.service.clone();
    guard.runtime.block_on(async move { !service.processes.running_pids().await.is_empty() })
}

unsafe extern "system" fn window_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_QUERYENDSESSION => {
            if let Some(guard) = GUARD.get() {
                if servers_running(guard) {
                    let reason = wide("Nebula Host: saving game worlds before Windows shuts down…");
                    ShutdownBlockReasonCreate(hwnd, reason.as_ptr());
                    tracing::info!("Windows is ending the session: servers will be stopped first");
                }
            }
            1 // allow; the work happens in WM_ENDSESSION
        }
        WM_ENDSESSION => {
            if wparam != 0 {
                if let Some(guard) = GUARD.get() {
                    let service = guard.service.clone();
                    guard.runtime.block_on(async move {
                        service.wake.stop_all().await;
                        service.forwarder.release_all().await;
                        service.processes.stop_all("stop").await;
                    });
                    tracing::info!("servers stopped for Windows shutdown");
                }
            }
            ShutdownBlockReasonDestroy(hwnd);
            0
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

/// Start listening for the end of the Windows session. Call once at startup,
/// from inside the async runtime.
pub fn install(service: Arc<InstanceService>) {
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        tracing::warn!("shutdown guard needs the async runtime");
        return;
    };
    if GUARD.set(Guard { service, runtime }).is_err() {
        return;
    }
    std::thread::Builder::new()
        .name("nebula-shutdown-guard".into())
        .spawn(|| unsafe {
            // Ask to be told early (apps range 0x100–0x3FF; higher = sooner).
            SetProcessShutdownParameters(0x3FF, 0);
            let instance = GetModuleHandleW(std::ptr::null());
            let class_name = wide("NebulaHostShutdownGuard");
            let class = WNDCLASSW {
                style: 0,
                lpfnWndProc: Some(window_proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: instance,
                hIcon: std::ptr::null_mut(),
                hCursor: std::ptr::null_mut(),
                hbrBackground: std::ptr::null_mut(),
                lpszMenuName: std::ptr::null(),
                lpszClassName: class_name.as_ptr(),
            };
            RegisterClassW(&class);
            let title = wide("Nebula Host");
            // A real top-level window (never shown): message-only windows do
            // not receive the end-of-session broadcast.
            let hwnd = CreateWindowExW(
                0,
                class_name.as_ptr(),
                title.as_ptr(),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                instance,
                std::ptr::null(),
            );
            if hwnd.is_null() {
                tracing::warn!("shutdown guard window could not be created");
                return;
            }
            let mut message: MSG = std::mem::zeroed();
            while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        })
        .ok();
}
