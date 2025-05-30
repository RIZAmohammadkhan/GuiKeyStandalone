use crate::core_monitors::foreground_app::get_current_foreground_app_info_sync;
use crate::core_monitors::vk_utils;
use crate::errors::{AppError, win_api_error};
use std::ptr::null_mut;
use std::sync::mpsc as std_mpsc;
use std::thread;

use windows_sys::Win32::Foundation::{FALSE, HMODULE, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, HC_ACTION, HHOOK, KBDLLHOOKSTRUCT, MSG,
    PM_NOREMOVE, PeekMessageW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx,
    WH_KEYBOARD_LL,
};

// Ensure this struct is correctly named RawKeyboardData
#[derive(Debug, Clone)]
pub struct RawKeyboardData {
    // <<<<------ CORRECT NAME HERE
    pub vk_code: u16,
    pub scan_code: u32,
    pub flags: u32,
    pub key_value: String,
    pub is_char: bool,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub foreground_app_name: String,
    pub foreground_window_title: String,
}

static mut EVENT_SENDER_KEYBOARD: Option<std_mpsc::Sender<RawKeyboardData>> = None;
static mut HOOK_HANDLE_KEYBOARD: HHOOK = 0 as HHOOK;

struct KeyboardHookHandleRAII(HHOOK);
impl Drop for KeyboardHookHandleRAII {
    fn drop(&mut self) {
        if self.0 != (0 as HHOOK) {
            unsafe {
                if UnhookWindowsHookEx(self.0) == FALSE {
                    eprintln!(
                        "[ERROR] Failed to unhook keyboard: {}",
                        win_api_error("UnhookWindowsHookEx (keyboard)").to_string()
                    );
                } else {
                    // eprintln!("[INFO] Keyboard hook unhooked successfully.");
                }
                HOOK_HANDLE_KEYBOARD = 0 as HHOOK;
            }
        }
    }
}

// Ensure this function is correctly named start_keyboard_monitoring
pub fn start_keyboard_monitoring(
    // <<<<------ CORRECT NAME HERE
    event_tx: std_mpsc::Sender<RawKeyboardData>, // <<<<------ Parameter type should be RawKeyboardData
) -> Result<thread::JoinHandle<()>, AppError> {
    println!("[INFO] Initializing keyboard monitor...");
    unsafe {
        EVENT_SENDER_KEYBOARD = Some(event_tx);
    }

    let handle = thread::Builder::new()
        .name("keyboard_hook_thread".to_string())
        .spawn(move || {
            let h_instance_handle = unsafe { GetModuleHandleW(null_mut()) };
            if h_instance_handle == 0 {
                eprintln!(
                    "[ERROR] Keyboard hook GetModuleHandleW failed: {}",
                    win_api_error("GetModuleHandleW (keyboard)").to_string()
                );
                return;
            }
            let h_instance = h_instance_handle as HMODULE;

            let hook_handle = unsafe {
                SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook_proc), h_instance, 0)
            };

            if hook_handle == (0 as HHOOK) {
                eprintln!(
                    "[ERROR] SetWindowsHookExW for keyboard failed: {}",
                    win_api_error("SetWindowsHookExW (keyboard)").to_string()
                );
                return;
            }
            unsafe { HOOK_HANDLE_KEYBOARD = hook_handle };
            println!(
                "[INFO] Keyboard hook set successfully. Handle: {:?}",
                hook_handle
            );
            let _hook_guard = KeyboardHookHandleRAII(hook_handle);

            let mut msg: MSG = unsafe { std::mem::zeroed() };
            unsafe {
                PeekMessageW(&mut msg, 0 as HWND, 0, 0, PM_NOREMOVE);
                while GetMessageW(&mut msg, 0 as HWND, 0, 0) > 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            println!("[INFO] Keyboard hook message loop ended.");
        })
        .map_err(|e| AppError::Hook(format!("Failed to spawn keyboard hook thread: {}", e)))?;

    Ok(handle)
}

unsafe extern "system" fn keyboard_hook_proc(
    n_code: i32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    if n_code == HC_ACTION as i32 {
        let kbd_struct_ptr = l_param as *const KBDLLHOOKSTRUCT;
        if kbd_struct_ptr.is_null() {
            return CallNextHookEx(HOOK_HANDLE_KEYBOARD, n_code, w_param, l_param);
        }
        let kbd_struct = *kbd_struct_ptr;

        let (key_value, is_char) = vk_utils::vk_code_to_string(
            kbd_struct.vkCode as u16,
            kbd_struct.scanCode,
            kbd_struct.flags,
        );

        let app_info = get_current_foreground_app_info_sync();

        let raw_event = RawKeyboardData {
            // <<<<------ Ensure this is RawKeyboardData
            vk_code: kbd_struct.vkCode as u16,
            scan_code: kbd_struct.scanCode,
            flags: kbd_struct.flags,
            key_value,
            is_char,
            timestamp: chrono::Utc::now(),
            foreground_app_name: app_info.executable_name,
            foreground_window_title: app_info.title,
        };

        let sender_option_ptr: *const Option<std_mpsc::Sender<RawKeyboardData>> =
            core::ptr::addr_of!(EVENT_SENDER_KEYBOARD);

        if let Some(ref sender_in_option) = *sender_option_ptr {
            let sender_clone = sender_in_option.clone();
            if let Err(e) = sender_clone.send(raw_event) {
                eprintln!(
                    "[ERROR] Failed to send raw keyboard event: {}",
                    e.to_string()
                );
            }
        }
    }
    CallNextHookEx(HOOK_HANDLE_KEYBOARD, n_code, w_param, l_param)
}
