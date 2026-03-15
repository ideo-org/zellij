//! Cell size query utilities for Kitty graphics protocol.
//!
//! Applications query cell pixel dimensions before sending images
//! to calculate how many cells an image will occupy.
//! The query is typically via `\x1b[16t` (cell size report).

/// Default cell pixel width used when actual dimensions are unknown.
pub const DEFAULT_CELL_WIDTH_PX: u32 = 8;

/// Default cell pixel height used when actual dimensions are unknown.
pub const DEFAULT_CELL_HEIGHT_PX: u32 = 16;

/// Build a cell size response for `\x1b[16t` query.
/// Format: `\x1b[6;<height>;<width>t`
pub fn build_cell_size_response(cell_width_px: u32, cell_height_px: u32) -> Vec<u8> {
    format!("\x1b[6;{};{}t", cell_height_px, cell_width_px).into_bytes()
}

/// Calculate how many terminal cells an image of given pixel dimensions would occupy.
/// Uses ceiling division to ensure partial cells are counted.
pub fn pixels_to_cells(pixel_dim: u32, cell_dim_px: u32) -> u32 {
    if cell_dim_px == 0 {
        return 0;
    }
    (pixel_dim + cell_dim_px - 1) / cell_dim_px // ceiling division
}

/// Build an XTSMGRAPHICS response for cell size query.
/// Format: `\x1b[?2;0;<width>S\x1b[?3;0;<height>S` where values are cell dimensions in pixels.
pub fn build_xtsmgraphics_cell_size_response(cell_width_px: u32, cell_height_px: u32) -> Vec<u8> {
    // Respond with both dimensions
    let response = format!("\x1b[?2;0;{}S\x1b[?3;0;{}S", cell_width_px, cell_height_px);
    response.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_cell_size_response_format() {
        let response = build_cell_size_response(8, 16);
        let response_str = String::from_utf8(response).unwrap();

        // Verify format: \x1b[6;<height>;<width>t
        assert!(response_str.starts_with("\x1b[6;"));
        assert!(response_str.ends_with("t"));
        assert!(response_str.contains("16;8"));
    }

    #[test]
    fn test_pixels_to_cells_exact() {
        // 160px / 8px = 20 cells exactly
        let cells = pixels_to_cells(160, 8);
        assert_eq!(cells, 20);
    }

    #[test]
    fn test_pixels_to_cells_ceiling() {
        // 161px / 8px = 20.125 → 21 cells (ceiling)
        let cells = pixels_to_cells(161, 8);
        assert_eq!(cells, 21);
    }

    #[test]
    fn test_pixels_to_cells_zero_cell_size() {
        // Avoid divide by zero
        let cells = pixels_to_cells(160, 0);
        assert_eq!(cells, 0);
    }

    #[test]
    fn test_pixels_to_cells_small_pixel_dim() {
        // 1px / 8px = 0.125 → 1 cell (ceiling)
        let cells = pixels_to_cells(1, 8);
        assert_eq!(cells, 1);
    }

    #[test]
    fn test_build_xtsmgraphics_response_format() {
        let response = build_xtsmgraphics_cell_size_response(8, 16);
        let response_str = String::from_utf8(response).unwrap();

        // Verify format contains both width and height responses
        assert!(response_str.contains("\x1b[?2;0;8S"));
        assert!(response_str.contains("\x1b[?3;0;16S"));
    }

    #[test]
    fn test_build_xtsmgraphics_response_different_dimensions() {
        let response = build_xtsmgraphics_cell_size_response(10, 20);
        let response_str = String::from_utf8(response).unwrap();

        assert!(response_str.contains("10"));
        assert!(response_str.contains("20"));
    }

    #[test]
    fn test_pixels_to_cells_large_dimensions() {
        // 1920px / 8px = 240 cells
        let cells = pixels_to_cells(1920, 8);
        assert_eq!(cells, 240);
    }

    #[test]
    fn test_pixels_to_cells_height_calculation() {
        // 384px / 16px = 24 cells
        let cells = pixels_to_cells(384, 16);
        assert_eq!(cells, 24);
    }
}
