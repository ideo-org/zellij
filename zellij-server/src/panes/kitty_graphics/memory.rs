use super::store::KittyImageStore;

/// Enforce memory limits by evicting LRU images.
/// Returns the IDs of evicted images (for passthrough delete commands).
pub fn enforce_memory_limit(store: &mut KittyImageStore) -> Vec<u32> {
    store.evict_lru_if_needed()
}

/// Build delete passthrough APC for an evicted image.
/// Tells the host terminal to remove the image.
/// Format: \x1b_Ga=d,d=I,i=<id>\x1b\\
pub fn build_eviction_delete_apc(image_id: u32) -> Vec<u8> {
    let out = format!("\x1b_Ga=d,d=I,i={}\x1b\\", image_id);
    out.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::super::store::ImageFormat;
    use super::*;

    #[test]
    fn evict_lru_when_over_limit() {
        let mut store = KittyImageStore::new();
        store.set_max_memory(100);

        // Store 3 images of 50 bytes each (total 150 > 100 limit)
        store.store(Some(1), ImageFormat::Png, vec![0; 50]);
        store.store(Some(2), ImageFormat::Png, vec![0; 50]);
        store.store(Some(3), ImageFormat::Png, vec![0; 50]);

        // LRU order after stores: [1, 2, 3]
        // Total = 150, limit = 100 → must evict oldest
        let evicted = enforce_memory_limit(&mut store);
        assert!(evicted.contains(&1), "oldest image (1) should be evicted");
        assert!(store.get(1).is_none(), "image 1 should be gone");
        assert!(store.get(2).is_some(), "image 2 should remain");
        assert!(store.get(3).is_some(), "image 3 should remain");
        assert_eq!(store.total_memory_bytes(), 100);
    }

    #[test]
    fn no_eviction_under_limit() {
        let mut store = KittyImageStore::new();
        store.set_max_memory(200);

        store.store(Some(1), ImageFormat::Png, vec![0; 50]);
        store.store(Some(2), ImageFormat::Png, vec![0; 50]);

        // Total = 100, limit = 200 → no eviction needed
        let evicted = enforce_memory_limit(&mut store);
        assert!(evicted.is_empty(), "nothing should be evicted");
        assert_eq!(store.image_count(), 2);
        assert_eq!(store.total_memory_bytes(), 100);
    }

    #[test]
    fn touch_updates_lru_order() {
        let mut store = KittyImageStore::new();
        store.set_max_memory(100);

        // Store A=1, B=2, C=3 (50 bytes each, total 150)
        store.store(Some(1), ImageFormat::Png, vec![0; 50]);
        store.store(Some(2), ImageFormat::Png, vec![0; 50]);
        store.store(Some(3), ImageFormat::Png, vec![0; 50]);

        // Touch A (image 1) — moves it to most-recently-used
        // LRU order becomes [2, 3, 1]
        store.touch(1);

        // Evict to get under limit → should evict 2 (now oldest), not 1
        let evicted = enforce_memory_limit(&mut store);
        assert!(
            evicted.contains(&2),
            "image 2 should be evicted (oldest after touch)"
        );
        assert!(
            !evicted.contains(&1),
            "image 1 should NOT be evicted (was touched)"
        );
        assert!(store.get(1).is_some(), "image 1 should remain");
        assert!(store.get(2).is_none(), "image 2 should be gone");
        assert!(store.get(3).is_some(), "image 3 should remain");
    }

    #[test]
    fn build_eviction_delete_apc_format() {
        let apc = build_eviction_delete_apc(42);
        let apc_str = String::from_utf8(apc.clone()).unwrap();

        // Must start with ESC_G
        assert!(apc_str.starts_with("\x1b_G"), "should start with ESC_G");
        // Must end with ESC backslash
        assert!(apc_str.ends_with("\x1b\\"), "should end with ESC\\");
        // Must contain delete action
        assert!(
            apc_str.contains("a=d"),
            "should contain a=d (delete action)"
        );
        // Must contain delete by ID
        assert!(apc_str.contains("d=I"), "should contain d=I (delete by ID)");
        // Must contain the image ID
        assert!(apc_str.contains("i=42"), "should contain i=42");

        // Full format check
        assert_eq!(apc_str, "\x1b_Ga=d,d=I,i=42\x1b\\");
    }

    #[test]
    fn set_max_memory_changes_limit() {
        let mut store = KittyImageStore::new();

        // Default limit is 320MB
        assert_eq!(store.max_memory_bytes(), 320 * 1024 * 1024);
        assert!(!store.is_over_limit());

        // Set low limit
        store.set_max_memory(50);
        assert_eq!(store.max_memory_bytes(), 50);

        // Store data that exceeds new limit
        store.store(Some(1), ImageFormat::Png, vec![0; 100]);
        assert!(store.is_over_limit(), "100 bytes > 50 byte limit");

        // Raise limit above current usage
        store.set_max_memory(200);
        assert!(!store.is_over_limit(), "100 bytes < 200 byte limit");
    }

    #[test]
    fn store_with_eviction_auto_evicts() {
        let mut store = KittyImageStore::new();
        store.set_max_memory(80);

        store.store(Some(1), ImageFormat::Png, vec![0; 40]);
        store.store(Some(2), ImageFormat::Png, vec![0; 40]);

        // store_with_eviction stores + evicts in one call
        let (id, evicted) = store.store_with_eviction(Some(3), ImageFormat::Png, vec![0; 40]);
        assert_eq!(id, 3);
        // Total was 120 after store, limit 80 → must evict oldest (1), maybe 2
        assert!(evicted.contains(&1), "oldest image should be evicted");
        assert!(store.total_memory_bytes() <= 80);
    }

    #[test]
    fn eviction_removes_from_access_order() {
        let mut store = KittyImageStore::new();
        store.set_max_memory(60);

        store.store(Some(1), ImageFormat::Png, vec![0; 30]);
        store.store(Some(2), ImageFormat::Png, vec![0; 30]);
        store.store(Some(3), ImageFormat::Png, vec![0; 30]);

        // Total = 90, limit = 60 → evict 1
        let evicted = enforce_memory_limit(&mut store);
        assert_eq!(evicted, vec![1]);

        // Now remove_all and verify access_order is also cleared
        store.remove_all();
        assert_eq!(store.image_count(), 0);
        assert_eq!(store.total_memory_bytes(), 0);

        // Re-add — should work cleanly without stale access_order entries
        store.store(Some(10), ImageFormat::Png, vec![0; 20]);
        assert_eq!(store.image_count(), 1);
    }
}
