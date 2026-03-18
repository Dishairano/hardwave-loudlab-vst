//! Token persistence — reused across all Hardwave VST plugins.
//!
//! Stores the Hardwave Studios auth token at:
//!   <data_dir>/hardwave/auth_token
//!
//! The webview sends the token via IPC after login; we persist it so the
//! plugin can authenticate on next load without re-login.

use std::fs;
use std::path::PathBuf;

/// Return the path to the auth token file.
fn token_path() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("hardwave").join("auth_token"))
}

/// Read the stored auth token, if any.
pub fn load_token() -> Option<String> {
    let path = token_path()?;
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

/// Persist an auth token.
pub fn save_token(token: &str) -> Result<(), String> {
    let path = token_path().ok_or("no data directory")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(&path, token).map_err(|e| e.to_string())
}

/// Delete the stored token (logout).
pub fn clear_token() -> Result<(), String> {
    let path = token_path().ok_or("no data directory")?;
    if path.exists() {
        fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    Ok(())
}
