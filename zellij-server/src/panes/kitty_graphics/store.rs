use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Rgba,
    Rgb,
    Png,
}

impl Default for ImageFormat {
    fn default() -> Self {
        ImageFormat::Png
    }
}

#[derive(Debug, Clone)]
pub struct KittyPlacement {
    pub placement_id: u32,
    pub columns: Option<u32>,
    pub rows: Option<u32>,
    pub source_x: Option<u32>,
    pub source_y: Option<u32>,
    pub source_width: Option<u32>,
    pub source_height: Option<u32>,
    pub cell_offset_x: u32,
    pub cell_offset_y: u32,
    pub z_index: i32,
    pub virtual_placement: bool,
}

#[derive(Debug, Clone)]
pub struct KittyFrame {
    pub frame_number: u32,
    pub data: Vec<u8>,
    pub gap_ms: u32,
}

#[derive(Debug, Clone)]
pub struct KittyImage {
    pub id: u32,
    pub format: ImageFormat,
    pub pixel_width: Option<u32>,
    pub pixel_height: Option<u32>,
    pub data: Vec<u8>,
    pub placements: HashMap<u32, KittyPlacement>,
    pub frames: Vec<KittyFrame>,
}

impl KittyImage {
    pub fn new(id: u32, format: ImageFormat, data: Vec<u8>) -> Self {
        KittyImage {
            id,
            format,
            pixel_width: None,
            pixel_height: None,
            data,
            placements: HashMap::new(),
            frames: vec![],
        }
    }

    pub fn memory_bytes(&self) -> usize {
        self.data.len() + self.frames.iter().map(|f| f.data.len()).sum::<usize>()
    }
}

#[derive(Debug, Clone)]
pub struct KittyImageStore {
    images: HashMap<u32, KittyImage>,
    next_auto_id: u32,
    total_bytes: usize,
    max_memory_bytes: usize,
    access_order: Vec<u32>,
}

impl KittyImageStore {
    pub fn new() -> Self {
        KittyImageStore {
            images: HashMap::new(),
            next_auto_id: 0,
            total_bytes: 0,
            max_memory_bytes: 320 * 1024 * 1024, // 320 MB default
            access_order: Vec::new(),
        }
    }

    /// Store image data. If id is 0 or None, auto-assign.
    /// Returns the assigned image ID.
    pub fn store(&mut self, id: Option<u32>, format: ImageFormat, data: Vec<u8>) -> u32 {
        let actual_id = match id {
            Some(0) | None => {
                self.next_auto_id += 1;
                self.next_auto_id
            },
            Some(id) => id,
        };

        let byte_len = data.len();

        // If replacing an existing image, subtract its old memory
        if let Some(old) = self.images.get(&actual_id) {
            self.total_bytes -= old.memory_bytes();
        }

        let image = KittyImage::new(actual_id, format, data);
        self.total_bytes += byte_len;
        self.images.insert(actual_id, image);
        // Track access order for LRU eviction
        self.access_order.retain(|&id| id != actual_id);
        self.access_order.push(actual_id);

        actual_id
    }

    pub fn get(&self, id: u32) -> Option<&KittyImage> {
        self.images.get(&id)
    }

    pub fn get_mut(&mut self, id: u32) -> Option<&mut KittyImage> {
        self.images.get_mut(&id)
    }

    pub fn remove(&mut self, id: u32) -> bool {
        if let Some(image) = self.images.remove(&id) {
            self.total_bytes -= image.memory_bytes();
            self.access_order.retain(|&x| x != id);
            true
        } else {
            false
        }
    }

    pub fn remove_all(&mut self) {
        self.images.clear();
        self.total_bytes = 0;
        self.access_order.clear();
    }

    pub fn add_placement(&mut self, image_id: u32, placement: KittyPlacement) -> bool {
        if let Some(image) = self.images.get_mut(&image_id) {
            image.placements.insert(placement.placement_id, placement);
            true
        } else {
            false
        }
    }

    pub fn remove_placement(&mut self, image_id: u32, placement_id: u32) -> bool {
        if let Some(image) = self.images.get_mut(&image_id) {
            image.placements.remove(&placement_id).is_some()
        } else {
            false
        }
    }

    pub fn image_count(&self) -> usize {
        self.images.len()
    }

    pub fn total_memory_bytes(&self) -> usize {
        self.total_bytes
    }

    pub fn all_image_ids(&self) -> Vec<u32> {
        self.images.keys().copied().collect()
    }

    pub fn set_max_memory(&mut self, bytes: usize) {
        self.max_memory_bytes = bytes;
    }

    /// Evict LRU images until total memory is under the limit.
    /// Returns the IDs of evicted images.
    pub fn evict_lru_if_needed(&mut self) -> Vec<u32> {
        let mut evicted = Vec::new();
        while self.total_bytes > self.max_memory_bytes && !self.access_order.is_empty() {
            let oldest_id = self.access_order.remove(0);
            if let Some(img) = self.images.remove(&oldest_id) {
                self.total_bytes = self.total_bytes.saturating_sub(img.memory_bytes());
                evicted.push(oldest_id);
            }
        }
        evicted
    }

    /// Remove an image only if it has no active placements.
    /// Returns true if the image was removed.
    pub fn remove_if_no_placements(&mut self, image_id: u32) -> bool {
        let should_remove = self.images.get(&image_id)
            .map(|img| img.placements.is_empty())
            .unwrap_or(false);
        if should_remove {
            self.remove(image_id)
        } else {
            false
        }
    }

    /// Get the current memory limit in bytes.
    pub fn max_memory_bytes(&self) -> usize {
        self.max_memory_bytes
    }

    /// Check if current memory usage exceeds the limit.
    pub fn is_over_limit(&self) -> bool {
        self.total_bytes > self.max_memory_bytes
    }

    /// Mark an image as recently accessed (moves to back of LRU order).
    pub fn touch(&mut self, image_id: u32) {
        if self.images.contains_key(&image_id) {
            self.access_order.retain(|&id| id != image_id);
            self.access_order.push(image_id);
        }
    }

    /// Store an image and immediately evict LRU images if over limit.
    /// Returns (assigned_id, evicted_ids).
    pub fn store_with_eviction(
        &mut self,
        id: Option<u32>,
        format: ImageFormat,
        data: Vec<u8>,
    ) -> (u32, Vec<u32>) {
        let assigned_id = self.store(id, format, data);
        let evicted = self.evict_lru_if_needed();
        (assigned_id, evicted)
    }
}

#[cfg(test)]
mod tests {
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
    fn test_store_and_get() {
        let mut store = KittyImageStore::new();
        let data = vec![1, 2, 3, 4];
        let id = store.store(Some(42), ImageFormat::Png, data.clone());
        assert_eq!(id, 42);
        let image = store.get(42).unwrap();
        assert_eq!(image.id, 42);
        assert_eq!(image.data, data);
        assert_eq!(image.format, ImageFormat::Png);
    }

    #[test]
    fn test_auto_id_assignment() {
        let mut store = KittyImageStore::new();
        let id1 = store.store(None, ImageFormat::Rgba, vec![10, 20]);
        assert!(id1 > 0);
        let id2 = store.store(Some(0), ImageFormat::Rgb, vec![30, 40]);
        assert!(id2 > 0);
        assert_ne!(id1, id2);
        assert!(store.get(id1).is_some());
        assert!(store.get(id2).is_some());
    }

    #[test]
    fn test_remove_clears_memory() {
        let mut store = KittyImageStore::new();
        store.store(Some(1), ImageFormat::Png, vec![0; 50]);
        assert_eq!(store.total_memory_bytes(), 50);
        assert!(store.remove(1));
        assert_eq!(store.total_memory_bytes(), 0);
        assert!(store.get(1).is_none());
    }

    #[test]
    fn test_remove_all() {
        let mut store = KittyImageStore::new();
        store.store(Some(1), ImageFormat::Png, vec![0; 10]);
        store.store(Some(2), ImageFormat::Rgb, vec![0; 20]);
        store.store(Some(3), ImageFormat::Rgba, vec![0; 30]);
        assert_eq!(store.image_count(), 3);
        store.remove_all();
        assert_eq!(store.image_count(), 0);
        assert_eq!(store.total_memory_bytes(), 0);
    }

    #[test]
    fn test_add_placement() {
        let mut store = KittyImageStore::new();
        store.store(Some(5), ImageFormat::Png, vec![0; 10]);
        let placement = make_placement(1);
        assert!(store.add_placement(5, placement));
        let image = store.get(5).unwrap();
        assert_eq!(image.placements.len(), 1);
        assert!(image.placements.contains_key(&1));

        // Adding to nonexistent image returns false
        assert!(!store.add_placement(999, make_placement(2)));
    }

    #[test]
    fn test_memory_tracking() {
        let mut store = KittyImageStore::new();
        store.store(Some(1), ImageFormat::Rgba, vec![0; 100]);
        assert_eq!(store.total_memory_bytes(), 100);
    }
}
