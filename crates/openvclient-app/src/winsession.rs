//! Windows: close cleanly when Windows Restart Manager or a logoff asks the app
//! to end.
//!
//! Installers (including OpenVClient's own setup) use Restart Manager to close
//! a running copy before replacing files. It sends `WM_QUERYENDSESSION`, then
//! `WM_ENDSESSION`, and expects the process to exit on its own.
//!
//! winit answers `WM_QUERYENDSESSION` with 0 ("do not end"), so Restart
//! Manager terminated the process instead of letting it close, losing whatever
//! had not been saved. Every window on the UI thread is subclassed to agree to
//! the shutdown, and the app window is then asked to close as if its close
//! button had been clicked, which saves the workspace on the way out.
//!
//! The installer pairs this with `CloseApplications=force`, because a plain
//! `RmShutdown` is cancelled by Windows whatever the app answers (measured:
//! `ERROR_FAIL_SHUTDOWN` after ~30 ms, with `WM_ENDSESSION` arriving with
//! `wParam = 0`). Forcing is safe here precisely because of this module: the
//! app is asked first and closes itself, and is only killed if it does not.

use std::sync::atomic::{AtomicIsize, Ordering};

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, TRUE, WPARAM};
use windows_sys::Win32::System::Recovery::RegisterApplicationRestart;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{EnumThreadWindows, PostMessageW, WM_CLOSE, WM_ENDSESSION, WM_QUERYENDSESSION};
use windows_sys::core::BOOL;

const SUBCLASS_ID: usize = 0x4F56_4301;

/// The app window, which is the one asked to close. Windows handles fit in an
/// isize, and this is only ever written once from the UI thread.
static MAIN_WINDOW: AtomicIsize = AtomicIsize::new(0);

/// Subclass every top-level window on the UI thread. Returns false when there
/// is no native window (for example in headless tests).
pub fn install(frame: &eframe::Frame) -> bool {
    let Ok(handle) = frame.window_handle() else { return false };
    let RawWindowHandle::Win32(w) = handle.as_raw() else { return false };
    MAIN_WINDOW.store(w.hwnd.get(), Ordering::SeqCst);
    register_for_restart();
    // SAFETY: called on the UI thread; `hook` matches WNDENUMPROC and only
    // subclasses windows owned by this thread.
    unsafe { EnumThreadWindows(GetCurrentThreadId(), Some(hook), 0) != 0 }
}

/// Tell Windows this app may be restarted after an update.
///
/// Without it `RmGetList` reports the process as not restartable, and Restart
/// Manager then refuses to close it unless the caller forces a shutdown: the
/// installer's "automatically close the applications" step fails. The flags
/// keep Windows from relaunching the app after a crash, a hang or a reboot, so
/// the only restart left is the one an installer asks for.
fn register_for_restart() {
    const RESTART_NO_CRASH: u32 = 1;
    const RESTART_NO_HANG: u32 = 2;
    const RESTART_NO_REBOOT: u32 = 8;
    // A null command line means "restart with the current one".
    // SAFETY: null is the documented way to pass no command line.
    let hr = unsafe { RegisterApplicationRestart(std::ptr::null(), RESTART_NO_CRASH | RESTART_NO_HANG | RESTART_NO_REBOOT) };
    if hr < 0 {
        tracing::debug!(target: "openvclient", "RegisterApplicationRestart failed: 0x{hr:x}");
    }
}

unsafe extern "system" fn hook(hwnd: HWND, _param: LPARAM) -> BOOL {
    // SAFETY: hwnd is a live window owned by this thread, and `session_proc`
    // matches SUBCLASSPROC. Subclassing twice with the same id is a no-op.
    unsafe { SetWindowSubclass(hwnd, Some(session_proc), SUBCLASS_ID, 0) };
    TRUE
}

unsafe extern "system" fn session_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _id: usize, _data: usize) -> LRESULT {
    match msg {
        // "Yes, this app can be shut down." Answering 0 for any one window is
        // what made Restart Manager give up, so never forward this to winit.
        WM_QUERYENDSESSION => return 1,
        // The session is ending, or an installer needs our files. wparam is 0
        // when the shutdown was cancelled after all.
        WM_ENDSESSION if wparam != 0 => {
            let main = MAIN_WINDOW.load(Ordering::SeqCst);
            if main != 0 {
                // SAFETY: posting to our own window; handled on the next turn
                // of the event loop, like clicking the close button.
                unsafe { PostMessageW(main as HWND, WM_CLOSE, 0, 0) };
            }
            return 0;
        }
        _ => {}
    }
    // SAFETY: forwards to the next window procedure in the chain (winit's).
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}
