//! Where ReflectMaps keeps its data, and what the drive under it is like.
//!
//! **Portable mode** (Windows zip for an external drive): when a folder
//! named `ReflectMaps Data` sits next to the program, everything the app
//! writes goes in there: map packs, settings, the web view's storage. The
//! folder can be copied to another drive as is and works under any drive
//! letter. A `WebView2` folder next to the program (Microsoft's Fixed
//! Version runtime) is used instead of the copy installed on the PC.

use serde::Serialize;
use std::path::{Path, PathBuf};

pub const PORTABLE_DATA: &str = "ReflectMaps Data";
#[cfg_attr(not(windows), allow(dead_code))]
pub const PORTABLE_WEBVIEW: &str = "WebView2";

/// The folder holding the program, if it is set up as a portable copy.
pub fn portable_root() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?.to_path_buf();
    dir.join(PORTABLE_DATA).is_dir().then_some(dir)
}

/// Must run before any web view is created.
pub fn setup_portable_env(root: &Path) {
    #[cfg(windows)]
    {
        let rt = root.join(PORTABLE_WEBVIEW);
        if rt.join("msedgewebview2.exe").exists() && std::env::var_os("WEBVIEW2_BROWSER_EXECUTABLE_FOLDER").is_none() {
            grant_sandbox_access(&rt);
            std::env::set_var("WEBVIEW2_BROWSER_EXECUTABLE_FOLDER", &rt);
        }
        std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", root.join(PORTABLE_DATA).join("webview"));
    }
    #[cfg(not(windows))]
    let _ = root;
}

/// WebView2's sandboxed processes need read access to a Fixed Version
/// runtime folder. On an NTFS drive that the folder was copied to, it has
/// only the drive's default permissions, so grant it once per drive (the
/// drive's serial number is remembered; exFAT drives have no permissions).
#[cfg(windows)]
fn grant_sandbox_access(rt: &Path) {
    use std::os::windows::process::CommandExt;
    let Some((fs, serial)) = volume_info(rt) else { return };
    if fs != "NTFS" {
        return;
    }
    let marker = rt.join(".reflectmaps-acl");
    if std::fs::read_to_string(&marker).ok().as_deref() == Some(&serial.to_string()) {
        return;
    }
    let ok = std::process::Command::new("icacls")
        .arg(rt)
        .args(["/grant", "*S-1-15-2-1:(OI)(CI)(RX)", "*S-1-15-2-2:(OI)(CI)(RX)", "/T", "/C", "/Q"])
        .creation_flags(0x0800_0000) // no console window
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if ok {
        let _ = std::fs::write(&marker, serial.to_string());
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct StorageInfo {
    pub data_dir: String,
    pub portable: bool,
    pub free: Option<u64>,
    pub total: Option<u64>,
    /// File system of the drive, where known ("NTFS", "exFAT", "FAT32").
    pub file_system: Option<String>,
    /// Shown to the user when the drive is unsuitable.
    pub warning: Option<String>,
}

pub fn info(data: &Path, portable: bool) -> StorageInfo {
    let _ = std::fs::create_dir_all(data);
    let file_system = file_system(data);
    let warning = match file_system.as_deref() {
        Some("FAT32") | Some("FAT") => Some(
            "This drive is formatted FAT32, which can't hold files over 4 GB. Some large states won't install. \
             Reformat it as exFAT or NTFS (this erases the drive)."
                .to_string(),
        ),
        _ => None,
    };
    StorageInfo {
        data_dir: data.to_string_lossy().into(),
        portable,
        free: fs4::available_space(data).ok(),
        total: fs4::total_space(data).ok(),
        file_system,
        warning,
    }
}

pub fn free_space(data: &Path) -> Option<u64> {
    fs4::available_space(data).ok()
}

#[cfg(windows)]
fn file_system(path: &Path) -> Option<String> {
    volume_info(path).map(|v| v.0)
}

/// File system name and serial number of the drive holding `path`.
#[cfg(windows)]
fn volume_info(path: &Path) -> Option<(String, u32)> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn GetVolumePathNameW(file: *const u16, root: *mut u16, len: u32) -> i32;
        fn GetVolumeInformationW(
            root: *const u16,
            name: *mut u16,
            name_len: u32,
            serial: *mut u32,
            max_component: *mut u32,
            flags: *mut u32,
            fs_name: *mut u16,
            fs_name_len: u32,
        ) -> i32;
    }
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut root = [0u16; 512];
    let mut fs = [0u16; 64];
    let mut serial = 0u32;
    unsafe {
        if GetVolumePathNameW(wide.as_ptr(), root.as_mut_ptr(), root.len() as u32) == 0 {
            return None;
        }
        if GetVolumeInformationW(
            root.as_ptr(),
            std::ptr::null_mut(),
            0,
            &mut serial,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            fs.as_mut_ptr(),
            fs.len() as u32,
        ) == 0
        {
            return None;
        }
    }
    let n = fs.iter().position(|c| *c == 0).unwrap_or(fs.len());
    Some((String::from_utf16_lossy(&fs[..n]), serial))
}

#[cfg(not(windows))]
fn file_system(_path: &Path) -> Option<String> {
    None
}

pub fn format_bytes(n: u64) -> String {
    let n = n as f64;
    if n >= 1e9 {
        format!("{:.1} GB", n / 1e9)
    } else if n >= 1e6 {
        format!("{:.0} MB", n / 1e6)
    } else {
        format!("{:.0} KB", (n / 1e3).max(1.0))
    }
}
