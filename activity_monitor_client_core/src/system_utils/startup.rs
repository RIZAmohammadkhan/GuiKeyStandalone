// src/system_utils/startup.rs

use crate::app_config::Settings; // Assumes Settings is in crate::app_config
use crate::errors::AppError; // Assumes AppError is in crate::errors
use std::env;
use std::sync::Arc;
use winreg::RegKey;
use winreg::enums::*; // For KEY_WRITE, REG_CREATED_NEW_KEY, etc.

pub fn setup_autostart(settings: &Arc<Settings>) -> Result<(), AppError> {
    // HKEY_CURRENT_USER for current user login
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let path = r"Software\Microsoft\Windows\CurrentVersion\Run"; // Standard Run key

    // create_subkey opens if exists, creates if not.
    let (key, disposition) = hkcu.create_subkey(&path).map_err(|e| AppError::Io(e))?; // Convert winreg::Error to AppError::Io

    match disposition {
        REG_CREATED_NEW_KEY => tracing::info!(
            "Startup: Registry Run key created at HKEY_CURRENT_USER\\{}",
            path
        ),
        REG_OPENED_EXISTING_KEY => tracing::debug!(
            "Startup: Registry Run key opened at HKEY_CURRENT_USER\\{}",
            path
        ),
    }

    let current_exe_path = env::current_exe().map_err(|e| AppError::Io(e))?;
    let exe_path_str = current_exe_path.to_string_lossy(); // Handles potential non-UTF8 paths gracefully for display

    // Check if our app's autorun entry already exists and is correct
    match key.get_value::<String, _>(&settings.app_name_for_autorun) {
        Ok(existing_path_val) if existing_path_val == exe_path_str.as_ref() => {
            tracing::info!(
                "Startup: Autostart entry for '{}' is already correctly configured to '{}'.",
                settings.app_name_for_autorun,
                exe_path_str
            );
        }
        Ok(existing_path_val) => {
            // Exists but points elsewhere, update it
            tracing::warn!(
                "Startup: Autostart entry for '{}' currently points to '{}'. Updating to '{}'.",
                settings.app_name_for_autorun,
                existing_path_val,
                exe_path_str
            );
            key.set_value(&settings.app_name_for_autorun, &exe_path_str.as_ref())
                .map_err(|e| AppError::Io(e))?;
            tracing::info!(
                "Startup: Autostart entry for '{}' updated.",
                settings.app_name_for_autorun
            );
        }
        Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Entry doesn't exist, create it
            key.set_value(&settings.app_name_for_autorun, &exe_path_str.as_ref())
                .map_err(|e| AppError::Io(e))?;
            tracing::info!(
                "Startup: Autostart entry for '{}' set to '{}'",
                settings.app_name_for_autorun,
                exe_path_str
            );
        }
        Err(e) => {
            // Some other error reading the registry value
            return Err(AppError::Io(e));
        }
    }
    Ok(())
}

// Optional: Function to remove autostart entry (e.g., for uninstaller)
#[allow(dead_code)]
pub fn remove_autostart(settings: &Arc<Settings>) -> Result<(), AppError> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let path = r"Software\Microsoft\Windows\CurrentVersion\Run";

    // Need write access to delete a value
    let key = hkcu
        .open_subkey_with_flags(&path, KEY_WRITE)
        .map_err(|e| AppError::Io(e))?;

    match key.delete_value(&settings.app_name_for_autorun) {
        Ok(_) => {
            tracing::info!(
                "Startup: Autostart entry '{}' removed.",
                settings.app_name_for_autorun
            );
            Ok(())
        }
        Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => {
            tracing::info!(
                "Startup: Autostart entry '{}' not found, nothing to remove.",
                settings.app_name_for_autorun
            );
            Ok(()) // Not an error if it's already gone
        }
        Err(e) => Err(AppError::Io(e)), // Other error deleting value
    }
}
