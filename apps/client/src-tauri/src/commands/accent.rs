// SPDX-License-Identifier: AGPL-3.0-or-later

//! The system's accent color, for the setting that follows it: Windows'
//! own, KDE's, or GNOME's named one. Android tells the page itself
//! (`SystemInfo` in `MainActivity.kt`). `None` where the system has none to give.

/// The accent as `#RRGGBB`, `None` when this system has none.
#[tauri::command]
pub async fn system_accent() -> Option<String> {
    tauri::async_runtime::spawn_blocking(read)
        .await
        .ok()
        .flatten()
}

fn hex(r: u8, g: u8, b: u8) -> String {
    format!("#{r:02X}{g:02X}{b:02X}")
}

#[cfg(windows)]
fn read() -> Option<String> {
    use windows::UI::ViewManagement::{UIColorType, UISettings};
    let color = UISettings::new()
        .ok()?
        .GetColorValue(UIColorType::Accent)
        .ok()?;
    Some(hex(color.R, color.G, color.B))
}

#[cfg(target_os = "linux")]
fn read() -> Option<String> {
    kde().or_else(gnome)
}

#[cfg(not(any(windows, target_os = "linux")))]
fn read() -> Option<String> {
    None
}

/// KDE keeps it in kdeglobals, `[General] AccentColor=r,g,b`, once the
/// person chose one (or took it from the wallpaper).
#[cfg(target_os = "linux")]
fn kde() -> Option<String> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config"))
        })?;
    kde_accent(&std::fs::read_to_string(config.join("kdeglobals")).ok()?)
}

/// `AccentColor=r,g,b` in the `[General]` group of a kdeglobals file.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn kde_accent(text: &str) -> Option<String> {
    let mut general = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            general = line == "[General]";
        } else if general && let Some(value) = line.strip_prefix("AccentColor=") {
            let parts: Vec<u8> = value
                .split(',')
                .map(|p| p.trim().parse().ok())
                .collect::<Option<_>>()?;
            if let [r, g, b, ..] = parts[..] {
                return Some(hex(r, g, b));
            }
        }
    }
    None
}

/// GNOME 47 and later name it: `gsettings get org.gnome.desktop.interface
/// accent-color` says `'blue'`, one of a fixed few, here in GNOME's colors.
#[cfg(target_os = "linux")]
fn gnome() -> Option<String> {
    let out = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "accent-color"])
        .output()
        .ok()?;
    gnome_accent(String::from_utf8_lossy(&out.stdout).trim())
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn gnome_accent(name: &str) -> Option<String> {
    Some(
        match name.trim_matches('\'') {
            "blue" => "#3584E4",
            "teal" => "#2190A4",
            "green" => "#3A944A",
            "yellow" => "#C88800",
            "orange" => "#ED5B00",
            "red" => "#E62D42",
            "pink" => "#D56199",
            "purple" => "#9141AC",
            "slate" => "#6F8396",
            _ => return None,
        }
        .to_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// This machine's own accent, read the way the setting does.
    #[cfg(windows)]
    #[test]
    #[ignore = "reads this machine's settings"]
    fn windows_gives_its_accent() {
        let accent = read().expect("an accent");
        assert!(accent.starts_with('#') && accent.len() == 7, "{accent}");
        println!("accent {accent}");
    }

    #[test]
    fn kde_and_gnome_accents_read() {
        let kde = "[Colors:View]\nAccentColor=1,2,3\n[General]\nColorScheme=Breeze\nAccentColor=61,174,233\n";
        assert_eq!(kde_accent(kde).as_deref(), Some("#3DAEE9"));
        assert_eq!(kde_accent("[General]\nColorScheme=Breeze\n"), None);
        assert_eq!(gnome_accent("'purple'").as_deref(), Some("#9141AC"));
        assert_eq!(gnome_accent("''"), None);
    }
}
