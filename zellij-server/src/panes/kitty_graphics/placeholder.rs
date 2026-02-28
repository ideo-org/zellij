//! Utilities for Kitty graphics protocol Unicode placeholder encoding.
//!
//! The Kitty graphics protocol uses Unicode placeholders for multiplexer-safe image display.
//! Each cell containing a placeholder encodes:
//! - Image ID (24-bit, encoded in foreground color RGB)
//! - Row index (5-bit, encoded as combining diacritic U+0305..U+0324)
//! - Column index (5-bit, encoded as combining diacritic U+0305..U+0324)
//!
//! Reference: https://sw.kovidgoyal.net/kitty/graphics-protocol/#unicode-placeholders

/// The Unicode placeholder character for Kitty graphics protocol image cells.
/// U+10EEEE — Supplementary Private Use Area-B
pub const KITTY_PLACEHOLDER_CHAR: char = '\u{10EEEE}';

/// Check if a character is the Kitty graphics placeholder
pub fn is_kitty_placeholder(c: char) -> bool {
    c == KITTY_PLACEHOLDER_CHAR
}

/// Encode an image ID (u32) into an RGB foreground color.
/// The image ID is encoded in the lower 24 bits of the u32.
/// Returns (r, g, b) where r is the most significant byte.
pub fn image_id_to_fg_color(image_id: u32) -> (u8, u8, u8) {
    let id = image_id & 0x00FFFFFF; // only lower 24 bits
    let r = ((id >> 16) & 0xFF) as u8;
    let g = ((id >> 8) & 0xFF) as u8;
    let b = (id & 0xFF) as u8;
    (r, g, b)
}

/// Decode an RGB foreground color back into an image ID.
pub fn fg_color_to_image_id(r: u8, g: u8, b: u8) -> u32 {
    ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

/// Encode a row/column index as a combining diacritic character.
/// Uses U+0305 through U+0324 for values 0–31.
/// Values > 31 will wrap (value % 32).
pub fn encode_row_col_diacritic(value: u32) -> char {
    let codepoint = 0x0305 + (value % 32);
    char::from_u32(codepoint).unwrap_or('\u{0305}')
}

/// Decode a combining diacritic character back to a row/column index.
/// Returns None if the character is not a valid diacritic.
pub fn decode_row_col_diacritic(c: char) -> Option<u32> {
    let cp = c as u32;
    if cp >= 0x0305 && cp <= 0x0324 {
        Some(cp - 0x0305)
    } else {
        None
    }
}

/// Build the string to write into a placeholder cell:
/// [KITTY_PLACEHOLDER_CHAR][row_diacritic][col_diacritic]
/// with foreground color set to encode the image_id.
/// Returns (placeholder_string, fg_color_rgb)
pub fn build_placeholder_cell(image_id: u32, row: u32, col: u32) -> (String, (u8, u8, u8)) {
    let mut s = String::new();
    s.push(KITTY_PLACEHOLDER_CHAR);
    s.push(encode_row_col_diacritic(row));
    s.push(encode_row_col_diacritic(col));
    let fg = image_id_to_fg_color(image_id);
    (s, fg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_image_id_roundtrip() {
        let test_ids = [1, 42, 255, 65535, 0x00FFFFFF];
        for id in test_ids.iter() {
            let (r, g, b) = image_id_to_fg_color(*id);
            let decoded = fg_color_to_image_id(r, g, b);
            assert_eq!(decoded, *id, "Image ID roundtrip failed for {}", id);
        }
    }

    #[test]
    fn test_row_col_roundtrip() {
        let test_values = [0, 1, 10, 31];
        for value in test_values.iter() {
            let diacritic = encode_row_col_diacritic(*value);
            let decoded = decode_row_col_diacritic(diacritic);
            assert_eq!(
                decoded,
                Some(*value),
                "Row/col roundtrip failed for {}",
                value
            );
        }
    }

    #[test]
    fn test_is_kitty_placeholder() {
        assert!(is_kitty_placeholder(KITTY_PLACEHOLDER_CHAR));
        assert!(!is_kitty_placeholder('A'));
    }

    #[test]
    fn test_build_placeholder_cell() {
        let (cell_str, fg) = build_placeholder_cell(42, 0, 0);
        assert!(cell_str.starts_with(KITTY_PLACEHOLDER_CHAR));
        let decoded_id = fg_color_to_image_id(fg.0, fg.1, fg.2);
        assert_eq!(decoded_id, 42);
    }

    #[test]
    fn test_row_col_wrap() {
        let diacritic_32 = encode_row_col_diacritic(32);
        let decoded_32 = decode_row_col_diacritic(diacritic_32);
        assert_eq!(decoded_32, Some(0), "Value 32 should wrap to 0");

        let diacritic_33 = encode_row_col_diacritic(33);
        let decoded_33 = decode_row_col_diacritic(diacritic_33);
        assert_eq!(decoded_33, Some(1), "Value 33 should wrap to 1");
    }
}
