//! Scroll integration for Kitty graphics protocol.
//!
//! Handles cleanup of unreferenced images when lines are evicted from
//! the scrollback buffer. The U+10EEEE placeholder characters scroll
//! naturally with text — this module provides the mechanism to detect
//! when evicted lines contained image references and clean up images
//! that are no longer referenced by any placement.

use std::collections::HashSet;

use super::placeholder::{fg_color_to_image_id, KITTY_PLACEHOLDER_CHAR};
use super::store::KittyImageStore;

/// Called when lines are evicted from the scrollback buffer.
/// For each evicted image ID, checks if the image has no remaining placements
/// and removes it from the store if so.
///
/// Returns the list of image IDs that were fully removed from the store.
pub fn cleanup_evicted_images(store: &mut KittyImageStore, evicted_image_ids: &[u32]) -> Vec<u32> {
    let unique_ids: HashSet<u32> = evicted_image_ids.iter().copied().collect();
    let mut removed = Vec::new();

    for image_id in unique_ids {
        if store.remove_if_no_placements(image_id) {
            removed.push(image_id);
        }
    }

    removed
}

/// Extract image IDs from evicted lines.
///
/// Scans each line for KITTY_PLACEHOLDER_CHAR (U+10EEEE) and extracts the
/// image ID from the associated foreground color (r, g, b) encoding.
///
/// Each element in `lines` is a vec of `(char, (u8, u8, u8))` tuples
/// representing the character and its foreground color.
///
/// Returns a deduplicated list of image IDs found.
pub fn extract_image_ids_from_lines(lines: &[Vec<(char, (u8, u8, u8))>]) -> Vec<u32> {
    let mut ids = HashSet::new();

    for line in lines {
        for &(c, (r, g, b)) in line {
            if c == KITTY_PLACEHOLDER_CHAR {
                let image_id = fg_color_to_image_id(r, g, b);
                if image_id > 0 {
                    ids.insert(image_id);
                }
            }
        }
    }

    ids.into_iter().collect()
}

/// Check if an image is still referenced by any active placements.
pub fn image_has_active_placements(store: &KittyImageStore, image_id: u32) -> bool {
    match store.get(image_id) {
        Some(image) => !image.placements.is_empty(),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::super::store::{ImageFormat, KittyImageStore, KittyPlacement};
    use super::*;

    fn make_placement(id: u32) -> KittyPlacement {
        KittyPlacement {
            placement_id: id,
            columns: None,
            rows: None,
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

    #[test]
    fn test_cleanup_evicted_images_removes_no_placement_image() {
        let mut store = KittyImageStore::new();
        // Image with no placements — should be removed
        store.store(Some(10), ImageFormat::Png, vec![0; 16]);
        assert!(store.get(10).is_some());

        let removed = cleanup_evicted_images(&mut store, &[10]);
        assert_eq!(removed, vec![10]);
        assert!(store.get(10).is_none());
    }

    #[test]
    fn test_cleanup_evicted_images_keeps_image_with_placements() {
        let mut store = KittyImageStore::new();
        store.store(Some(20), ImageFormat::Png, vec![0; 16]);
        store.add_placement(20, make_placement(1));

        let removed = cleanup_evicted_images(&mut store, &[20]);
        assert!(removed.is_empty());
        assert!(store.get(20).is_some());
    }

    #[test]
    fn test_cleanup_evicted_images_with_empty_list() {
        let mut store = KittyImageStore::new();
        store.store(Some(30), ImageFormat::Png, vec![0; 8]);

        let removed = cleanup_evicted_images(&mut store, &[]);
        assert!(removed.is_empty());
        assert_eq!(store.image_count(), 1);
    }

    #[test]
    fn test_cleanup_evicted_images_deduplicates() {
        let mut store = KittyImageStore::new();
        store.store(Some(40), ImageFormat::Png, vec![0; 8]);

        // Same ID appears multiple times in evicted list
        let removed = cleanup_evicted_images(&mut store, &[40, 40, 40]);
        assert_eq!(removed.len(), 1);
        assert!(removed.contains(&40));
        assert!(store.get(40).is_none());
    }

    #[test]
    fn test_cleanup_evicted_images_nonexistent_id() {
        let mut store = KittyImageStore::new();
        store.store(Some(50), ImageFormat::Png, vec![0; 8]);

        // ID 999 doesn't exist — should not error
        let removed = cleanup_evicted_images(&mut store, &[999]);
        assert!(removed.is_empty());
        assert_eq!(store.image_count(), 1);
    }

    #[test]
    fn test_extract_image_ids_with_placeholders() {
        use super::super::placeholder::image_id_to_fg_color;

        let fg1 = image_id_to_fg_color(42);
        let fg2 = image_id_to_fg_color(100);

        let lines = vec![
            vec![
                (KITTY_PLACEHOLDER_CHAR, fg1),
                ('A', (255, 255, 255)),
                (KITTY_PLACEHOLDER_CHAR, fg2),
            ],
            vec![
                (KITTY_PLACEHOLDER_CHAR, fg1), // duplicate of 42
            ],
        ];

        let mut ids = extract_image_ids_from_lines(&lines);
        ids.sort();
        assert_eq!(ids, vec![42, 100]);
    }

    #[test]
    fn test_extract_image_ids_no_placeholders() {
        let lines = vec![
            vec![('A', (255, 0, 0)), ('B', (0, 255, 0))],
            vec![('C', (0, 0, 255))],
        ];

        let ids = extract_image_ids_from_lines(&lines);
        assert!(ids.is_empty());
    }

    #[test]
    fn test_extract_image_ids_empty_lines() {
        let lines: Vec<Vec<(char, (u8, u8, u8))>> = vec![];
        let ids = extract_image_ids_from_lines(&lines);
        assert!(ids.is_empty());
    }

    #[test]
    fn test_image_has_active_placements_true() {
        let mut store = KittyImageStore::new();
        store.store(Some(60), ImageFormat::Png, vec![0; 8]);
        store.add_placement(60, make_placement(1));

        assert!(image_has_active_placements(&store, 60));
    }

    #[test]
    fn test_image_has_active_placements_false_no_placements() {
        let mut store = KittyImageStore::new();
        store.store(Some(70), ImageFormat::Png, vec![0; 8]);

        assert!(!image_has_active_placements(&store, 70));
    }

    #[test]
    fn test_image_has_active_placements_false_nonexistent() {
        let store = KittyImageStore::new();
        assert!(!image_has_active_placements(&store, 999));
    }

    #[test]
    fn test_cleanup_mixed_eviction() {
        let mut store = KittyImageStore::new();
        // Image 1: no placements — should be removed
        store.store(Some(1), ImageFormat::Png, vec![0; 10]);
        // Image 2: has placement — should be kept
        store.store(Some(2), ImageFormat::Png, vec![0; 20]);
        store.add_placement(2, make_placement(1));
        // Image 3: no placements — should be removed
        store.store(Some(3), ImageFormat::Png, vec![0; 30]);

        let mut removed = cleanup_evicted_images(&mut store, &[1, 2, 3]);
        removed.sort();
        assert_eq!(removed, vec![1, 3]);
        assert!(store.get(1).is_none());
        assert!(store.get(2).is_some());
        assert!(store.get(3).is_none());
        assert_eq!(store.image_count(), 1);
        assert_eq!(store.total_memory_bytes(), 20);
    }
}
