// SPDX-License-Identifier: AGPL-3.0-or-later

//! How the desktop knows the app on Linux. Wayland has no window icons: a
//! compositor shows the icon of the desktop entry named like the window's app
//! id, which GTK takes from the program name. The AppImage installs nothing, so
//! it writes that entry and its icon itself, in the user's own folders. The
//! Arch package installs the entry under the same name, and removes the one
//! an AppImage left, which would come first and start the AppImage.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tauri::Emitter;
use tauri::image::Image;

/// Before GTK starts: the program name, and so the Wayland app id and the X11
/// window class, is the app's identifier.
pub fn set_app_id() {
    gtk::glib::set_prgname(Some(app_brand::IDENTIFIER));
}

/// Before GTK starts: on KDE, the file pickers go through the desktop portal,
/// which shows KDE's own dialog instead of GTK's. GTK falls back to its own
/// when no portal answers.
pub fn use_kde_file_dialogs() {
    let kde = std::env::var("XDG_CURRENT_DESKTOP")
        .is_ok_and(|desktops| desktops.split(':').any(|d| d.eq_ignore_ascii_case("KDE")));
    if kde {
        let _ = gtk::glib::setenv("GTK_USE_PORTAL", "1", false);
    }
}

/// A logout or a shutdown ends the app with SIGTERM: the window saves the
/// keys typed last first, still waiting out the editor's pause, then the
/// app ends. Saving takes milliseconds, the wait is for a busy machine.
pub fn save_on_term(app: &tauri::AppHandle) {
    let app = app.clone();
    gtk::glib::unix_signal_add_local(15, move || {
        let _ = app.emit("app-ending", ());
        let app = app.clone();
        gtk::glib::timeout_add_local_once(Duration::from_millis(1500), move || app.exit(0));
        gtk::glib::ControlFlow::Break
    });
}

/// When running as an AppImage, its desktop entry and icon, written again
/// only when they change (the AppImage moved, a new icon). Before the first
/// window: the desktop looks its icon up once, when it maps.
pub fn integrate(icon: Option<&Image<'_>>) {
    let Some(data) = data_home() else {
        return;
    };
    let Some(appimage) = std::env::var_os("APPIMAGE").map(PathBuf::from) else {
        if crate::commands::updates::pacman() {
            remove_appimage_entry(&data);
        }
        return;
    };
    // every size the AppImage carries, sharp in a taskbar or a dock; the
    // window's own icon (32 px) only when there are none
    let copied =
        std::env::var_os("APPDIR").map_or(0, |dir| copy_bundled_icons(&PathBuf::from(dir), &data));
    if copied == 0
        && let Some(icon) = icon
        && let Some(png) = encode_png(icon.rgba(), icon.width(), icon.height())
    {
        let folder = data.join(format!(
            "icons/hicolor/{}x{}/apps",
            icon.width(),
            icon.height()
        ));
        write_if_changed(&folder.join(format!("{}.png", app_brand::IDENTIFIER)), &png);
    }
    write_if_changed(
        &data
            .join("applications")
            .join(format!("{}.desktop", app_brand::IDENTIFIER)),
        desktop_entry(&appimage).as_bytes(),
    );
}

/// `usr/share/icons/hicolor/<size>/apps/<slug>.png` inside the AppImage, under
/// the app id in the user's icons. The bundler names its largest folder
/// `256x256@2`, which no theme reads: written as `256x256`.
fn copy_bundled_icons(appdir: &Path, data: &Path) -> usize {
    let Ok(sizes) = fs::read_dir(appdir.join("usr/share/icons/hicolor")) else {
        return 0;
    };
    let mut copied = 0;
    for size in sizes.flatten() {
        let name = size.file_name().to_string_lossy().replace("@2", "");
        let Ok(png) = fs::read(size.path().join(format!("apps/{}.png", app_brand::SLUG))) else {
            continue;
        };
        write_if_changed(
            &data.join(format!(
                "icons/hicolor/{name}/apps/{}.png",
                app_brand::IDENTIFIER
            )),
            &png,
        );
        copied += 1;
    }
    copied
}

fn data_home() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
}

fn desktop_entry(appimage: &Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName={name}\nExec={exec} %F\nIcon={id}\nStartupWMClass={id}\nTerminal=false\nCategories=Office;Utility;\nMimeType=text/markdown;text/x-markdown;text/plain;\n",
        name = app_brand::DISPLAY_NAME,
        exec = quote_exec(&appimage.to_string_lossy()),
        id = app_brand::IDENTIFIER,
    )
}

/// A path as one argument of `Exec`, quoted the way the desktop entry
/// specification asks.
fn quote_exec(path: &str) -> String {
    let mut out = String::from("\"");
    for c in path.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    // `%` starts a field code, even inside quotes
    out.replace('%', "%%")
}

fn encode_png(rgba: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().ok()?.write_image_data(rgba).ok()?;
    Some(out)
}

/// The entry and icons an AppImage wrote under `data`: only an entry this
/// file wrote, whose `Exec` is a quoted path, never one the person made.
fn remove_appimage_entry(data: &Path) {
    let entry = data
        .join("applications")
        .join(format!("{}.desktop", app_brand::IDENTIFIER));
    let ours = fs::read_to_string(&entry).is_ok_and(|text| {
        text.lines().any(|line| line.starts_with("Exec=\""))
            && text.contains(&format!("StartupWMClass={}\n", app_brand::IDENTIFIER))
    });
    if !ours {
        return;
    }
    let _ = fs::remove_file(&entry);
    let Ok(sizes) = fs::read_dir(data.join("icons/hicolor")) else {
        return;
    };
    for size in sizes.flatten() {
        let _ = fs::remove_file(
            size.path()
                .join(format!("apps/{}.png", app_brand::IDENTIFIER)),
        );
    }
}

/// Best effort: a desktop entry that cannot be written only costs the icon.
fn write_if_changed(path: &Path, bytes: &[u8]) {
    if fs::read(path).is_ok_and(|old| old == bytes) {
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(path, bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_paths_are_quoted() {
        assert_eq!(
            quote_exec("/home/a/Ficelle.AppImage"),
            "\"/home/a/Ficelle.AppImage\""
        );
        assert_eq!(quote_exec("/x/a b$c\"d"), "\"/x/a b\\$c\\\"d\"");
        assert_eq!(quote_exec("/x/100%"), "\"/x/100%%\"");
    }

    #[test]
    fn the_appimage_entry_goes_and_others_stay() {
        let data = std::env::temp_dir().join(format!("desktop-entry-{}", std::process::id()));
        let entry = data
            .join("applications")
            .join(format!("{}.desktop", app_brand::IDENTIFIER));
        let icon = data.join(format!(
            "icons/hicolor/128x128/apps/{}.png",
            app_brand::IDENTIFIER
        ));
        let other = data.join("icons/hicolor/128x128/apps/other.png");
        write_if_changed(&icon, b"png");
        write_if_changed(&other, b"png");
        // one the person wrote stays, with the icons
        write_if_changed(&entry, b"[Desktop Entry]\nExec=ficelle\n");
        remove_appimage_entry(&data);
        assert!(entry.exists() && icon.exists());
        write_if_changed(
            &entry,
            desktop_entry(Path::new("/opt/F.AppImage")).as_bytes(),
        );
        remove_appimage_entry(&data);
        assert!(!entry.exists() && !icon.exists() && other.exists());
        let _ = fs::remove_dir_all(&data);
    }

    #[test]
    fn entry_names_the_app_id() {
        let entry = desktop_entry(Path::new("/opt/F.AppImage"));
        assert!(entry.contains(&format!("StartupWMClass={}\n", app_brand::IDENTIFIER)));
        assert!(entry.contains(&format!("Icon={}\n", app_brand::IDENTIFIER)));
    }
}
