//! Types into the focused window with `SendInput` Unicode events. Like on macOS, the clipboard
//! stays untouched while text arrives piece by piece.

use crate::hotkeys::Keys;
use std::cell::Cell;
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{CloseHandle, GlobalFree};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows_sys::Win32::System::SystemInformation::GetTickCount;
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetLastInputInfo, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, LASTINPUTINFO, SendInput, VK_BACK, VK_RETURN,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

/// Executables where Return sends a prompt: terminals that run coding agents, and Claude.
// shortcut: fixed list, make it a setting if users ask for other apps.
const SEND_APPS: &[&str] = &[
    "WindowsTerminal.exe",
    "wezterm-gui.exe",
    "alacritty.exe",
    "claude.exe",
];
/// System input this close to our own `SendInput` is taken to be ours.
const OWN_INPUT_SLACK: Duration = Duration::from_millis(100);
const CF_UNICODETEXT: u32 = 13;

pub struct SystemOutput {
    keys: Arc<Keys>,
    /// The foreground window handle, kept as a number so the struct stays `Send`.
    target: Option<isize>,
    sent_at: Cell<Option<Instant>>,
    /// Latest input that was not ours, latched before each injection hides it.
    user_input_at: Cell<Option<Instant>>,
}

fn foreground() -> Option<isize> {
    // SAFETY: plain query without arguments.
    let window = unsafe { GetForegroundWindow() } as isize;
    (window != 0).then_some(window)
}

fn exe_name(window: isize) -> Option<String> {
    let mut pid = 0u32;
    // SAFETY: `pid` is a valid out-pointer; a stale handle only makes the calls fail.
    unsafe {
        GetWindowThreadProcessId(window as _, &mut pid);
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let mut path = [0u16; 1024];
        let mut len = path.len() as u32;
        let ok =
            QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, path.as_mut_ptr(), &mut len);
        CloseHandle(process);
        if ok == 0 {
            return None;
        }
        let path = String::from_utf16_lossy(&path[..len as usize]);
        path.rsplit('\\').next().map(str::to_owned)
    }
}

/// Time of the latest keyboard or mouse input in the session, ours included.
fn last_input() -> Option<Instant> {
    let mut info = LASTINPUTINFO {
        cbSize: size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    // SAFETY: `info` is initialised with its size, as the call requires.
    if unsafe { GetLastInputInfo(&mut info) } == 0 {
        return None;
    }
    // SAFETY: plain query without arguments. The tick counter wraps, so subtract wrapping.
    let age = unsafe { GetTickCount() }.wrapping_sub(info.dwTime);
    Instant::now().checked_sub(Duration::from_millis(age.into()))
}

fn key(vk: u16, scan: u16, flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Each UTF-16 unit as a key press and release; Windows joins surrogate pairs itself.
fn unicode_inputs(text: &str) -> Vec<INPUT> {
    text.encode_utf16()
        .flat_map(|unit| {
            [
                key(0, unit, KEYEVENTF_UNICODE),
                key(0, unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP),
            ]
        })
        .collect()
}

fn tap(vk: u16, count: usize) -> Vec<INPUT> {
    (0..count)
        .flat_map(|_| [key(vk, 0, 0), key(vk, 0, KEYEVENTF_KEYUP)])
        .collect()
}

impl SystemOutput {
    pub fn new(keys: Arc<Keys>) -> Self {
        Self {
            keys,
            target: None,
            sent_at: Cell::new(None),
            user_input_at: Cell::new(None),
        }
    }

    fn last_user_input(&self) -> Option<Instant> {
        let last = last_input();
        let ours = match (last, self.sent_at.get()) {
            (Some(last), Some(sent)) => last <= sent + OWN_INPUT_SLACK,
            _ => false,
        };
        if ours { self.user_input_at.get() } else { last }
    }

    fn send(&self, inputs: &[INPUT]) {
        self.user_input_at.set(self.last_user_input());
        // SAFETY: the pointer and length describe `inputs`, which outlives the call.
        unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_ptr(),
                size_of::<INPUT>() as i32,
            )
        };
        self.sent_at.set(Some(Instant::now()));
    }

    pub fn capture_target(&mut self) {
        self.target = foreground();
    }

    pub fn is_target_focused(&self) -> bool {
        self.target.is_none() || foreground() == self.target
    }

    /// Windows has no secure input mode that swallows synthetic keys.
    pub fn secure_input(&self) -> bool {
        false
    }

    pub fn type_text(&mut self, text: &str) {
        self.send(&unicode_inputs(text));
    }

    pub fn backspace(&mut self, count: usize) {
        self.send(&tap(VK_BACK, count));
    }

    /// Presses Return if the target is one of [`SEND_APPS`] and still has focus.
    pub fn submit(&mut self) {
        let Some(target) = self.target else {
            return;
        };
        let allowed = exe_name(target)
            .is_some_and(|name| SEND_APPS.iter().any(|app| app.eq_ignore_ascii_case(&name)));
        if !allowed || !self.is_target_focused() {
            return;
        }
        // A pause after the typed text, so terminal apps do not read Return as part of a paste.
        std::thread::sleep(Duration::from_millis(150));
        self.send(&tap(VK_RETURN, 1));
    }

    /// Any mouse movement also counts, so removal errs on the side of keeping text.
    pub fn user_interacted_since(&self, since: Instant) -> bool {
        self.keys.user_typed_since(since) || self.last_user_input().is_some_and(|last| last > since)
    }
}

pub fn set_clipboard(text: &str) {
    let units: Vec<u16> = text.encode_utf16().chain([0]).collect();
    let bytes = units.len() * size_of::<u16>();
    // SAFETY: the buffer is sized for `units`; ownership passes to the clipboard on success.
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            log::warn!("clipboard is busy");
            return;
        }
        EmptyClipboard();
        let memory = GlobalAlloc(GMEM_MOVEABLE, bytes);
        let buffer = GlobalLock(memory).cast::<u16>();
        if !buffer.is_null() {
            std::ptr::copy_nonoverlapping(units.as_ptr(), buffer, units.len());
            GlobalUnlock(memory);
            if SetClipboardData(CF_UNICODETEXT, memory).is_null() {
                GlobalFree(memory);
            }
        }
        CloseClipboard();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_unit_is_pressed_and_released() {
        let inputs = unicode_inputs("oʻ👍");
        // "o", "ʻ" and the two surrogate halves of the emoji.
        assert_eq!(inputs.len(), 8);
        // SAFETY: every input is a keyboard input.
        let units: Vec<u16> = inputs
            .iter()
            .step_by(2)
            .map(|i| unsafe { i.Anonymous.ki.wScan })
            .collect();
        assert_eq!(units, "oʻ👍".encode_utf16().collect::<Vec<_>>());
    }
}
