/// Kitty graphics protocol APC dispatch logic.
///
/// Routes a complete APC payload (captured by ApcParser) to the correct
/// handler based on the parsed command's action field.
use super::chunked::ChunkAssembler;
use super::command::{KittyAction, KittyCommand};
use super::delete::handle_delete;
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
    if cmd.more_chunks {
        chunk_assembler.add_chunk(cmd.image_id, &cmd.payload, true);
        return DispatchResult {
            response: vec![],
            passthrough_apc: vec![],
        };
    }
    // NOTE: Full chunked assembly integration (reassembling prior chunks
    // and passing the concatenated payload to handlers) is deferred to a
    // later task.  For now, only single-shot payloads are handled.

    // 3. Dispatch by action
    let action = cmd
        .action
        .clone()
        .unwrap_or(KittyAction::TransmitAndDisplay);
    match action {
        KittyAction::TransmitAndDisplay | KittyAction::Transmit => {
            let response = handle_transmit(&cmd, &cmd.payload, store);
            DispatchResult {
                response,
                passthrough_apc: vec![],
            }
        },
        KittyAction::Place => {
            // Placement handling — actual grid writing deferred to Wave 3
            DispatchResult {
                response: vec![],
                passthrough_apc: vec![],
            }
        },
        KittyAction::Query => {
            let response = handle_query(&cmd, store);
            DispatchResult {
                response,
                passthrough_apc: vec![],
            }
        },
        KittyAction::Delete => {
            let _result = handle_delete(&cmd, store, cursor_row, cursor_col);
            DispatchResult {
                response: vec![],
                passthrough_apc: vec![],
            }
        },
        KittyAction::Frame | KittyAction::Animate => {
            // Animation support deferred to Task 16
            DispatchResult {
                response: vec![],
                passthrough_apc: vec![],
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
}
