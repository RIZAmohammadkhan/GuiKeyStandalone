use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyboardState, ToUnicode, VK_ACCEPT, VK_ADD, VK_APPS, VK_BACK, VK_BROWSER_BACK,
    VK_BROWSER_FAVORITES, VK_BROWSER_FORWARD, VK_BROWSER_HOME, VK_BROWSER_REFRESH,
    VK_BROWSER_SEARCH, VK_BROWSER_STOP, VK_CAPITAL, VK_CLEAR, VK_CONTROL, VK_CONVERT, VK_DECIMAL,
    VK_DELETE, VK_DIVIDE, VK_DOWN, VK_END, VK_ESCAPE, VK_EXECUTE, VK_F1, VK_F2, VK_F3, VK_F4,
    VK_F5, VK_F6, VK_F7, VK_F8, VK_F9, VK_F10, VK_F11, VK_F12, VK_F13, VK_F14, VK_F15, VK_F16,
    VK_F17, VK_F18, VK_F19, VK_F20, VK_F21, VK_F22, VK_F23, VK_F24, VK_FINAL, VK_HANGEUL,
    VK_HANGUL, VK_HANJA, VK_HELP, VK_HOME, VK_INSERT, VK_JUNJA, VK_KANA, VK_KANJI, VK_LAUNCH_APP1,
    VK_LAUNCH_APP2, VK_LAUNCH_MAIL, VK_LAUNCH_MEDIA_SELECT, VK_LBUTTON, VK_LCONTROL, VK_LEFT,
    VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MBUTTON, VK_MEDIA_NEXT_TRACK, VK_MEDIA_PLAY_PAUSE,
    VK_MEDIA_PREV_TRACK, VK_MEDIA_STOP, VK_MENU, VK_MODECHANGE, VK_MULTIPLY, VK_NEXT,
    VK_NONCONVERT, VK_NUMLOCK, VK_NUMPAD0, VK_NUMPAD1, VK_NUMPAD2, VK_NUMPAD3, VK_NUMPAD4,
    VK_NUMPAD5, VK_NUMPAD6, VK_NUMPAD7, VK_NUMPAD8, VK_NUMPAD9, VK_PAUSE, VK_PRINT, VK_PRIOR,
    VK_RBUTTON, VK_RCONTROL, VK_RETURN, VK_RIGHT, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SCROLL,
    VK_SELECT, VK_SEPARATOR, VK_SHIFT, VK_SLEEP, VK_SNAPSHOT, VK_SPACE, VK_SUBTRACT, VK_TAB, VK_UP,
    VK_VOLUME_DOWN, VK_VOLUME_MUTE, VK_VOLUME_UP, VK_XBUTTON1, VK_XBUTTON2,
};
use windows_sys::Win32::UI::WindowsAndMessaging::LLKHF_UP;
// use windows_sys::Win32::UI::WindowsAndMessaging::LLKHF_UP;

pub fn vk_code_to_string(vk_code_u16: u16, scan_code: u32, flags: u32) -> (String, bool) {
    let mut is_char = false;
    let mut buffer: [u16; 8] = [0; 8];
    let mut keyboard_state: [u8; 256] = [0; 256];

    // Check if LLKHF_UP is set in flags. LLKHF_UP should be u32.
    let _is_key_up = flags & LLKHF_UP == LLKHF_UP;
    let to_unicode_flags = 0u32; // For ToUnicode, 0 is often sufficient.

    let representation = unsafe {
        if GetKeyboardState(keyboard_state.as_mut_ptr()) == 0 {
            // This tracing call might not be ideal if logger isn't fully up when this is first called
            // Consider a more robust fallback or ensuring logger is always available.
            // For now, simple println if tracing fails, or just proceed to simple_vk_map.
            // tracing::warn!("GetKeyboardState failed in vk_code_to_string");
            return simple_vk_map(vk_code_u16);
        }

        let result = ToUnicode(
            vk_code_u16 as u32,
            scan_code,
            keyboard_state.as_ptr(),
            buffer.as_mut_ptr(),
            buffer.len() as i32,
            to_unicode_flags,
        );

        if result > 0 {
            is_char = true;
            let char_count = result as usize;
            let end = buffer
                .iter()
                .take(char_count)
                .position(|&c| c == 0)
                .unwrap_or(char_count);
            String::from_utf16_lossy(&buffer[..end])
        } else if result == 0 {
            // No translation
            is_char = false;
            simple_vk_map(vk_code_u16).0
        } else {
            // result < 0, dead key. abs(result) is number of chars.
            is_char = true; // Treat as a character for logging purposes
            let char_count = result.abs() as usize;
            let end = buffer
                .iter()
                .take(char_count)
                .position(|&c| c == 0)
                .unwrap_or(char_count);
            String::from_utf16_lossy(&buffer[..end]) // This will be the dead key char like ` or ~
        }
    };

    (representation, is_char)
}

fn simple_vk_map(vk_code: u16) -> (String, bool) {
    let mut is_char = false;
    let vk_code_i32 = vk_code as i32;

    // Assuming VK_* constants are u16 or directly castable as per your working version
    let representation = match vk_code_i32 {
        c if c == VK_LBUTTON as i32 => "[MOUSE_LBUTTON]".to_string(),
        c if c == VK_RBUTTON as i32 => "[MOUSE_RBUTTON]".to_string(),
        c if c == VK_MBUTTON as i32 => "[MOUSE_MBUTTON]".to_string(),
        c if c == VK_XBUTTON1 as i32 => "[MOUSE_XBUTTON1]".to_string(),
        c if c == VK_XBUTTON2 as i32 => "[MOUSE_XBUTTON2]".to_string(),
        c if c == VK_BACK as i32 => "[BACKSPACE]".to_string(),
        c if c == VK_TAB as i32 => "[TAB]".to_string(),
        c if c == VK_CLEAR as i32 => "[CLEAR]".to_string(),
        c if c == VK_RETURN as i32 => "[ENTER]".to_string(),
        c if c == VK_SHIFT as i32 => "[SHIFT_ANY]".to_string(),
        c if c == VK_LSHIFT as i32 => "[LSHIFT]".to_string(),
        c if c == VK_RSHIFT as i32 => "[RSHIFT]".to_string(),
        c if c == VK_CONTROL as i32 => "[CTRL_ANY]".to_string(),
        c if c == VK_LCONTROL as i32 => "[LCTRL]".to_string(),
        c if c == VK_RCONTROL as i32 => "[RCTRL]".to_string(),
        c if c == VK_MENU as i32 => "[ALT_ANY]".to_string(),
        c if c == VK_LMENU as i32 => "[LALT]".to_string(),
        c if c == VK_RMENU as i32 => "[RALT]".to_string(),
        c if c == VK_PAUSE as i32 => "[PAUSE]".to_string(),
        c if c == VK_CAPITAL as i32 => "[CAPSLOCK]".to_string(),
        c if c == VK_KANA as i32 || c == VK_HANGEUL as i32 || c == VK_HANGUL as i32 => {
            "[KANA/HANGUL]".to_string()
        }
        c if c == VK_JUNJA as i32 => "[JUNJA]".to_string(),
        c if c == VK_FINAL as i32 => "[FINAL]".to_string(),
        c if c == VK_HANJA as i32 || c == VK_KANJI as i32 => "[HANJA/KANJI]".to_string(),
        c if c == VK_ESCAPE as i32 => "[ESC]".to_string(),
        c if c == VK_CONVERT as i32 => "[CONVERT]".to_string(),
        c if c == VK_NONCONVERT as i32 => "[NONCONVERT]".to_string(),
        c if c == VK_ACCEPT as i32 => "[ACCEPT]".to_string(),
        c if c == VK_MODECHANGE as i32 => "[MODECHANGE]".to_string(),
        c if c == VK_SPACE as i32 => {
            is_char = true;
            " ".to_string()
        }
        c if c == VK_PRIOR as i32 => "[PAGE_UP]".to_string(),
        c if c == VK_NEXT as i32 => "[PAGE_DOWN]".to_string(),
        c if c == VK_END as i32 => "[END]".to_string(),
        c if c == VK_HOME as i32 => "[HOME]".to_string(),
        c if c == VK_LEFT as i32 => "[LEFT_ARROW]".to_string(),
        c if c == VK_UP as i32 => "[UP_ARROW]".to_string(),
        c if c == VK_RIGHT as i32 => "[RIGHT_ARROW]".to_string(),
        c if c == VK_DOWN as i32 => "[DOWN_ARROW]".to_string(),
        c if c == VK_SELECT as i32 => "[SELECT]".to_string(),
        c if c == VK_PRINT as i32 => "[PRINT]".to_string(),
        c if c == VK_EXECUTE as i32 => "[EXECUTE]".to_string(),
        c if c == VK_SNAPSHOT as i32 => "[PRINTSCREEN]".to_string(),
        c if c == VK_INSERT as i32 => "[INSERT]".to_string(),
        c if c == VK_DELETE as i32 => "[DELETE]".to_string(),
        c if c == VK_HELP as i32 => "[HELP]".to_string(),
        c if c == VK_LWIN as i32 => "[LWINKEY]".to_string(),
        c if c == VK_RWIN as i32 => "[RWINKEY]".to_string(),
        c if c == VK_APPS as i32 => "[APP_MENU]".to_string(),
        c if c == VK_SLEEP as i32 => "[SLEEP]".to_string(),
        c if c >= (VK_NUMPAD0 as i32) && c <= (VK_NUMPAD9 as i32) => {
            format!("[NUMPAD_{}]", c - (VK_NUMPAD0 as i32))
        }
        c if c == VK_MULTIPLY as i32 => "[NUMPAD_*]".to_string(),
        c if c == VK_ADD as i32 => "[NUMPAD_+]".to_string(),
        c if c == VK_SEPARATOR as i32 => "[NUMPAD_SEPARATOR]".to_string(),
        c if c == VK_SUBTRACT as i32 => "[NUMPAD_-]".to_string(),
        c if c == VK_DECIMAL as i32 => "[NUMPAD_.]".to_string(),
        c if c == VK_DIVIDE as i32 => "[NUMPAD_/]".to_string(),
        c if c >= (VK_F1 as i32) && c <= (VK_F24 as i32) => {
            format!("[F{}]", c - (VK_F1 as i32) + 1)
        }
        c if c == VK_NUMLOCK as i32 => "[NUMLOCK]".to_string(),
        c if c == VK_SCROLL as i32 => "[SCROLLLOCK]".to_string(),
        c if c == VK_BROWSER_BACK as i32 => "[BROWSER_BACK]".to_string(),
        c if c == VK_BROWSER_FORWARD as i32 => "[BROWSER_FORWARD]".to_string(),
        c if c == VK_BROWSER_REFRESH as i32 => "[BROWSER_REFRESH]".to_string(),
        c if c == VK_BROWSER_STOP as i32 => "[BROWSER_STOP]".to_string(),
        c if c == VK_BROWSER_SEARCH as i32 => "[BROWSER_SEARCH]".to_string(),
        c if c == VK_BROWSER_FAVORITES as i32 => "[BROWSER_FAVORITES]".to_string(),
        c if c == VK_BROWSER_HOME as i32 => "[BROWSER_HOME]".to_string(),
        c if c == VK_VOLUME_MUTE as i32 => "[VOLUME_MUTE]".to_string(),
        c if c == VK_VOLUME_DOWN as i32 => "[VOLUME_DOWN]".to_string(),
        c if c == VK_VOLUME_UP as i32 => "[VOLUME_UP]".to_string(),
        c if c == VK_MEDIA_NEXT_TRACK as i32 => "[MEDIA_NEXT]".to_string(),
        c if c == VK_MEDIA_PREV_TRACK as i32 => "[MEDIA_PREV]".to_string(),
        c if c == VK_MEDIA_STOP as i32 => "[MEDIA_STOP]".to_string(),
        c if c == VK_MEDIA_PLAY_PAUSE as i32 => "[MEDIA_PLAY_PAUSE]".to_string(),
        c if c == VK_LAUNCH_MAIL as i32 => "[LAUNCH_MAIL]".to_string(),
        c if c == VK_LAUNCH_MEDIA_SELECT as i32 => "[LAUNCH_MEDIA_SELECT]".to_string(),
        c if c == VK_LAUNCH_APP1 as i32 => "[LAUNCH_APP1]".to_string(),
        c if c == VK_LAUNCH_APP2 as i32 => "[LAUNCH_APP2]".to_string(),
        _ => format!("[VK_0x{:X}]", vk_code),
    };
    (representation, is_char)
}
