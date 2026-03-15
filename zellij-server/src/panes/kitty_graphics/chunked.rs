/// Chunked transmission assembly for Kitty graphics protocol.
///
/// Large images are sent across multiple APC commands using the `m` key:
/// - `m=1` means more chunks follow
/// - `m=0` means this is the final chunk
///
/// Chunks are raw base64 text that must be concatenated BEFORE decoding.
use crate::panes::kitty_graphics::command::KittyCommand;
use std::collections::HashMap;
use std::time::Instant;

/// Result of adding a chunk to the assembler.
#[derive(Debug)]
pub enum ChunkResult {
    /// More chunks expected for this transmission.
    Buffered,
    /// All chunks assembled — raw concatenated bytes ready for base64 decode.
    /// Returns (first_command, assembled_payload).
    /// first_command contains metadata from the first chunk; use its fields
    /// for format, compression, etc. The payload has been assembled from all chunks.
    Complete(Option<KittyCommand>, Vec<u8>),
}

/// In-progress transmission being assembled from multiple chunks.
#[derive(Debug, Clone)]
pub struct PendingTransmission {
    pub image_id: Option<u32>,
    /// Metadata from the first chunk (format, compression, etc.).
    pub first_command: Option<KittyCommand>,
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
    /// - `cmd`: The full command from this chunk (metadata preserved from first chunk).
    /// - `chunk_data`: Raw base64 bytes from the APC payload.
    /// - `more`: `true` if `m=1` (more chunks follow), `false` if `m=0` (final chunk).
    ///
    /// Returns `ChunkResult::Buffered` when more chunks are expected, or
    /// `ChunkResult::Complete(first_cmd, data)` with the first chunk's command and concatenated payload.
    pub fn add_chunk(&mut self, cmd: &KittyCommand, chunk_data: &[u8], more: bool) -> ChunkResult {
        let key = self.resolve_key(cmd.image_id);

        if more {
            // Buffer this chunk and metadata, more to come
            let entry = self
                .pending
                .entry(key)
                .or_insert_with(|| PendingTransmission {
                    image_id: cmd.image_id,
                    first_command: None,
                    chunks: Vec::new(),
                    created_at: Instant::now(),
                });
            // Preserve command metadata from the first chunk
            if entry.first_command.is_none() {
                entry.first_command = Some(cmd.clone());
            }
            entry.chunks.push(chunk_data.to_vec());
            ChunkResult::Buffered
        } else {
            // Final chunk — assemble everything
            let (first_cmd, mut assembled) = if let Some(pending) = self.pending.remove(&key) {
                // Pre-calculate total size for efficient allocation
                let total_len: usize =
                    pending.chunks.iter().map(|c| c.len()).sum::<usize>() + chunk_data.len();
                let mut buf = Vec::with_capacity(total_len);
                for chunk in pending.chunks {
                    buf.extend_from_slice(&chunk);
                }
                (pending.first_command, buf)
            } else {
                // Single chunk transmission (m=0 without any m=1 chunks)
                (None, Vec::with_capacity(chunk_data.len()))
            };
            assembled.extend_from_slice(chunk_data);
            ChunkResult::Complete(first_cmd, assembled)
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

    // Helper: create a minimal command for testing
    fn test_cmd(image_id: Option<u32>, payload: &[u8], more: bool) -> KittyCommand {
        KittyCommand {
            image_id,
            more_chunks: more,
            payload: payload.to_vec(),
            ..Default::default()
        }
    }

    #[test]
    fn kitty_chunk_single_chunk_complete() {
        // Single chunk with m=0 should immediately return Complete
        let mut assembler = ChunkAssembler::new();
        let cmd = test_cmd(Some(1), b"SGVsbG8=", false);
        let result = assembler.add_chunk(&cmd, b"SGVsbG8=", false);
        match result {
            ChunkResult::Complete(None, payload) => {
                assert_eq!(payload, b"SGVsbG8=".to_vec());
            },
            _ => panic!("Expected Complete without command"),
        }
        assert_eq!(assembler.pending_count(), 0);
    }

    #[test]
    fn kitty_chunk_two_chunks_assembly() {
        // m=1 then m=0: should concatenate both chunks
        let mut assembler = ChunkAssembler::new();

        let cmd1 = test_cmd(Some(42), b"AAAA", true);
        let r1 = assembler.add_chunk(&cmd1, b"AAAA", true);
        match r1 {
            ChunkResult::Buffered => {},
            _ => panic!("Expected Buffered"),
        }
        assert_eq!(assembler.pending_count(), 1);

        let cmd2 = test_cmd(Some(42), b"BBBB", false);
        let r2 = assembler.add_chunk(&cmd2, b"BBBB", false);
        match r2 {
            ChunkResult::Complete(Some(first_cmd), payload) => {
                assert_eq!(first_cmd.image_id, Some(42));
                assert_eq!(payload, b"AAAABBBB".to_vec());
            },
            _ => panic!("Expected Complete with command"),
        }
        assert_eq!(assembler.pending_count(), 0);
    }

    #[test]
    fn kitty_chunk_three_chunks_order() {
        // Three chunks should be assembled in order
        let mut assembler = ChunkAssembler::new();

        let cmd1 = test_cmd(Some(7), b"chunk1", true);
        assembler.add_chunk(&cmd1, b"chunk1", true);
        let cmd2 = test_cmd(Some(7), b"chunk2", true);
        assembler.add_chunk(&cmd2, b"chunk2", true);
        let cmd3 = test_cmd(Some(7), b"chunk3", false);
        let result = assembler.add_chunk(&cmd3, b"chunk3", false);

        match result {
            ChunkResult::Complete(Some(first_cmd), payload) => {
                assert_eq!(first_cmd.image_id, Some(7));
                assert_eq!(payload, b"chunk1chunk2chunk3".to_vec());
            },
            _ => panic!("Expected Complete with command"),
        }
    }

    #[test]
    fn kitty_chunk_multiple_images_simultaneous() {
        // Two different image_ids being chunked at the same time
        let mut assembler = ChunkAssembler::new();

        let cmd10_a = test_cmd(Some(10), b"img10_a", true);
        assembler.add_chunk(&cmd10_a, b"img10_a", true);
        let cmd20_a = test_cmd(Some(20), b"img20_a", true);
        assembler.add_chunk(&cmd20_a, b"img20_a", true);
        assert_eq!(assembler.pending_count(), 2);

        let cmd10_b = test_cmd(Some(10), b"img10_b", true);
        assembler.add_chunk(&cmd10_b, b"img10_b", true);
        let cmd20_b = test_cmd(Some(20), b"img20_b", true);
        assembler.add_chunk(&cmd20_b, b"img20_b", true);

        let cmd10_c = test_cmd(Some(10), b"img10_c", false);
        let r10 = assembler.add_chunk(&cmd10_c, b"img10_c", false);
        match r10 {
            ChunkResult::Complete(Some(first_cmd), payload) => {
                assert_eq!(first_cmd.image_id, Some(10));
                assert_eq!(payload, b"img10_aimg10_bimg10_c".to_vec());
            },
            _ => panic!("Expected Complete with command"),
        }
        assert_eq!(assembler.pending_count(), 1);

        let cmd20_c = test_cmd(Some(20), b"img20_c", false);
        let r20 = assembler.add_chunk(&cmd20_c, b"img20_c", false);
        match r20 {
            ChunkResult::Complete(Some(first_cmd), payload) => {
                assert_eq!(first_cmd.image_id, Some(20));
                assert_eq!(payload, b"img20_aimg20_bimg20_c".to_vec());
            },
            _ => panic!("Expected Complete with command"),
        }
        assert_eq!(assembler.pending_count(), 0);
    }

    #[test]
    fn kitty_chunk_cleanup_stale() {
        let mut assembler = ChunkAssembler::new();

        let cmd = test_cmd(Some(1), b"data", true);
        assembler.add_chunk(&cmd, b"data", true);
        assert_eq!(assembler.pending_count(), 1);

        // Cleanup with 0-second timeout should remove everything
        // (Instant::now() is always >= created_at)
        assembler.cleanup_stale(0);
        assert_eq!(assembler.pending_count(), 0);
    }

    #[test]
    fn kitty_chunk_cleanup_preserves_recent() {
        let mut assembler = ChunkAssembler::new();

        let cmd = test_cmd(Some(1), b"data", true);
        assembler.add_chunk(&cmd, b"data", true);
        assert_eq!(assembler.pending_count(), 1);

        // Cleanup with a large timeout should keep the entry
        assembler.cleanup_stale(3600);
        assert_eq!(assembler.pending_count(), 1);
    }

    #[test]
    fn kitty_chunk_pending_count_tracks() {
        let mut assembler = ChunkAssembler::new();
        assert_eq!(assembler.pending_count(), 0);

        let cmd1 = test_cmd(Some(1), b"a", true);
        assembler.add_chunk(&cmd1, b"a", true);
        assert_eq!(assembler.pending_count(), 1);

        let cmd2 = test_cmd(Some(2), b"b", true);
        assembler.add_chunk(&cmd2, b"b", true);
        assert_eq!(assembler.pending_count(), 2);

        // Completing image 1 reduces count
        let cmd1_final = test_cmd(Some(1), b"c", false);
        assembler.add_chunk(&cmd1_final, b"c", false);
        assert_eq!(assembler.pending_count(), 1);

        // Completing image 2 reduces count
        let cmd2_final = test_cmd(Some(2), b"d", false);
        assembler.add_chunk(&cmd2_final, b"d", false);
        assert_eq!(assembler.pending_count(), 0);
    }

    #[test]
    fn kitty_chunk_empty_data_handled() {
        let mut assembler = ChunkAssembler::new();

        // Empty chunk buffered
        let cmd1 = test_cmd(Some(5), b"", true);
        let r1 = assembler.add_chunk(&cmd1, b"", true);
        match r1 {
            ChunkResult::Buffered => {},
            _ => panic!("Expected Buffered"),
        }

        // Non-empty followed by empty final
        let cmd2 = test_cmd(Some(5), b"", false);
        let r2 = assembler.add_chunk(&cmd2, b"", false);
        match r2 {
            ChunkResult::Complete(Some(first_cmd), payload) => {
                assert_eq!(first_cmd.image_id, Some(5));
                assert_eq!(payload, b"".to_vec());
            },
            _ => panic!("Expected Complete with command"),
        }
    }

    #[test]
    fn kitty_chunk_concatenation_no_separators() {
        // Verify raw byte concatenation — no newlines, spaces, or other separators
        let mut assembler = ChunkAssembler::new();

        let cmd1 = test_cmd(Some(99), b"QUFB", true);
        assembler.add_chunk(&cmd1, b"QUFB", true); // "AAA" in base64
        let cmd2 = test_cmd(Some(99), b"QkJC", true);
        assembler.add_chunk(&cmd2, b"QkJC", true); // "BBB" in base64
        let cmd3 = test_cmd(Some(99), b"Q0ND", false);
        let result = assembler.add_chunk(&cmd3, b"Q0ND", false); // "CCC" in base64

        // Must be exact concatenation with no separators
        match result {
            ChunkResult::Complete(Some(first_cmd), payload) => {
                assert_eq!(first_cmd.image_id, Some(99));
                assert_eq!(payload, b"QUFBQkJCQ0ND".to_vec());
            },
            _ => panic!("Expected Complete with command"),
        }
    }

    #[test]
    fn kitty_chunk_single_immediate_no_prior_buffer() {
        // m=0 with no prior m=1 for this id — should just return the data
        let mut assembler = ChunkAssembler::new();
        let cmd = test_cmd(Some(100), b"complete_payload", false);
        let result = assembler.add_chunk(&cmd, b"complete_payload", false);
        match result {
            ChunkResult::Complete(None, payload) => {
                assert_eq!(payload, b"complete_payload".to_vec());
            },
            _ => panic!("Expected Complete without command"),
        }
        assert_eq!(assembler.pending_count(), 0);
    }

    #[test]
    fn kitty_chunk_image_id_zero_treated_as_anonymous() {
        // image_id=0 should be treated the same as None
        let mut assembler = ChunkAssembler::new();
        let cmd = test_cmd(Some(0), b"anon_data", false);
        let result = assembler.add_chunk(&cmd, b"anon_data", false);
        match result {
            ChunkResult::Complete(None, payload) => {
                assert_eq!(payload, b"anon_data".to_vec());
            },
            _ => panic!("Expected Complete without command"),
        }
    }

    #[test]
    fn kitty_chunk_image_id_none_treated_as_anonymous() {
        let mut assembler = ChunkAssembler::new();
        let cmd = test_cmd(None, b"anon_data", false);
        let result = assembler.add_chunk(&cmd, b"anon_data", false);
        match result {
            ChunkResult::Complete(None, payload) => {
                assert_eq!(payload, b"anon_data".to_vec());
            },
            _ => panic!("Expected Complete without command"),
        }
    }

    #[test]
    fn kitty_chunk_anonymous_multi_chunk() {
        let mut assembler = ChunkAssembler::new();

        let cmd1 = test_cmd(None, b"first", true);
        let r1 = assembler.add_chunk(&cmd1, b"first", true);
        match r1 {
            ChunkResult::Buffered => {},
            _ => panic!("Expected Buffered"),
        }
        assert_eq!(assembler.pending_count(), 1);

        let cmd2 = test_cmd(Some(0), b"second", false);
        let r2 = assembler.add_chunk(&cmd2, b"second", false);
        match r2 {
            ChunkResult::Complete(Some(first_cmd), payload) => {
                assert_eq!(first_cmd.image_id, None);
                assert_eq!(payload, b"firstsecond".to_vec());
            },
            _ => panic!("Expected Complete with command"),
        }
        assert_eq!(assembler.pending_count(), 0);
    }
}
