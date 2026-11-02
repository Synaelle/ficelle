// SPDX-License-Identifier: AGPL-3.0-or-later

//! The system's accent color, for the setting that follows it: Windows'
//! own, the desktop portal's on Linux, else KDE's or GNOME's named one. Android tells the page itself
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
    portal().or_else(kde).or_else(gnome)
}

/// A system command run with the host's own settings: the AppImage points
/// `GLib` at its bundled schemas and libraries, from an older GNOME that
/// knows no accent, and `gsettings` then finds no such key.
#[cfg(target_os = "linux")]
fn host_command(program: &str) -> std::process::Command {
    let mut command = std::process::Command::new(program);
    for name in ["GSETTINGS_SCHEMA_DIR", "LD_LIBRARY_PATH", "GIO_MODULE_DIR"] {
        command.env_remove(name);
    }
    if let (Some(dir), Ok(dirs)) = (std::env::var_os("APPDIR"), std::env::var("XDG_DATA_DIRS")) {
        let dir = dir.to_string_lossy().into_owned();
        let host: Vec<&str> = dirs.split(':').filter(|d| !d.starts_with(&dir)).collect();
        command.env("XDG_DATA_DIRS", host.join(":"));
    }
    command
}

/// The desktop portal's `org.freedesktop.appearance accent-color`, which
/// GNOME 47 and later and KDE Plasma 6 give as red, green and blue from 0
/// to 1.
#[cfg(target_os = "linux")]
fn portal() -> Option<String> {
    let out = host_command("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.freedesktop.portal.Desktop",
            "--object-path",
            "/org/freedesktop/portal/desktop",
            "--method",
            "org.freedesktop.portal.Settings.ReadOne",
            "org.freedesktop.appearance",
            "accent-color",
        ])
        .output()
        .ok()?;
    portal_accent(&String::from_utf8_lossy(&out.stdout))
}

/// `(<(0.78, 0.53, 0.0)>,)` from gdbus. Values outside 0 to 1 mean none
/// was chosen.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn portal_accent(text: &str) -> Option<String> {
    let inner = text.split('(').nth(2)?.split(')').next()?;
    let parts: Vec<f64> = inner
        .split(',')
        .map(|p| p.trim().parse().ok())
        .collect::<Option<_>>()?;
    let [r, g, b] = parts[..] else {
        return None;
    };
    let channel = |v: f64| {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "checked to be 0 to 1 first"
        )]
        (0.0..=1.0).contains(&v).then(|| (v * 255.0).round() as u8)
    };
    Some(hex(channel(r)?, channel(g)?, channel(b)?))
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
    let out = host_command("gsettings")
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

    #[test]
    fn the_portal_accent_reads() {
        let yellow = "(<(0.78431372549019607, 0.53333333333333333, 0.0)>,)\n";
        assert_eq!(portal_accent(yellow).as_deref(), Some("#C88800"));
        // none chosen: outside 0 to 1
        assert_eq!(portal_accent("(<(-1.0, -1.0, -1.0)>,)"), None);
        assert_eq!(
            portal_accent("Error: GDBus.Error:org.freedesktop.portal.Error.NotFound"),
            None
        );
    }
}
