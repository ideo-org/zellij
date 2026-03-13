use super::store::KittyImageStore;

/// Clean up all Kitty graphics resources for a pane.
/// Called when a pane is closed.
/// Returns the image IDs that were removed (for logging/debugging).
pub fn cleanup_on_close(store: &mut KittyImageStore) -> Vec<u32> {
    let ids = store.all_image_ids();
    store.remove_all();
    ids
}

/// Build delete-all passthrough APC to send to host terminal.
/// When a pane closes, we need to tell the host terminal to remove
/// all images that belonged to this pane.
pub fn build_delete_all_passthrough() -> Vec<u8> {
    // \x1b_Ga=d,d=A\x1b\\
    b"\x1b_Ga=d,d=A\x1b\\".to_vec()
}

/// Check if a pane's images should be rendered.
/// Returns false if the pane is hidden (images should be suppressed).
pub fn should_render_images(is_visible: bool) -> bool {
    is_visible
}

#[cfg(test)]
mod tests {
    use super::super::store::ImageFormat;
    use super::*;

    fn make_store_with_images(ids: &[u32]) -> KittyImageStore {
        let mut store = KittyImageStore::new();
        for &id in ids {
            store.store(Some(id), ImageFormat::Png, vec![0; 10]);
        }
        store
    }

    #[test]
    fn test_cleanup_on_close_empties_store() {
        let mut store = make_store_with_images(&[1, 2, 3]);
        assert_eq!(store.image_count(), 3);
        cleanup_on_close(&mut store);
        assert_eq!(store.image_count(), 0);
        assert_eq!(store.total_memory_bytes(), 0);
    }

    #[test]
    fn test_cleanup_on_close_returns_correct_ids() {
        let mut store = make_store_with_images(&[10, 20, 30]);
        let mut removed = cleanup_on_close(&mut store);
        removed.sort();
        assert_eq!(removed, vec![10, 20, 30]);
    }

    #[test]
    fn test_cleanup_on_close_empty_store() {
        let mut store = KittyImageStore::new();
        let removed = cleanup_on_close(&mut store);
        assert!(removed.is_empty());
        assert_eq!(store.image_count(), 0);
    }

    #[test]
    fn test_build_delete_all_passthrough_format() {
        let data = build_delete_all_passthrough();
        // Must start with ESC _ G (APC graphics)
        assert!(data.starts_with(b"\x1b_G"));
        // Must end with ESC \ (ST)
        assert!(data.ends_with(b"\x1b\\"));
        // Must contain action=delete, delete=All
        let middle = &data[3..data.len() - 2];
        let middle_str = std::str::from_utf8(middle).unwrap();
        assert!(middle_str.contains("a=d"));
        assert!(middle_str.contains("d=A"));
    }

    #[test]
    fn test_should_render_images_visible() {
        assert!(should_render_images(true));
    }

    #[test]
    fn test_should_render_images_hidden() {
        assert!(!should_render_images(false));
    }
}
