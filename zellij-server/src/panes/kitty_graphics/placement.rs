//! Display/Place action handler (a=p) for Kitty graphics protocol.
//!
//! Computes the 2D grid of Unicode placeholder cells that represent
//! an image placement. The actual writing into the terminal grid
//! happens in the grid integration layer (Task 12).

use super::command::KittyCommand;
use super::placeholder::build_placeholder_cell;
use super::store::KittyImageStore;

use std::fmt;

/// A single cell in the placeholder grid.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaceholderCell {
    /// The placeholder char + combining diacritics encoding row/col
    pub text: String,
    /// Foreground color encoding image_id (24-bit in RGB)
    pub fg_rgb: (u8, u8, u8),
}

/// Output of a successful place operation: the 2D grid of placeholder cells
/// plus metadata needed by the grid integration.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacementOutput {
    pub image_id: u32,
    pub placement_id: u32,
    pub columns: u32,
    pub rows: u32,
    /// \[row\]\[col\] — rows × cols grid of placeholder cells
    pub cells: Vec<Vec<PlaceholderCell>>,
    pub virtual_placement: bool,
    pub z_index: i32,
}

/// Errors from the place handler.
#[derive(Debug, Clone, PartialEq)]
pub enum PlaceError {
    ImageNotFound(u32),
    InvalidDimensions,
}

impl fmt::Display for PlaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlaceError::ImageNotFound(id) => write!(f, "image not found: {}", id),
            PlaceError::InvalidDimensions => write!(f, "invalid placement dimensions"),
        }
    }
}

/// Handle a Kitty graphics Place action (a=p).
///
/// Builds the 2D grid of Unicode placeholder cells for the given image.
/// Does NOT mutate the store — the caller (grid integration) is responsible
/// for persisting the placement.
pub fn handle_place(
    cmd: &KittyCommand,
    store: &KittyImageStore,
) -> Result<PlacementOutput, PlaceError> {
    let image_id = cmd.image_id.ok_or(PlaceError::InvalidDimensions)?;

    // Verify image exists in store
    let _image = store
        .get(image_id)
        .ok_or(PlaceError::ImageNotFound(image_id))?;

    let columns = cmd.columns.unwrap_or(1);
    let rows = cmd.rows.unwrap_or(1);

    if columns == 0 || rows == 0 {
        return Err(PlaceError::InvalidDimensions);
    }

    // Bounds check: prevent excessive memory allocation or overflow
    // Reasonable limits: 10,000 cells per dimension, 100M total cells
    const MAX_DIMENSION: u32 = 10_000;
    const MAX_TOTAL_CELLS: u64 = 100_000_000;

    if columns > MAX_DIMENSION || rows > MAX_DIMENSION {
        return Err(PlaceError::InvalidDimensions);
    }

    let total_cells = (columns as u64)
        .checked_mul(rows as u64)
        .ok_or(PlaceError::InvalidDimensions)?;

    if total_cells > MAX_TOTAL_CELLS {
        return Err(PlaceError::InvalidDimensions);
    }

    // Build the 2D grid of placeholder cells
    let mut cells = Vec::with_capacity(rows as usize);
    for row in 0..rows {
        let mut row_cells = Vec::with_capacity(columns as usize);
        for col in 0..columns {
            let (text, fg_rgb) = build_placeholder_cell(image_id, row, col);
            row_cells.push(PlaceholderCell { text, fg_rgb });
        }
        cells.push(row_cells);
    }

    let placement_id = cmd.placement_id.unwrap_or(0);

    Ok(PlacementOutput {
        image_id,
        placement_id,
        columns,
        rows,
        cells,
        virtual_placement: cmd.unicode_placeholder,
        z_index: cmd.z_index.unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panes::kitty_graphics::placeholder::{fg_color_to_image_id, KITTY_PLACEHOLDER_CHAR};
    use crate::panes::kitty_graphics::store::{ImageFormat, KittyImageStore};

    fn setup_store_with_image(image_id: u32) -> KittyImageStore {
        let mut store = KittyImageStore::new();
        store.store(Some(image_id), ImageFormat::Png, vec![0u8; 100]);
        store
    }

    fn make_place_cmd(image_id: u32, columns: Option<u32>, rows: Option<u32>) -> KittyCommand {
        let mut cmd = KittyCommand::default();
        cmd.image_id = Some(image_id);
        cmd.columns = columns;
        cmd.rows = rows;
        cmd
    }

    #[test]
    fn kitty_place_existing_image_creates_grid() {
        let store = setup_store_with_image(42);
        let cmd = make_place_cmd(42, Some(2), Some(3));
        let output = handle_place(&cmd, &store).unwrap();
        assert_eq!(output.image_id, 42);
        assert_eq!(output.columns, 2);
        assert_eq!(output.rows, 3);
        assert_eq!(output.cells.len(), 3);
        for row in &output.cells {
            assert_eq!(row.len(), 2);
        }
    }

    #[test]
    fn kitty_place_nonexistent_image_returns_error() {
        let store = KittyImageStore::new();
        let cmd = make_place_cmd(999, Some(1), Some(1));
        let result = handle_place(&cmd, &store);
        assert_eq!(result, Err(PlaceError::ImageNotFound(999)));
    }

    #[test]
    fn kitty_place_cells_contain_placeholder_char() {
        let store = setup_store_with_image(1);
        let cmd = make_place_cmd(1, Some(1), Some(1));
        let output = handle_place(&cmd, &store).unwrap();
        let cell = &output.cells[0][0];
        assert!(cell.text.starts_with(KITTY_PLACEHOLDER_CHAR));
    }

    #[test]
    fn kitty_place_fg_color_encodes_image_id() {
        let image_id = 12345u32;
        let store = setup_store_with_image(image_id);
        let cmd = make_place_cmd(image_id, Some(1), Some(1));
        let output = handle_place(&cmd, &store).unwrap();
        let cell = &output.cells[0][0];
        let decoded = fg_color_to_image_id(cell.fg_rgb.0, cell.fg_rgb.1, cell.fg_rgb.2);
        assert_eq!(decoded, image_id);
    }

    #[test]
    fn kitty_place_2x3_grid_has_6_cells() {
        let store = setup_store_with_image(10);
        let cmd = make_place_cmd(10, Some(2), Some(3));
        let output = handle_place(&cmd, &store).unwrap();
        assert_eq!(output.rows, 3);
        assert_eq!(output.columns, 2);
        let total_cells: usize = output.cells.iter().map(|r| r.len()).sum();
        assert_eq!(total_cells, 6);
    }

    #[test]
    fn kitty_place_virtual_placement_flag() {
        let store = setup_store_with_image(7);
        let mut cmd = make_place_cmd(7, Some(1), Some(1));
        cmd.unicode_placeholder = true;
        let output = handle_place(&cmd, &store).unwrap();
        assert!(output.virtual_placement);

        // Without the flag
        let mut cmd2 = make_place_cmd(7, Some(1), Some(1));
        cmd2.unicode_placeholder = false;
        let output2 = handle_place(&cmd2, &store).unwrap();
        assert!(!output2.virtual_placement);
    }

    #[test]
    fn kitty_place_z_index_propagated() {
        let store = setup_store_with_image(5);
        let mut cmd = make_place_cmd(5, Some(1), Some(1));
        cmd.z_index = Some(-42);
        let output = handle_place(&cmd, &store).unwrap();
        assert_eq!(output.z_index, -42);
    }

    #[test]
    fn kitty_place_z_index_defaults_to_zero() {
        let store = setup_store_with_image(5);
        let cmd = make_place_cmd(5, Some(1), Some(1));
        let output = handle_place(&cmd, &store).unwrap();
        assert_eq!(output.z_index, 0);
    }

    #[test]
    fn kitty_place_defaults_to_1x1_without_dimensions() {
        let store = setup_store_with_image(8);
        let cmd = make_place_cmd(8, None, None);
        let output = handle_place(&cmd, &store).unwrap();
        assert_eq!(output.columns, 1);
        assert_eq!(output.rows, 1);
        assert_eq!(output.cells.len(), 1);
        assert_eq!(output.cells[0].len(), 1);
    }

    #[test]
    fn kitty_place_zero_columns_returns_error() {
        let store = setup_store_with_image(3);
        let cmd = make_place_cmd(3, Some(0), Some(1));
        assert_eq!(
            handle_place(&cmd, &store),
            Err(PlaceError::InvalidDimensions)
        );
    }

    #[test]
    fn kitty_place_no_image_id_returns_error() {
        let store = setup_store_with_image(1);
        let cmd = KittyCommand::default(); // no image_id
        assert_eq!(
            handle_place(&cmd, &store),
            Err(PlaceError::InvalidDimensions)
        );
    }

    #[test]
    fn kitty_place_placement_id_propagated() {
        let store = setup_store_with_image(15);
        let mut cmd = make_place_cmd(15, Some(1), Some(1));
        cmd.placement_id = Some(99);
        let output = handle_place(&cmd, &store).unwrap();
        assert_eq!(output.placement_id, 99);
    }

    #[test]
    fn kitty_place_placement_id_defaults_to_zero() {
        let store = setup_store_with_image(15);
        let cmd = make_place_cmd(15, Some(1), Some(1));
        let output = handle_place(&cmd, &store).unwrap();
        assert_eq!(output.placement_id, 0);
    }
}
