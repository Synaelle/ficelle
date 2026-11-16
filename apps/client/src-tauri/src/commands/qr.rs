// SPDX-License-Identifier: AGPL-3.0-or-later
//! Reads a QR code in a camera frame, for a phone signing in to a sync server
//! by scanning the code its page shows. The page's camera view sends a small
//! greyscale copy of each frame, base64 like any text: the Android bridge
//! would send raw bytes as a list of numbers, much larger. Only the Android
//! app reads them: elsewhere the reader is not built in.

use crate::error::{CommandError, CommandResult};

/// The largest frame accepted, more than the camera view ever sends.
#[cfg(any(test, target_os = "android"))]
const MOST_PIXELS: usize = 1280 * 1280;

/// The text of the first QR code found in a `width` by `height` frame of
/// greyscale bytes, `None` when there is none, or none that reads.
#[cfg(any(test, target_os = "android"))]
pub fn read(width: usize, height: usize, luma: &[u8]) -> Option<String> {
    if width == 0 || height == 0 || width * height != luma.len() {
        return None;
    }
    let mut image =
        rqrr::PreparedImage::prepare_from_greyscale(width, height, |x, y| luma[y * width + x]);
    image
        .detect_grids()
        .into_iter()
        .find_map(|grid| grid.decode().ok().map(|(_, text)| text))
}

/// A camera frame from the scan screen: `luma` holds `width` by `height`
/// greyscale bytes, in base64.
#[tauri::command]
pub fn qr_read(width: usize, height: usize, luma: String) -> CommandResult<Option<String>> {
    #[cfg(any(test, target_os = "android"))]
    {
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(luma)
            .map_err(|_| CommandError::new("invalid", "the frame is not base64"))?;
        if width.saturating_mul(height) > MOST_PIXELS {
            return Err(CommandError::new("invalid", "the frame is too large"));
        }
        Ok(read(width, height, &bytes))
    }
    #[cfg(not(any(test, target_os = "android")))]
    {
        let _ = (width, height, luma);
        Err(CommandError::new(
            "unsupported",
            "QR codes are read in the Android app only",
        ))
    }
}

#[cfg(test)]
mod tests {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;

    use super::*;

    /// `text` as a QR code `scale` pixels a module, with a quiet zone, on a
    /// grey background: what a camera sees, roughly.
    fn frame(text: &str, scale: usize) -> (usize, Vec<u8>) {
        let code = qrcode::QrCode::new(text.as_bytes()).unwrap();
        let modules = code.width();
        let colors = code.to_colors();
        let side = (modules + 8) * scale;
        let mut luma = vec![200u8; side * side];
        for y in 0..side {
            for x in 0..side {
                let (mx, my) = (x / scale, y / scale);
                if (4..modules + 4).contains(&mx) && (4..modules + 4).contains(&my) {
                    let dark = colors[(my - 4) * modules + (mx - 4)] == qrcode::Color::Dark;
                    luma[y * side + x] = if dark { 30 } else { 230 };
                }
            }
        }
        (side, luma)
    }

    #[test]
    fn reads_the_servers_link_from_a_frame() {
        let link =
            "https://notes.example.org/link#offer=lo_q2p8Zt4cWnD0kLmV7yB3xR9sA1eH6uJfGiOaNbXcYd";
        let (side, luma) = frame(link, 4);
        assert_eq!(read(side, side, &luma).as_deref(), Some(link));
        let sent = STANDARD.encode(&luma);
        assert_eq!(qr_read(side, side, sent).unwrap().as_deref(), Some(link));
    }

    #[test]
    fn a_frame_without_a_code_or_of_the_wrong_size_reads_nothing() {
        assert_eq!(read(64, 64, &[180u8; 64 * 64]), None);
        let (side, luma) = frame("hello", 3);
        assert_eq!(read(side + 1, side, &luma), None, "size and bytes disagree");
        assert!(qr_read(2000, 2000, String::new()).is_err(), "too large");
        assert!(qr_read(1, 1, "not base64!".into()).is_err());
    }
}
