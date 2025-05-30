// src/core_monitors/clipboard_capture.rs
use crate::app_config::Settings;
use crate::core_monitors::foreground_app::get_current_foreground_app_info_sync;
use crate::errors::{AppError, win_api_error};
use std::ptr::{null, null_mut};
use std::sync::{Arc, mpsc as std_mpsc};
use std::thread;

use windows_sys::Win32::Foundation::{
    ERROR_CLASS_ALREADY_EXISTS,
    ERROR_CLASS_DOES_NOT_EXIST, // Using these from windows_sys
    FALSE,
    GetLastError,
    HGLOBAL,
    HMODULE,
    HWND,
    LPARAM,
    LRESULT,
    WPARAM,
};
use windows_sys::Win32::System::DataExchange::{
    AddClipboardFormatListener, CloseClipboard, GetClipboardData, OpenClipboard,
    RemoveClipboardFormatListener,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows_sys::Win32::System::Ole::CF_UNICODETEXT;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
    HMENU, HWND_MESSAGE, MSG, PM_NOREMOVE, PeekMessageW, PostQuitMessage, RegisterClassW,
    TranslateMessage, UnregisterClassW, WM_CLIPBOARDUPDATE, WM_DESTROY, WNDCLASSW,
};

const CLIPBOARD_LISTENER_CLASS_NAME_WSTR: &[u16] = &[
    0x0043, 0x006C, 0x0069, 0x0070, 0x0062, 0x006F, 0x0061, 0x0072, 0x0064, 0x004C, 0x0069, 0x0073,
    0x0074, 0x0065, 0x006E, 0x0065, 0x0072, 0x0052, 0x0075, 0x0073, 0x0074, 0x0000,
];

#[derive(Debug, Clone)]
pub struct RawClipboardData {
    pub text_content: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub foreground_app_name: String,
    pub foreground_window_title: String,
}

static mut EVENT_SENDER_CLIPBOARD: Option<std_mpsc::Sender<RawClipboardData>> = None;
static mut CLIPBOARD_HWND_STATIC: HWND = 0 as HWND;

struct ClipboardWindowResources {
    hwnd: HWND,
    h_instance: HMODULE,
    class_name_ptr: *const u16,
}

const LOCAL_ERROR_CANNOT_UNREGISTER_ONLINE_CLASS: u32 = 1431;

impl Drop for ClipboardWindowResources {
    fn drop(&mut self) {
        unsafe {
            if self.hwnd != (0 as HWND) {
                RemoveClipboardFormatListener(self.hwnd);
                DestroyWindow(self.hwnd);
                CLIPBOARD_HWND_STATIC = 0 as HWND;
            }
            if UnregisterClassW(self.class_name_ptr, self.h_instance) == FALSE {
                let err = GetLastError();
                if err != ERROR_CLASS_DOES_NOT_EXIST
                    && err != LOCAL_ERROR_CANNOT_UNREGISTER_ONLINE_CLASS
                {
                    eprintln!(
                        "[ERROR] Failed to unregister clipboard listener window class (Error: {}): {}",
                        err,
                        win_api_error("UnregisterClassW (clipboard)").to_string()
                    );
                }
            } else {
                // eprintln!("[INFO] Clipboard listener window class unregistered.");
            }
        }
    }
}

pub fn start_clipboard_monitoring(
    event_tx: std_mpsc::Sender<RawClipboardData>,
    _settings: Arc<Settings>,
) -> Result<thread::JoinHandle<()>, AppError> {
    println!("[INFO] Initializing clipboard monitor...");
    unsafe {
        EVENT_SENDER_CLIPBOARD = Some(event_tx);
    }

    let handle = thread::Builder::new()
        .name("clipboard_monitor_thread".to_string())
        .spawn(move || unsafe {
            let h_instance_handle = GetModuleHandleW(null_mut());
            if h_instance_handle == 0 {
                eprintln!(
                    "[ERROR] Clipboard GetModuleHandleW failed: {}",
                    win_api_error("GetModuleHandleW (clipboard)").to_string()
                );
                return;
            }
            let h_module_instance = h_instance_handle as HMODULE;

            let mut wc: WNDCLASSW = std::mem::zeroed();
            wc.lpfnWndProc = Some(clipboard_window_proc);
            wc.hInstance = h_module_instance;
            wc.lpszClassName = CLIPBOARD_LISTENER_CLASS_NAME_WSTR.as_ptr();

            if RegisterClassW(&wc) == 0 {
                let err = GetLastError();
                if err != ERROR_CLASS_ALREADY_EXISTS {
                    eprintln!(
                        "[ERROR] RegisterClassW for clipboard failed (Error: {}): {}",
                        err,
                        win_api_error("RegisterClassW (clipboard)").to_string()
                    );
                    return;
                }
            }

            let hwnd = CreateWindowExW(
                0,
                CLIPBOARD_LISTENER_CLASS_NAME_WSTR.as_ptr(),
                null(),
                0,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                HWND_MESSAGE,
                0 as HMENU,
                h_module_instance,
                null_mut(),
            );

            if hwnd == (0 as HWND) {
                eprintln!(
                    "[ERROR] CreateWindowExW for clipboard failed: {}",
                    win_api_error("CreateWindowExW (clipboard)").to_string()
                );
                return;
            }
            CLIPBOARD_HWND_STATIC = hwnd;
            println!("[INFO] Clipboard listener window created. HWND: {:?}", hwnd);

            let _window_resources_guard = ClipboardWindowResources {
                hwnd,
                h_instance: h_module_instance,
                class_name_ptr: CLIPBOARD_LISTENER_CLASS_NAME_WSTR.as_ptr(),
            };

            if AddClipboardFormatListener(hwnd) == FALSE {
                eprintln!(
                    "[ERROR] AddClipboardFormatListener failed: {}",
                    win_api_error("AddClipboardFormatListener").to_string()
                );
                return;
            }
            println!("[INFO] AddClipboardFormatListener succeeded.");

            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, hwnd, 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            println!("[INFO] Clipboard monitor message loop ended.");
        })
        .map_err(|e| AppError::Hook(format!("Failed to spawn clipboard monitor thread: {}", e)))?;
    Ok(handle)
}

unsafe extern "system" fn clipboard_window_proc(
    hwnd: HWND,
    msg: u32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    match msg {
        WM_CLIPBOARDUPDATE => {
            if OpenClipboard(hwnd) != FALSE {
                // CORRECTED: Cast CF_UNICODETEXT to u32
                let h_data_handle = GetClipboardData(CF_UNICODETEXT as u32);
                if h_data_handle != 0 {
                    let h_global_data = h_data_handle as HGLOBAL;
                    let p_data_raw = GlobalLock(h_global_data);
                    if !p_data_raw.is_null() {
                        let p_data = p_data_raw as *const u16;
                        let data_size_bytes = GlobalSize(h_global_data);
                        let mut len = 0;
                        if data_size_bytes > 0 {
                            let max_chars = (data_size_bytes / std::mem::size_of::<u16>()) as usize;
                            len = max_chars;
                            for i in 0..max_chars {
                                if *p_data.add(i) == 0 {
                                    len = i;
                                    break;
                                }
                            }
                        }

                        if len > 0 {
                            let slice = std::slice::from_raw_parts(p_data, len);
                            let text_content = String::from_utf16_lossy(slice);

                            let sender_option_ptr: *const Option<
                                std_mpsc::Sender<RawClipboardData>,
                            > = core::ptr::addr_of!(EVENT_SENDER_CLIPBOARD);

                            if let Some(ref sender_in_option) = *sender_option_ptr {
                                let sender_clone = sender_in_option.clone();
                                let app_info = get_current_foreground_app_info_sync();
                                let raw_event = RawClipboardData {
                                    text_content,
                                    timestamp: chrono::Utc::now(),
                                    foreground_app_name: app_info.executable_name,
                                    foreground_window_title: app_info.title,
                                };
                                if let Err(e) = sender_clone.send(raw_event) {
                                    eprintln!(
                                        "[ERROR] Failed to send raw clipboard event: {}",
                                        e.to_string()
                                    );
                                }
                            }
                        }
                        GlobalUnlock(h_global_data);
                    }
                }
                CloseClipboard();
            }
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, w_param, l_param),
    }
}
