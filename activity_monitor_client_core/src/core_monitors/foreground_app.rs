// src/core_monitors/foreground_app.rs

use std::ptr::null_mut;
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, MAX_PATH}, // GetLastError is in Foundation
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        },
        Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
            QueryFullProcessImageNameW,
        },
    },
    UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId},
}; // For GetModuleHandleW in other files

#[derive(Debug, Clone, Default)]
pub struct ForegroundAppInfo {
    pub title: String,
    pub executable_name: String,
    pub process_id: u32,
    pub thread_id: u32, // Main thread ID of the foreground window
}

pub fn get_current_foreground_app_info_sync() -> ForegroundAppInfo {
    let mut info = ForegroundAppInfo::default();
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd == 0 {
            // 0 is an invalid HWND
            // tracing::trace!("GetForegroundWindow returned null, no active window.");
            return info;
        }

        let mut window_title_buffer: [u16; 512] = [0; 512];
        let title_length = GetWindowTextW(hwnd, window_title_buffer.as_mut_ptr(), 512);
        if title_length > 0 {
            info.title = String::from_utf16_lossy(&window_title_buffer[..title_length as usize]);
        } else {
            // tracing::trace!("GetWindowTextW failed or returned empty title for HWND {:?}", hwnd);
        }

        info.thread_id = GetWindowThreadProcessId(hwnd, &mut info.process_id);

        if info.process_id != 0 {
            // PROCESS_QUERY_LIMITED_INFORMATION is generally safer and requires fewer privileges
            // than PROCESS_QUERY_INFORMATION. PROCESS_VM_READ might be needed for some fallbacks but try without first.
            let h_process = OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION, // Try with fewer permissions first
                0,                                 // FALSE for bInheritHandle
                info.process_id,
            );

            if h_process != 0 && h_process != -1isize {
                // Valid process handle
                let mut exe_path_buffer: [u16; MAX_PATH as usize] = [0; MAX_PATH as usize];
                let mut exe_path_len = MAX_PATH; // Needs to be u32
                if QueryFullProcessImageNameW(
                    h_process,
                    0, // Can be 0 for default format or PROCESS_NAME_NATIVE
                    exe_path_buffer.as_mut_ptr(),
                    &mut exe_path_len, // Pass as mutable reference
                ) != 0
                {
                    // Non-zero means success
                    let exe_path_actual_len = exe_path_buffer
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(exe_path_len as usize);
                    let exe_path =
                        String::from_utf16_lossy(&exe_path_buffer[..exe_path_actual_len]);
                    if let Some(name) = exe_path.rsplit('\\').next() {
                        info.executable_name = name.to_string();
                    } else {
                        info.executable_name = exe_path; // Should not happen if path is valid
                    }
                } else {
                    // Fallback if QueryFullProcessImageNameW fails (e.g., access denied for some processes)
                    // tracing::debug!("QueryFullProcessImageNameW failed for PID {}: Error code {}. Using fallback.", info.process_id, windows_sys::Win32::Foundation::GetLastError());
                    info.executable_name = get_process_name_fallback(info.process_id)
                        .unwrap_or_else(|| "unknown.exe".to_string());
                }
                CloseHandle(h_process);
            } else {
                // Fallback if OpenProcess fails
                // tracing::debug!("OpenProcess failed for PID {}: Error code {}. Using fallback.", info.process_id, windows_sys::Win32::Foundation::GetLastError());
                info.executable_name = get_process_name_fallback(info.process_id)
                    .unwrap_or_else(|| "unknown.exe".to_string());
            }
        }
    }
    info
}

// Fallback using ToolHelp snapshot
fn get_process_name_fallback(pid: u32) -> Option<String> {
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == -1isize as HANDLE {
            // INVALID_HANDLE_VALUE
            return None;
        }

        let mut pe32: PROCESSENTRY32W = std::mem::zeroed();
        pe32.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

        if Process32FirstW(snapshot, &mut pe32) == 0 {
            // FALSE
            CloseHandle(snapshot);
            return None;
        }

        loop {
            if pe32.th32ProcessID == pid {
                let exe_file_null_term_idx = pe32
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(pe32.szExeFile.len());
                let name = String::from_utf16_lossy(&pe32.szExeFile[..exe_file_null_term_idx]);
                CloseHandle(snapshot);
                return Some(name);
            }
            if Process32NextW(snapshot, &mut pe32) == 0 {
                // FALSE
                break;
            }
        }
        CloseHandle(snapshot);
    }
    None
}
