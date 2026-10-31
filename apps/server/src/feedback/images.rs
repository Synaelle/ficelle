// SPDX-License-Identifier: AGPL-3.0-or-later

//! Images attached to reports: PNG, JPEG or WebP, known by their first
//! bytes, their size read from the header without decoding anything. The
//! server never decodes them, it only stores them and hands them to the
//! inbox as downloads.

use app_protocol::feedback::MAX_IMAGE_SIDE;

/// What an attached image is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageInfo {
    /// `png`, `jpeg` or `webp`.
    pub kind: &'static str,
    /// Pixels across.
    pub width: u32,
    /// Pixels down.
    pub height: u32,
}

/// The image's kind and size, or why it is refused.
pub fn check(bytes: &[u8]) -> Result<ImageInfo, &'static str> {
    let (kind, size) = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        ("png", png(bytes))
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        ("jpeg", jpeg(bytes))
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        ("webp", webp(bytes))
    } else {
        return Err("not a PNG, JPEG or WebP image");
    };
    let (width, height) = size.ok_or("an image whose size cannot be read")?;
    if width == 0 || height == 0 {
        return Err("an image without pixels");
    }
    if width > MAX_IMAGE_SIDE || height > MAX_IMAGE_SIDE {
        return Err("an image over 4096 pixels a side");
    }
    Ok(ImageInfo {
        kind,
        width,
        height,
    })
}

fn be16(bytes: &[u8], at: usize) -> Option<u32> {
    let b = bytes.get(at..at + 2)?;
    Some(u32::from(u16::from_be_bytes([b[0], b[1]])))
}

fn le16(bytes: &[u8], at: usize) -> Option<u32> {
    let b = bytes.get(at..at + 2)?;
    Some(u32::from(u16::from_le_bytes([b[0], b[1]])))
}

fn le24(bytes: &[u8], at: usize) -> Option<u32> {
    let b = bytes.get(at..at + 3)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], 0]))
}

/// The header chunk comes first, its width and height big endian.
fn png(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.get(12..16)? != b"IHDR" {
        return None;
    }
    let read = |at: usize| -> Option<u32> {
        let b = bytes.get(at..at + 4)?;
        Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    };
    Some((read(16)?, read(20)?))
}

/// Segments up to the first start of frame, which holds the size.
fn jpeg(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut at = 2;
    loop {
        if *bytes.get(at)? != 0xff {
            return None;
        }
        // any number of 0xff may pad before a marker
        while *bytes.get(at + 1)? == 0xff {
            at += 1;
        }
        let marker = *bytes.get(at + 1)?;
        match marker {
            // markers without a length
            0x01 | 0xd0..=0xd7 => at += 2,
            // the image or the scan starts before any frame: broken
            0xd9 | 0xda => return None,
            0xc0..=0xcf if !matches!(marker, 0xc4 | 0xc8 | 0xcc) => {
                return Some((be16(bytes, at + 7)?, be16(bytes, at + 5)?));
            }
            _ => {
                let length = usize::try_from(be16(bytes, at + 2)?).ok()?;
                if length < 2 {
                    return None;
                }
                at += 2 + length;
            }
        }
    }
}

/// The first chunk is a lossy frame, a lossless one, or the extended
/// header with the canvas size.
fn webp(bytes: &[u8]) -> Option<(u32, u32)> {
    match bytes.get(12..16)? {
        b"VP8 " => {
            if bytes.get(23..26)? != [0x9d, 0x01, 0x2a] {
                return None;
            }
            Some((le16(bytes, 26)? & 0x3fff, le16(bytes, 28)? & 0x3fff))
        }
        b"VP8L" => {
            if *bytes.get(20)? != 0x2f {
                return None;
            }
            let b = bytes.get(21..25)?;
            let bits = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
            Some(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1))
        }
        b"VP8X" => Some((le24(bytes, 24)? + 1, le24(bytes, 27)? + 1)),
        _ => None,
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A PNG header of the given size, enough for [`check`].
    pub fn png_of(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        bytes.extend(width.to_be_bytes());
        bytes.extend(height.to_be_bytes());
        bytes.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
        bytes
    }

    fn jpeg_of(width: u16, height: u16) -> Vec<u8> {
        let mut bytes = vec![0xff, 0xd8];
        // an APP0 segment first, then padding, then the frame
        bytes.extend([0xff, 0xe0, 0, 4, 0, 0]);
        bytes.extend([0xff, 0xff, 0xc2, 0, 11, 8]);
        bytes.extend(height.to_be_bytes());
        bytes.extend(width.to_be_bytes());
        bytes.extend([1, 1, 0x11, 0]);
        bytes
    }

    fn webp_of(chunk: [u8; 4], body: &[u8]) -> Vec<u8> {
        let mut bytes = b"RIFF\0\0\0\0WEBP".to_vec();
        bytes.extend(chunk);
        bytes.extend([0, 0, 0, 0]);
        bytes.extend(body);
        bytes
    }

    #[test]
    fn sizes_are_read_from_each_kind() {
        let png = check(&png_of(640, 480)).unwrap();
        assert_eq!((png.kind, png.width, png.height), ("png", 640, 480));
        let jpeg = check(&jpeg_of(800, 600)).unwrap();
        assert_eq!((jpeg.kind, jpeg.width, jpeg.height), ("jpeg", 800, 600));

        let mut lossy = vec![0, 0, 0, 0x9d, 0x01, 0x2a];
        lossy.extend(300_u16.to_le_bytes());
        lossy.extend(200_u16.to_le_bytes());
        let webp = check(&webp_of(*b"VP8 ", &lossy)).unwrap();
        assert_eq!((webp.width, webp.height), (300, 200));

        let bits: u32 = (100 - 1) | ((50 - 1) << 14);
        let mut lossless = vec![0x2f];
        lossless.extend(bits.to_le_bytes());
        let webp = check(&webp_of(*b"VP8L", &lossless)).unwrap();
        assert_eq!((webp.width, webp.height), (100, 50));

        let extended = [0, 0, 0, 0, 0xff, 0x0f, 0, 0x7f, 0x02, 0];
        let webp = check(&webp_of(*b"VP8X", &extended)).unwrap();
        assert_eq!((webp.width, webp.height), (4096, 640));
    }

    #[test]
    fn other_files_and_wrong_sizes_are_refused() {
        assert!(check(b"GIF89a....").is_err());
        assert!(check(b"<svg xmlns=...>").is_err());
        assert!(check(&[]).is_err());
        assert!(check(&png_of(5000, 10)).is_err(), "too wide");
        assert!(check(&png_of(10, 0)).is_err(), "no pixels");
        assert!(check(&png_of(10, 10)[..20]).is_err(), "cut short");
        assert!(
            check(&[0xff, 0xd8, 0xff, 0xda, 0, 2]).is_err(),
            "scan first"
        );
        assert!(
            check(&[0xff, 0xd8, 0xff, 0xe0, 0, 0]).is_err(),
            "a length under 2"
        );
        // every prefix of a good file is refused or read, never a panic
        let whole = jpeg_of(10, 10);
        for end in 0..whole.len() {
            let _ = check(&whole[..end]);
        }
    }
}
