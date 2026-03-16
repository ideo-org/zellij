/// Kitty graphics protocol APC dispatch logic.
///
/// Routes a complete APC payload (captured by ApcParser) to the correct
/// handler based on the parsed command's action field.
use super::animation::{handle_animate, handle_frame};
use super::chunked::ChunkAssembler;
use super::chunked::ChunkResult;
use super::command::{KittyAction, KittyCommand};
use super::delete::handle_delete;
use super::passthrough::build_passthrough_apc;
use super::query::handle_query;
use super::store::KittyImageStore;
use super::transmit::handle_transmit;

/// Result of dispatching a complete APC sequence.
pub struct DispatchResult {
    /// Response bytes to send back to the application (via PTY write).
    /// Empty if quiet mode suppressed the response.
    pub response: Vec<u8>,
    /// Image data that needs to be forwarded to the host terminal.
    /// Empty if no passthrough needed for this action.
    pub passthrough_apc: Vec<u8>,
}

/// Dispatch a complete Kitty graphics APC payload to the appropriate handler.
///
/// `apc_data` is the raw bytes between `ESC_G` and ST (the payload captured
/// by ApcParser), NOT including the `ESC_G` prefix or the ST terminator.
pub fn dispatch_kitty_apc(
    apc_data: &[u8],
    store: &mut KittyImageStore,
    chunk_assembler: &mut ChunkAssembler,
    cursor_row: u32,
    cursor_col: u32,
) -> DispatchResult {
    // 1. Parse the control data
    let cmd = match KittyCommand::parse(apc_data) {
        Ok(cmd) => cmd,
        Err(_) => {
            return DispatchResult {
                response: vec![],
                passthrough_apc: vec![],
            }
        },
    };

    // 2. Handle chunked transmission (m=1)
    let payload = match chunk_assembler.add_chunk(cmd.image_id, &cmd.payload, cmd.more_chunks) {
        ChunkResult::Buffered => {
            return DispatchResult {
                response: vec![],
                passthrough_apc: vec![],
            };
        },
        ChunkResult::Complete(payload) => payload,
    };

    // 3. Dispatch by action
    let action = cmd
        .action
        .clone()
        .unwrap_or(KittyAction::TransmitAndDisplay);
    match action {
        KittyAction::TransmitAndDisplay | KittyAction::Transmit => {
            let response = handle_transmit(&cmd, &payload, store);
            let passthrough_apc = build_passthrough_apc(apc_data);
            DispatchResult {
                response,
                passthrough_apc,
            }
        },
        KittyAction::Place => {
            let passthrough_apc = build_passthrough_apc(apc_data);
            DispatchResult {
                response: vec![],
                passthrough_apc,
            }
        },
        KittyAction::Query => {
            // If query includes payload data (e.g. kitty icat --detect-support),
            // transmit the test image first so the store has it, then respond OK.
            // No passthrough for queries — responses are generated locally.
            if !payload.is_empty() {
                let _ = handle_transmit(&cmd, &payload, store);
            }
            let response = handle_query(&cmd, store);
            DispatchResult {
                response,
                passthrough_apc: vec![],
            }
        },
        KittyAction::Delete => {
            let _result = handle_delete(&cmd, store, cursor_row, cursor_col);
            let passthrough_apc = build_passthrough_apc(apc_data);
            DispatchResult {
                response: vec![],
                passthrough_apc,
            }
        },
        KittyAction::Frame => {
            let response = handle_frame(&cmd, store);
            let passthrough_apc = build_passthrough_apc(apc_data);
            DispatchResult {
                response,
                passthrough_apc,
            }
        },
        KittyAction::Animate => {
            let response = handle_animate(&cmd, store);
            let passthrough_apc = build_passthrough_apc(apc_data);
            DispatchResult {
                response,
                passthrough_apc,
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panes::kitty_graphics::chunked::ChunkAssembler;
    use crate::panes::kitty_graphics::store::KittyImageStore;

    #[test]
    fn dispatch_valid_transmit_stores_image() {
        let mut store = KittyImageStore::new();
        let mut assembler = ChunkAssembler::new();

        // Build APC data: a=T,i=42,f=100;base64-of-"PNG"
        let png_b64 = base64::encode(b"PNG");
        let apc_data = format!("a=T,i=42,f=100;{}", png_b64);

        let result = dispatch_kitty_apc(apc_data.as_bytes(), &mut store, &mut assembler, 0, 0);

        // Image should be stored
        assert!(store.get(42).is_some());
        let image = store.get(42).unwrap();
        assert_eq!(image.data, b"PNG");

        // Response should contain OK
        let resp_str = String::from_utf8_lossy(&result.response);
        assert!(resp_str.contains("OK"));
        assert!(resp_str.contains("i=42"));
    }

    #[test]
    fn dispatch_query_missing_image_returns_error() {
        let mut store = KittyImageStore::new();
        let mut assembler = ChunkAssembler::new();

        // Query for image 999 which doesn't exist
        let apc_data = b"a=q,i=999";

        let result = dispatch_kitty_apc(apc_data, &mut store, &mut assembler, 0, 0);

        // Response should contain ENOENT
        let resp_str = String::from_utf8_lossy(&result.response);
        assert!(resp_str.contains("ENOENT"));
        assert!(resp_str.contains("i=999"));
    }

    #[test]
    fn dispatch_invalid_apc_returns_empty() {
        let mut store = KittyImageStore::new();
        let mut assembler = ChunkAssembler::new();

        // Malformed control data (missing = in key-value pair)
        let apc_data = b"badpair";

        let result = dispatch_kitty_apc(apc_data, &mut store, &mut assembler, 0, 0);

        assert!(result.response.is_empty());
        assert!(result.passthrough_apc.is_empty());
    }

    #[test]
    fn dispatch_delete_all_clears_store() {
        let mut store = KittyImageStore::new();
        let mut assembler = ChunkAssembler::new();

        // First, store an image
        use crate::panes::kitty_graphics::store::ImageFormat;
        store.store(Some(10), ImageFormat::Png, vec![1, 2, 3]);
        assert_eq!(store.image_count(), 1);

        // Delete all
        let apc_data = b"a=d,d=A";
        let _result = dispatch_kitty_apc(apc_data, &mut store, &mut assembler, 0, 0);

        assert_eq!(store.image_count(), 0);
    }

    #[test]
    fn dispatch_chunked_buffers_without_response() {
        let mut store = KittyImageStore::new();
        let mut assembler = ChunkAssembler::new();

        // Send a chunk with m=1 (more coming)
        let apc_data = b"a=T,i=50,m=1;AAAA";
        let result = dispatch_kitty_apc(apc_data, &mut store, &mut assembler, 0, 0);

        // No response for intermediate chunks
        assert!(result.response.is_empty());
        // Image should NOT be stored yet
        assert!(store.get(50).is_none());
        // Assembler should have pending data
        assert_eq!(assembler.pending_count(), 1);
    }

    #[test]
    fn dispatch_chunked_transmit_reassembles_payload() {
        let mut store = KittyImageStore::new();
        let mut assembler = ChunkAssembler::new();

        // First chunk (base64 data split across commands)
        let result = dispatch_kitty_apc(b"a=T,i=42,m=1;UE", &mut store, &mut assembler, 0, 0);
        assert!(result.response.is_empty());
        assert_eq!(store.image_count(), 0);

        // Final chunk should trigger decode and store image data
        let result = dispatch_kitty_apc(b"a=T,i=42;5H", &mut store, &mut assembler, 0, 0);
        assert!(result.response.len() > 0);
        assert_eq!(store.image_count(), 1);
        assert_eq!(store.get(42).unwrap().data, b"PNG");
    }

    #[test]
    fn dispatch_chunked_transmit_without_image_id_reassembles_payload() {
        let mut store = KittyImageStore::new();
        let mut assembler = ChunkAssembler::new();

        let result = dispatch_kitty_apc(b"a=T,m=1;SG", &mut store, &mut assembler, 0, 0);
        assert!(result.response.is_empty());
        assert_eq!(store.image_count(), 0);

        let result =
            dispatch_kitty_apc(b"a=T,m=0;VsbG8ga2l0dHk=", &mut store, &mut assembler, 0, 0);
        assert!(result.response.len() > 0);
        assert_eq!(store.image_count(), 1);
    }

    #[test]
    fn dispatch_transmit_has_passthrough() {
        let mut store = KittyImageStore::new();
        let mut assembler = ChunkAssembler::new();
        let png_b64 = base64::encode(b"PNG");
        let apc_data = format!("a=T,i=42,f=100;{}", png_b64);

        let result = dispatch_kitty_apc(apc_data.as_bytes(), &mut store, &mut assembler, 0, 0);

        // Passthrough APC should be populated for transmit actions
        assert!(!result.passthrough_apc.is_empty());
        assert!(result.passthrough_apc.starts_with(b"\x1b_G"));
        assert!(result.passthrough_apc.ends_with(b"\x1b\\"));
    }

    #[test]
    fn dispatch_query_no_passthrough() {
        let mut store = KittyImageStore::new();
        let mut assembler = ChunkAssembler::new();

        let result = dispatch_kitty_apc(b"a=q,i=1", &mut store, &mut assembler, 0, 0);

        // Query should NOT have passthrough
        assert!(result.passthrough_apc.is_empty());
    }

    #[test]
    fn dispatch_delete_has_passthrough() {
        let mut store = KittyImageStore::new();
        let mut assembler = ChunkAssembler::new();

        let result = dispatch_kitty_apc(b"a=d,d=A", &mut store, &mut assembler, 0, 0);

        // Delete SHOULD have passthrough to tell host terminal to remove images
        assert!(!result.passthrough_apc.is_empty());
        assert!(result.passthrough_apc.starts_with(b"\x1b_G"));
    }

    #[test]
    fn dispatch_frame_stores_frame() {
        let mut store = KittyImageStore::new();
        let mut assembler = ChunkAssembler::new();
        use crate::panes::kitty_graphics::store::ImageFormat;
        store.store(Some(42), ImageFormat::Png, vec![1, 2, 3]);

        let frame_b64 = base64::encode(b"frame_data");
        let apc_data = format!("a=f,i=42;{}", frame_b64);
        let result = dispatch_kitty_apc(apc_data.as_bytes(), &mut store, &mut assembler, 0, 0);

        // Frame should be stored
        assert_eq!(store.get(42).unwrap().frames.len(), 1);
        // Response should contain OK
        let resp_str = String::from_utf8_lossy(&result.response);
        assert!(resp_str.contains("OK"));
        // Passthrough should be populated
        assert!(!result.passthrough_apc.is_empty());
    }

    #[test]
    fn dispatch_animate_returns_ok() {
        let mut store = KittyImageStore::new();
        let mut assembler = ChunkAssembler::new();
        use crate::panes::kitty_graphics::store::ImageFormat;
        store.store(Some(42), ImageFormat::Png, vec![1, 2, 3]);

        let result = dispatch_kitty_apc(b"a=a,i=42", &mut store, &mut assembler, 0, 0);

        // Response should contain OK
        let resp_str = String::from_utf8_lossy(&result.response);
        assert!(resp_str.contains("OK"));
        // Passthrough should be populated
        assert!(!result.passthrough_apc.is_empty());
    }
}
