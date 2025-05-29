// src/core_monitors/mod.rs

pub mod foreground_app;
pub mod keyboard_capture;
pub mod clipboard_capture;
mod vk_utils; // Keep vk_utils private to the core_monitors module (helper)