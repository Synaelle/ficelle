// SPDX-License-Identifier: AGPL-3.0-or-later

//! How the desktop knows the app on Linux. Wayland has no window icons: a
//! compositor shows the icon of the desktop entry named like the window's app
//! id, which GTK takes from the program name. The AppImage installs nothing, so
//! it writes that entry and its icon itself, in the user's own folders.

use std::fs;
use std::path::{Path, PathBuf};

use tauri::image::Image;

/// Before GTK starts: the program name, and so the Wayland app id and the X11
/// window class, is the app's identifier.
pub fn set_app_id() {
    gtk::glib::set_prgname(Some(app_brand::IDENTIFIER));
}

/// When running as an AppImage, its desktop entry and icon, written again
/// only when they change (the AppImage moved, a new icon). Before the first
/// window: the desktop looks its icon up once, when it maps.
pub fn integrate(icon: Option<&Image<'_>>) {
    let Some(appimage) = std::env::var_os("APPIMAGE").map(PathBuf::from) else {
        return;
    };
    let Some(data) = data_home() else {
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
        "[Desktop Entry]\nType=Application\nName={name}\nExec={exec}\nIcon={id}\nStartupWMClass={id}\nTerminal=false\nCategories=Office;Utility;\n",
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
    fn entry_names_the_app_id() {
        let entry = desktop_entry(Path::new("/opt/F.AppImage"));
        assert!(entry.contains(&format!("StartupWMClass={}\n", app_brand::IDENTIFIER)));
        assert!(entry.contains(&format!("Icon={}\n", app_brand::IDENTIFIER)));
    }
}
