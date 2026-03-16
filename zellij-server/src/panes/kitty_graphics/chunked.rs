/// Chunked transmission assembly for Kitty graphics protocol.
///
/// Large images are sent across multiple APC commands using the `m` key:
/// - `m=1` means more chunks follow
/// - `m=0` means this is the final chunk
///
/// Chunks are raw base64 text that must be concatenated BEFORE decoding.
/// This module handles buffering and assembly; the caller decodes the result.
use std::collections::HashMap;
use std::time::Instant;

/// Result of adding a chunk to the assembler.
#[derive(Debug, PartialEq)]
pub enum ChunkResult {
    /// More chunks expected for this transmission.
    Buffered,
    /// All chunks assembled — raw concatenated bytes ready for base64 decode.
    Complete(Vec<u8>),
}

/// In-progress transmission being assembled from multiple chunks.
#[derive(Debug, Clone)]
pub struct PendingTransmission {
    pub image_id: Option<u32>,
    /// Raw base64 chunks (NOT decoded), in arrival order.
    pub chunks: Vec<Vec<u8>>,
    pub created_at: Instant,
}

/// Manages assembly of multi-chunk image transmissions.
///
/// Each transmission is keyed by `image_id`. When `image_id` is `None` or `0`,
/// a temporary internal ID is assigned so chunks can still be grouped.
#[derive(Clone)]
pub struct ChunkAssembler {
    pending: HashMap<u32, PendingTransmission>,
    next_temp_id: u32,
}

impl ChunkAssembler {
    pub fn new() -> Self {
        ChunkAssembler {
            pending: HashMap::new(),
            // Start temp IDs high to avoid collision with real image IDs
            next_temp_id: u32::MAX / 2,
        }
    }

    /// Add a chunk to the assembler.
    ///
    /// - `image_id`: The image identifier from the `i=` key. `None` or `Some(0)` means unspecified.
    /// - `chunk_data`: Raw base64 bytes from the APC payload.
    /// - `more`: `true` if `m=1` (more chunks follow), `false` if `m=0` (final chunk).
    ///
    /// Returns `ChunkResult::Buffered` when more chunks are expected, or
    /// `ChunkResult::Complete(data)` with the concatenated raw payload when done.
    pub fn add_chunk(
        &mut self,
        image_id: Option<u32>,
        chunk_data: &[u8],
        more: bool,
    ) -> ChunkResult {
        let key = self.resolve_key(image_id);

        if more {
            // Buffer this chunk, more to come
            let entry = self
                .pending
                .entry(key)
                .or_insert_with(|| PendingTransmission {
                    image_id,
                    chunks: Vec::new(),
                    created_at: Instant::now(),
                });
            entry.chunks.push(chunk_data.to_vec());
            ChunkResult::Buffered
        } else {
            // Final chunk — assemble everything
            let mut assembled = if let Some(pending) = self.pending.remove(&key) {
                // Pre-calculate total size for efficient allocation
                let total_len: usize =
                    pending.chunks.iter().map(|c| c.len()).sum::<usize>() + chunk_data.len();
                let mut buf = Vec::with_capacity(total_len);
                for chunk in pending.chunks {
                    buf.extend_from_slice(&chunk);
                }
                buf
            } else {
                Vec::with_capacity(chunk_data.len())
            };
            assembled.extend_from_slice(chunk_data);
            ChunkResult::Complete(assembled)
        }
    }

    /// Remove transmissions older than `timeout_secs` seconds.
    pub fn cleanup_stale(&mut self, timeout_secs: u64) {
        let cutoff = std::time::Duration::from_secs(timeout_secs);
        let now = Instant::now();
        self.pending
            .retain(|_, v| now.duration_since(v.created_at) < cutoff);
    }

    /// Number of in-progress (pending) transmissions.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Resolve an image_id into a HashMap key.
    /// `None` or `Some(0)` gets a temporary internal ID.
    fn resolve_key(&mut self, image_id: Option<u32>) -> u32 {
        match image_id {
            Some(id) if id != 0 => id,
            _ => {
                // Reuse an existing anonymous in-progress transmission, if one exists.
                if let Some((id, _)) = self
                    .pending
                    .iter()
                    .find(|(_, pending)| pending.image_id.is_none() || pending.image_id == Some(0))
                {
                    return *id;
                }

                // No matching anonymous transmission: allocate a fresh temporary ID.
                let id = self.next_temp_id;
                self.next_temp_id = self.next_temp_id.wrapping_add(1);
                id
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kitty_chunk_single_chunk_complete() {
        // Single chunk with m=0 should immediately return Complete
        let mut assembler = ChunkAssembler::new();
        let result = assembler.add_chunk(Some(1), b"SGVsbG8=", false);
        assert_eq!(result, ChunkResult::Complete(b"SGVsbG8=".to_vec()));
        assert_eq!(assembler.pending_count(), 0);
    }

    #[test]
    fn kitty_chunk_two_chunks_assembly() {
        // m=1 then m=0: should concatenate both chunks
        let mut assembler = ChunkAssembler::new();

        let r1 = assembler.add_chunk(Some(42), b"AAAA", true);
        assert_eq!(r1, ChunkResult::Buffered);
        assert_eq!(assembler.pending_count(), 1);

        let r2 = assembler.add_chunk(Some(42), b"BBBB", false);
        assert_eq!(r2, ChunkResult::Complete(b"AAAABBBB".to_vec()));
        assert_eq!(assembler.pending_count(), 0);
    }

    #[test]
    fn kitty_chunk_three_chunks_order() {
        // Three chunks should be assembled in order
        let mut assembler = ChunkAssembler::new();

        assembler.add_chunk(Some(7), b"chunk1", true);
        assembler.add_chunk(Some(7), b"chunk2", true);
        let result = assembler.add_chunk(Some(7), b"chunk3", false);

        assert_eq!(
            result,
            ChunkResult::Complete(b"chunk1chunk2chunk3".to_vec())
        );
    }

    #[test]
    fn kitty_chunk_multiple_images_simultaneous() {
        // Two different image_ids being chunked at the same time
        let mut assembler = ChunkAssembler::new();

        assembler.add_chunk(Some(10), b"img10_a", true);
        assembler.add_chunk(Some(20), b"img20_a", true);
        assert_eq!(assembler.pending_count(), 2);

        assembler.add_chunk(Some(10), b"img10_b", true);
        assembler.add_chunk(Some(20), b"img20_b", true);

        let r10 = assembler.add_chunk(Some(10), b"img10_c", false);
        assert_eq!(
            r10,
            ChunkResult::Complete(b"img10_aimg10_bimg10_c".to_vec())
        );
        assert_eq!(assembler.pending_count(), 1);

        let r20 = assembler.add_chunk(Some(20), b"img20_c", false);
        assert_eq!(
            r20,
            ChunkResult::Complete(b"img20_aimg20_bimg20_c".to_vec())
        );
        assert_eq!(assembler.pending_count(), 0);
    }

    #[test]
    fn kitty_chunk_cleanup_stale() {
        let mut assembler = ChunkAssembler::new();

        assembler.add_chunk(Some(1), b"data", true);
        assert_eq!(assembler.pending_count(), 1);

        // Cleanup with 0-second timeout should remove everything
        // (Instant::now() is always >= created_at)
        assembler.cleanup_stale(0);
        assert_eq!(assembler.pending_count(), 0);
    }

    #[test]
    fn kitty_chunk_cleanup_preserves_recent() {
        let mut assembler = ChunkAssembler::new();

        assembler.add_chunk(Some(1), b"data", true);
        assert_eq!(assembler.pending_count(), 1);

        // Cleanup with a large timeout should keep the entry
        assembler.cleanup_stale(3600);
        assert_eq!(assembler.pending_count(), 1);
    }

    #[test]
    fn kitty_chunk_pending_count_tracks() {
        let mut assembler = ChunkAssembler::new();
        assert_eq!(assembler.pending_count(), 0);

        assembler.add_chunk(Some(1), b"a", true);
        assert_eq!(assembler.pending_count(), 1);

        assembler.add_chunk(Some(2), b"b", true);
        assert_eq!(assembler.pending_count(), 2);

        // Completing image 1 reduces count
        assembler.add_chunk(Some(1), b"c", false);
        assert_eq!(assembler.pending_count(), 1);

        // Completing image 2 reduces count
        assembler.add_chunk(Some(2), b"d", false);
        assert_eq!(assembler.pending_count(), 0);
    }

    #[test]
    fn kitty_chunk_empty_data_handled() {
        let mut assembler = ChunkAssembler::new();

        // Empty chunk buffered
        let r1 = assembler.add_chunk(Some(5), b"", true);
        assert_eq!(r1, ChunkResult::Buffered);

        // Non-empty followed by empty final
        let r2 = assembler.add_chunk(Some(5), b"", false);
        assert_eq!(r2, ChunkResult::Complete(b"".to_vec()));
    }

    #[test]
    fn kitty_chunk_concatenation_no_separators() {
        // Verify raw byte concatenation — no newlines, spaces, or other separators
        let mut assembler = ChunkAssembler::new();

        assembler.add_chunk(Some(99), b"QUFB", true); // "AAA" in base64
        assembler.add_chunk(Some(99), b"QkJC", true); // "BBB" in base64
        let result = assembler.add_chunk(Some(99), b"Q0ND", false); // "CCC" in base64

        // Must be exact concatenation with no separators
        assert_eq!(result, ChunkResult::Complete(b"QUFBQkJCQ0ND".to_vec()));
    }

    #[test]
    fn kitty_chunk_single_immediate_no_prior_buffer() {
        // m=0 with no prior m=1 for this id — should just return the data
        let mut assembler = ChunkAssembler::new();
        let result = assembler.add_chunk(Some(100), b"complete_payload", false);
        assert_eq!(result, ChunkResult::Complete(b"complete_payload".to_vec()));
        assert_eq!(assembler.pending_count(), 0);
    }

    #[test]
    fn kitty_chunk_image_id_zero_treated_as_anonymous() {
        // image_id=0 should be treated the same as None
        let mut assembler = ChunkAssembler::new();
        let result = assembler.add_chunk(Some(0), b"anon_data", false);
        assert_eq!(result, ChunkResult::Complete(b"anon_data".to_vec()));
    }

    #[test]
    fn kitty_chunk_image_id_none_treated_as_anonymous() {
        let mut assembler = ChunkAssembler::new();
        let result = assembler.add_chunk(None, b"anon_data", false);
        assert_eq!(result, ChunkResult::Complete(b"anon_data".to_vec()));
    }

    #[test]
    fn kitty_chunk_anonymous_multi_chunk() {
        let mut assembler = ChunkAssembler::new();

        let r1 = assembler.add_chunk(None, b"first", true);
        assert_eq!(r1, ChunkResult::Buffered);
        assert_eq!(assembler.pending_count(), 1);

        let r2 = assembler.add_chunk(Some(0), b"second", false);
        assert_eq!(r2, ChunkResult::Complete(b"firstsecond".to_vec()));
        assert_eq!(assembler.pending_count(), 0);
    }
}
