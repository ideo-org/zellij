/// Kitty graphics pane resize handling.
///
/// When a pane is resized, Unicode placeholder characters (U+10EEEE)
/// automatically reflow with the text content since they're just characters
/// in the grid. However, the image placement dimensions (c= columns, r= rows)
/// may no longer match the intended display size.
///
/// This module provides utilities for handling resize events.
use super::store::{KittyImageStore, KittyPlacement};

/// Result of a resize operation for a single image.
pub struct ResizeResult {
    pub image_id: u32,
    /// APC sequence to re-transmit image to host terminal after resize.
    pub retransmit_apc: Vec<u8>,
}

/// Build a delete-and-replace APC sequence for an image after resize.
/// The host terminal needs to be told to delete the old placement and
/// accept the new one with updated dimensions.
pub fn build_resize_delete_apc(image_id: u32, placement_id: u32) -> Vec<u8> {
    // Delete specific placement: \x1b_Ga=d,d=P,i=<id>,p=<placement_id>\x1b\\
    format!("\x1b_Ga=d,d=P,i={},p={}\x1b\\", image_id, placement_id).into_bytes()
}

/// Check if any image placements need updating after a resize.
/// Returns list of image IDs that have placements (may need re-rendering).
pub fn images_needing_resize_update(store: &KittyImageStore) -> Vec<u32> {
    store
        .all_image_ids()
        .into_iter()
        .filter(|&id| {
            store
                .get(id)
                .map(|img| !img.placements.is_empty())
                .unwrap_or(false)
        })
        .collect()
}

/// Determine if a placement's dimensions are still valid after resize.
/// A placement is invalid if its column/row count exceeds the new pane size.
pub fn placement_fits_pane(placement: &KittyPlacement, pane_cols: u32, pane_rows: u32) -> bool {
    let cols_ok = placement.columns.map(|c| c <= pane_cols).unwrap_or(true);
    let rows_ok = placement.rows.map(|r| r <= pane_rows).unwrap_or(true);
    cols_ok && rows_ok
}

#[cfg(test)]
mod tests {
    use super::super::store::ImageFormat;
    use super::*;

    fn make_placement(id: u32, columns: Option<u32>, rows: Option<u32>) -> KittyPlacement {
        KittyPlacement {
            placement_id: id,
            columns,
            rows,
            source_x: None,
            source_y: None,
            source_width: None,
            source_height: None,
            cell_offset_x: 0,
            cell_offset_y: 0,
            z_index: 0,
            virtual_placement: false,
        }
    }

    fn make_store_with_images(ids: &[u32]) -> KittyImageStore {
        let mut store = KittyImageStore::new();
        for &id in ids {
            store.store(Some(id), ImageFormat::Png, vec![0; 10]);
        }
        store
    }

    #[test]
    fn build_resize_delete_apc_format() {
        let apc = build_resize_delete_apc(42, 7);
        // Must start with ESC _ G (APC graphics)
        assert!(apc.starts_with(b"\x1b_G"));
        // Must end with ESC \ (ST)
        assert!(apc.ends_with(b"\x1b\\"));
        // Must contain action=delete, delete=Placement
        let middle = &apc[3..apc.len() - 2];
        let middle_str = std::str::from_utf8(middle).unwrap();
        assert!(middle_str.contains("a=d"));
        assert!(middle_str.contains("d=P"));
        assert!(middle_str.contains("i=42"));
        assert!(middle_str.contains("p=7"));
    }

    #[test]
    fn images_needing_resize_update_with_placements() {
        let mut store = make_store_with_images(&[1, 2, 3]);
        // Add placement to image 2 only
        let placement = make_placement(1, Some(10), Some(5));
        store.add_placement(2, placement);

        let result = images_needing_resize_update(&store);
        assert_eq!(result.len(), 1);
        assert!(result.contains(&2));
    }

    #[test]
    fn images_needing_resize_update_no_placements() {
        let store = make_store_with_images(&[1, 2, 3]);
        let result = images_needing_resize_update(&store);
        assert!(result.is_empty());
    }

    #[test]
    fn placement_fits_pane_within_bounds() {
        let placement = make_placement(1, Some(10), Some(5));
        assert!(placement_fits_pane(&placement, 80, 24));
    }

    #[test]
    fn placement_fits_pane_exceeds_bounds() {
        // Columns exceed pane width
        let placement = make_placement(1, Some(100), Some(5));
        assert!(!placement_fits_pane(&placement, 80, 24));
    }

    #[test]
    fn placement_fits_pane_none_dimensions() {
        // None columns/rows → always fits (no constraint)
        let placement = make_placement(1, None, None);
        assert!(placement_fits_pane(&placement, 80, 24));
        assert!(placement_fits_pane(&placement, 1, 1));
    }
}
