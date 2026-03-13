/// Kitty graphics protocol delete action (a=d) handler.
///
/// Implements all 6 delete sub-actions, removing images and/or placements
/// from KittyImageStore and returning metadata about what was removed so
/// callers can clean up placeholder characters from the grid.
use super::command::{DeleteTarget, KittyCommand};
use super::store::KittyImageStore;

/// Result of a delete operation, describing what was removed from the store.
/// Callers can use this information to clean up placeholder characters from the grid.
#[derive(Debug, Default, PartialEq)]
pub struct DeleteResult {
    /// Image IDs that were fully removed from the store
    pub removed_image_ids: Vec<u32>,
    /// (image_id, placement_id) pairs that were removed
    pub removed_placement_ids: Vec<(u32, u32)>,
}

/// Handle a kitty graphics delete action (a=d).
///
/// Dispatches on `cmd.delete_target` to remove images and/or placements
/// from the store. Returns a `DeleteResult` describing what was removed,
/// so callers can clean up placeholder characters from the grid.
pub fn handle_delete(
    cmd: &KittyCommand,
    store: &mut KittyImageStore,
    _cursor_row: u32,
    _cursor_col: u32,
) -> DeleteResult {
    match &cmd.delete_target {
        None | Some(DeleteTarget::All) => delete_all(store),
        Some(DeleteTarget::ById) => delete_by_id(cmd, store),
        Some(DeleteTarget::ByPlacement) => delete_by_placement(cmd, store),
        Some(DeleteTarget::AtCursor) => delete_at_cursor(cmd, store),
        Some(DeleteTarget::InRange) => delete_in_range(cmd, store),
        Some(DeleteTarget::ByZIndex) => delete_by_z_index(cmd, store),
    }
}

/// d=A (or no target): Remove ALL images from the store.
fn delete_all(store: &mut KittyImageStore) -> DeleteResult {
    let removed_image_ids = store.all_image_ids();
    store.remove_all();
    DeleteResult {
        removed_image_ids,
        removed_placement_ids: vec![],
    }
}

/// d=I: Remove a specific image by its ID.
fn delete_by_id(cmd: &KittyCommand, store: &mut KittyImageStore) -> DeleteResult {
    let id = match cmd.image_id {
        Some(id) => id,
        None => return DeleteResult::default(),
    };

    if store.remove(id) {
        DeleteResult {
            removed_image_ids: vec![id],
            removed_placement_ids: vec![],
        }
    } else {
        DeleteResult::default()
    }
}

/// d=P: Remove a specific placement. If no placements remain, remove the image too.
fn delete_by_placement(cmd: &KittyCommand, store: &mut KittyImageStore) -> DeleteResult {
    let image_id = match cmd.image_id {
        Some(id) => id,
        None => return DeleteResult::default(),
    };
    let placement_id = match cmd.placement_id {
        Some(id) => id,
        None => return DeleteResult::default(),
    };

    let mut result = DeleteResult::default();

    if store.remove_placement(image_id, placement_id) {
        result.removed_placement_ids.push((image_id, placement_id));

        // If no placements remain, remove the whole image
        let should_remove_image = store
            .get(image_id)
            .map(|img| img.placements.is_empty())
            .unwrap_or(false);

        if should_remove_image {
            store.remove(image_id);
            result.removed_image_ids.push(image_id);
        }
    }

    result
}

/// d=C: Remove placements at cursor position.
/// Conservative fallback: without grid position tracking (Task 12),
/// we can't determine which placements intersect the cursor.
/// If image_id is provided, delete that specific image; otherwise remove all.
fn delete_at_cursor(cmd: &KittyCommand, store: &mut KittyImageStore) -> DeleteResult {
    if let Some(id) = cmd.image_id {
        if store.remove(id) {
            return DeleteResult {
                removed_image_ids: vec![id],
                removed_placement_ids: vec![],
            };
        }
        DeleteResult::default()
    } else {
        delete_all(store)
    }
}

/// d=R: Remove placements in a cell range.
/// Conservative fallback: same approach as AtCursor — without grid tracking
/// we can't determine range intersections.
fn delete_in_range(cmd: &KittyCommand, store: &mut KittyImageStore) -> DeleteResult {
    if let Some(id) = cmd.image_id {
        if store.remove(id) {
            return DeleteResult {
                removed_image_ids: vec![id],
                removed_placement_ids: vec![],
            };
        }
        DeleteResult::default()
    } else {
        delete_all(store)
    }
}

/// d=Z: Remove placements matching a specific z-index.
/// If an image has no remaining placements after removal, the image is also removed.
fn delete_by_z_index(cmd: &KittyCommand, store: &mut KittyImageStore) -> DeleteResult {
    let target_z = cmd.z_index.unwrap_or(0);
    let mut result = DeleteResult::default();

    // First pass: collect (image_id, [placement_ids_to_remove]) without borrowing store mutably
    let image_ids = store.all_image_ids();
    let mut removals: Vec<(u32, Vec<u32>)> = Vec::new();

    for &image_id in &image_ids {
        if let Some(image) = store.get(image_id) {
            let matching: Vec<u32> = image
                .placements
                .iter()
                .filter(|(_, p)| p.z_index == target_z)
                .map(|(&pid, _)| pid)
                .collect();

            if !matching.is_empty() {
                removals.push((image_id, matching));
            }
        }
    }

    // Second pass: perform removals
    for (image_id, placement_ids) in removals {
        for pid in placement_ids {
            if store.remove_placement(image_id, pid) {
                result.removed_placement_ids.push((image_id, pid));
            }
        }

        // If no placements remain, remove the whole image
        let should_remove = store
            .get(image_id)
            .map(|img| img.placements.is_empty())
            .unwrap_or(false);

        if should_remove {
            store.remove(image_id);
            result.removed_image_ids.push(image_id);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::super::command::KittyCommand;
    use super::super::store::{ImageFormat, KittyImageStore, KittyPlacement};
    use super::*;
    use std::collections::HashSet;

    fn make_placement(id: u32, z_index: i32) -> KittyPlacement {
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
            z_index,
            virtual_placement: false,
        }
    }

    fn make_delete_cmd(
        target: Option<DeleteTarget>,
        image_id: Option<u32>,
        placement_id: Option<u32>,
        z_index: Option<i32>,
    ) -> KittyCommand {
        KittyCommand {
            delete_target: target,
            image_id,
            placement_id,
            z_index,
            ..KittyCommand::default()
        }
    }

    #[test]
    fn kitty_delete_all_clears_store() {
        let mut store = KittyImageStore::new();
        store.store(Some(1), ImageFormat::Png, vec![0; 10]);
        store.store(Some(2), ImageFormat::Png, vec![0; 20]);
        store.store(Some(3), ImageFormat::Png, vec![0; 30]);

        let cmd = make_delete_cmd(Some(DeleteTarget::All), None, None, None);
        let result = handle_delete(&cmd, &mut store, 0, 0);

        assert_eq!(store.image_count(), 0);
        let ids: HashSet<u32> = result.removed_image_ids.into_iter().collect();
        assert_eq!(ids, [1, 2, 3].iter().copied().collect::<HashSet<u32>>());
    }

    #[test]
    fn kitty_delete_all_empty_store_no_panic() {
        let mut store = KittyImageStore::new();
        let cmd = make_delete_cmd(Some(DeleteTarget::All), None, None, None);
        let result = handle_delete(&cmd, &mut store, 0, 0);

        assert!(result.removed_image_ids.is_empty());
        assert!(result.removed_placement_ids.is_empty());
        assert_eq!(store.image_count(), 0);
    }

    #[test]
    fn kitty_delete_none_target_deletes_all() {
        let mut store = KittyImageStore::new();
        store.store(Some(5), ImageFormat::Png, vec![0; 10]);

        let cmd = make_delete_cmd(None, None, None, None);
        let result = handle_delete(&cmd, &mut store, 0, 0);

        assert_eq!(store.image_count(), 0);
        assert_eq!(result.removed_image_ids, vec![5]);
    }

    #[test]
    fn kitty_delete_by_id_removes_specific_image() {
        let mut store = KittyImageStore::new();
        store.store(Some(10), ImageFormat::Png, vec![0; 10]);
        store.store(Some(42), ImageFormat::Png, vec![0; 20]);
        store.store(Some(99), ImageFormat::Png, vec![0; 30]);

        let cmd = make_delete_cmd(Some(DeleteTarget::ById), Some(42), None, None);
        let result = handle_delete(&cmd, &mut store, 0, 0);

        assert_eq!(result.removed_image_ids, vec![42]);
        assert_eq!(store.image_count(), 2);
        assert!(store.get(42).is_none());
        assert!(store.get(10).is_some());
        assert!(store.get(99).is_some());
    }

    #[test]
    fn kitty_delete_by_id_no_image_id_returns_empty() {
        let mut store = KittyImageStore::new();
        store.store(Some(1), ImageFormat::Png, vec![0; 10]);

        let cmd = make_delete_cmd(Some(DeleteTarget::ById), None, None, None);
        let result = handle_delete(&cmd, &mut store, 0, 0);

        assert!(result.removed_image_ids.is_empty());
        assert_eq!(store.image_count(), 1);
    }

    #[test]
    fn kitty_delete_by_id_nonexistent_returns_empty() {
        let mut store = KittyImageStore::new();
        store.store(Some(1), ImageFormat::Png, vec![0; 10]);

        let cmd = make_delete_cmd(Some(DeleteTarget::ById), Some(999), None, None);
        let result = handle_delete(&cmd, &mut store, 0, 0);

        assert!(result.removed_image_ids.is_empty());
        assert_eq!(store.image_count(), 1);
    }

    #[test]
    fn kitty_delete_placement_keeps_image_with_other_placements() {
        let mut store = KittyImageStore::new();
        store.store(Some(5), ImageFormat::Png, vec![0; 10]);
        store.add_placement(5, make_placement(1, 0));
        store.add_placement(5, make_placement(2, 0));

        let cmd = make_delete_cmd(Some(DeleteTarget::ByPlacement), Some(5), Some(1), None);
        let result = handle_delete(&cmd, &mut store, 0, 0);

        assert!(result.removed_image_ids.is_empty()); // Image kept
        assert_eq!(result.removed_placement_ids, vec![(5, 1)]);
        assert_eq!(store.image_count(), 1);
        let img = store.get(5).unwrap();
        assert_eq!(img.placements.len(), 1);
        assert!(img.placements.contains_key(&2));
    }

    #[test]
    fn kitty_delete_placement_removes_image_if_last() {
        let mut store = KittyImageStore::new();
        store.store(Some(5), ImageFormat::Png, vec![0; 10]);
        store.add_placement(5, make_placement(1, 0));

        let cmd = make_delete_cmd(Some(DeleteTarget::ByPlacement), Some(5), Some(1), None);
        let result = handle_delete(&cmd, &mut store, 0, 0);

        assert_eq!(result.removed_image_ids, vec![5]);
        assert_eq!(result.removed_placement_ids, vec![(5, 1)]);
        assert_eq!(store.image_count(), 0);
    }

    #[test]
    fn kitty_delete_by_z_index_removes_matching_placements() {
        let mut store = KittyImageStore::new();
        store.store(Some(1), ImageFormat::Png, vec![0; 10]);
        store.add_placement(1, make_placement(10, 5)); // z=5
        store.add_placement(1, make_placement(11, 0)); // z=0
        store.add_placement(1, make_placement(12, 5)); // z=5

        store.store(Some(2), ImageFormat::Png, vec![0; 10]);
        store.add_placement(2, make_placement(20, 5)); // z=5 (only placement)

        let cmd = make_delete_cmd(Some(DeleteTarget::ByZIndex), None, None, Some(5));
        let result = handle_delete(&cmd, &mut store, 0, 0);

        // Image 1 should still exist (has placement 11 with z=0)
        assert!(store.get(1).is_some());
        let img1 = store.get(1).unwrap();
        assert_eq!(img1.placements.len(), 1);
        assert!(img1.placements.contains_key(&11));

        // Image 2 should be removed (all placements had z=5)
        assert!(store.get(2).is_none());
        assert!(result.removed_image_ids.contains(&2));
        assert!(!result.removed_image_ids.contains(&1));

        // Verify removed placements
        let removed_set: HashSet<(u32, u32)> = result.removed_placement_ids.into_iter().collect();
        assert!(removed_set.contains(&(1, 10)));
        assert!(removed_set.contains(&(1, 12)));
        assert!(removed_set.contains(&(2, 20)));
    }

    #[test]
    fn kitty_delete_by_z_index_default_zero() {
        let mut store = KittyImageStore::new();
        store.store(Some(1), ImageFormat::Png, vec![0; 10]);
        store.add_placement(1, make_placement(1, 0)); // z=0
        store.add_placement(1, make_placement(2, 3)); // z=3

        // No z_index in cmd → defaults to 0
        let cmd = make_delete_cmd(Some(DeleteTarget::ByZIndex), None, None, None);
        let result = handle_delete(&cmd, &mut store, 0, 0);

        assert_eq!(result.removed_placement_ids, vec![(1, 1)]);
        assert!(result.removed_image_ids.is_empty()); // placement 2 still exists
        let img = store.get(1).unwrap();
        assert_eq!(img.placements.len(), 1);
        assert!(img.placements.contains_key(&2));
    }

    #[test]
    fn kitty_delete_placement_missing_ids_returns_empty() {
        let mut store = KittyImageStore::new();
        store.store(Some(1), ImageFormat::Png, vec![0; 10]);

        // No image_id
        let cmd1 = make_delete_cmd(Some(DeleteTarget::ByPlacement), None, Some(1), None);
        let r1 = handle_delete(&cmd1, &mut store, 0, 0);
        assert!(r1.removed_placement_ids.is_empty());

        // No placement_id
        let cmd2 = make_delete_cmd(Some(DeleteTarget::ByPlacement), Some(1), None, None);
        let r2 = handle_delete(&cmd2, &mut store, 0, 0);
        assert!(r2.removed_placement_ids.is_empty());

        assert_eq!(store.image_count(), 1);
    }

    #[test]
    fn kitty_delete_at_cursor_with_image_id() {
        let mut store = KittyImageStore::new();
        store.store(Some(1), ImageFormat::Png, vec![0; 10]);
        store.store(Some(2), ImageFormat::Png, vec![0; 20]);

        let cmd = make_delete_cmd(Some(DeleteTarget::AtCursor), Some(1), None, None);
        let result = handle_delete(&cmd, &mut store, 5, 10);

        assert_eq!(result.removed_image_ids, vec![1]);
        assert_eq!(store.image_count(), 1);
        assert!(store.get(2).is_some());
    }

    #[test]
    fn kitty_delete_at_cursor_without_image_id_removes_all() {
        let mut store = KittyImageStore::new();
        store.store(Some(1), ImageFormat::Png, vec![0; 10]);
        store.store(Some(2), ImageFormat::Png, vec![0; 20]);

        let cmd = make_delete_cmd(Some(DeleteTarget::AtCursor), None, None, None);
        let result = handle_delete(&cmd, &mut store, 5, 10);

        assert_eq!(store.image_count(), 0);
        let ids: HashSet<u32> = result.removed_image_ids.into_iter().collect();
        assert!(ids.contains(&1));
        assert!(ids.contains(&2));
    }
}
