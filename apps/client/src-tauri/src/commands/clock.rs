// SPDX-License-Identifier: AGPL-3.0-or-later

//! Whether the system shows a 24-hour clock, for times in the app to match:
//! Windows' regional format, GNOME's clock setting. Android tells the page
//! itself (`SystemInfo` in `MainActivity.kt`). `None` where the system does
//! not say, and the page then goes by the system's locale.

/// `Some(true)` for a 24-hour clock, `Some(false)` for 12 hours with AM and
/// PM, `None` when this system does not say.
#[tauri::command]
pub async fn system_clock_24h() -> Option<bool> {
    tauri::async_runtime::spawn_blocking(read)
        .await
        .ok()
        .flatten()
}

#[cfg(windows)]
fn read() -> Option<bool> {
    use windows::Globalization::DateTimeFormatting::DateTimeFormatter;
    let clock = DateTimeFormatter::ShortTime().ok()?.Clock().ok()?;
    clock_name(&clock.to_string_lossy())
}

#[cfg(target_os = "linux")]
fn read() -> Option<bool> {
    let out = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "clock-format"])
        .output()
        .ok()?;
    gnome_clock(String::from_utf8_lossy(&out.stdout).trim())
}

#[cfg(not(any(windows, target_os = "linux")))]
fn read() -> Option<bool> {
    None
}

/// Windows' clock names, `24HourClock` and `12HourClock`.
#[cfg_attr(not(windows), allow(dead_code))]
fn clock_name(name: &str) -> Option<bool> {
    match name {
        "24HourClock" => Some(true),
        "12HourClock" => Some(false),
        _ => None,
    }
}

/// GNOME's `clock-format`, `'24h'` or `'12h'`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn gnome_clock(value: &str) -> Option<bool> {
    match value.trim_matches('\'') {
        "24h" => Some(true),
        "12h" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_names_read_both_ways() {
        assert_eq!(clock_name("24HourClock"), Some(true));
        assert_eq!(clock_name("12HourClock"), Some(false));
        assert_eq!(clock_name(""), None);
        assert_eq!(gnome_clock("'24h'"), Some(true));
        assert_eq!(gnome_clock("'12h'"), Some(false));
        assert_eq!(gnome_clock("nothing"), None);
    }
}
