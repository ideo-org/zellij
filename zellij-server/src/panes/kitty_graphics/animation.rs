/// Kitty graphics protocol animation frame and control handlers.
///
/// Handles `a=f` (Frame) — add an animation frame to an existing image.
/// Handles `a=a` (Animate) — control animation playback.
///
/// Zellij stores frame data; actual rendering/playback is performed
/// by the host terminal via APC passthrough.
use super::command::{Compression as CmdCompression, KittyCommand};
use super::payload::{decode_kitty_payload, Compression as PayloadCompression};
use super::store::{KittyFrame, KittyImageStore};
use super::transmit::build_kitty_response;

/// Convert `command::Compression` to `payload::Compression`.
fn map_compression(c: &CmdCompression) -> PayloadCompression {
    match c {
        CmdCompression::None => PayloadCompression::None,
        CmdCompression::Zlib => PayloadCompression::Zlib,
    }
}

/// Build an OK response, respecting the quiet flag.
///
/// - `q=0`: send OK response
/// - `q>=1`: suppress OK (success) responses
fn ok_response(image_id: u32, quiet: u8) -> Vec<u8> {
    match quiet {
        0 => build_kitty_response(image_id, "OK"),
        _ => Vec::new(),
    }
}

/// Build an error response, respecting the quiet flag.
///
/// - `q=0` or `q=1`: send error response
/// - `q=2`: suppress ALL responses (including errors)
fn error_response(image_id: u32, message: &str, quiet: u8) -> Vec<u8> {
    if quiet >= 2 {
        Vec::new()
    } else {
        build_kitty_response(image_id, message)
    }
}

/// Handle `a=f` (Frame): Add an animation frame to an existing image.
///
/// In the Kitty protocol, animation frames overload some control keys:
/// - `r=N` → frame number (normally `rows`)
/// - `z=N` → gap in milliseconds between this frame and the next (normally `z_index`)
///
/// The payload is base64-encoded pixel data for this frame.
pub fn handle_frame(cmd: &KittyCommand, store: &mut KittyImageStore) -> Vec<u8> {
    let image_id = match cmd.image_id {
        Some(id) if id > 0 => id,
        _ => return error_response(0, "EINVAL:image_id required for frame", cmd.quiet),
    };

    // Decode the payload (base64, optional zlib)
    let compression = map_compression(&cmd.compression);
    let decoded_data = match decode_kitty_payload(&cmd.payload, compression) {
        Ok(data) => data,
        Err(e) => return error_response(image_id, &format!("EBADF:{}", e), cmd.quiet),
    };

    // Look up the image
    let image = match store.get_mut(image_id) {
        Some(img) => img,
        None => return error_response(image_id, "ENOENT:image not found", cmd.quiet),
    };

    // Frame number: use r=N if provided, otherwise auto-assign
    let frame_number = cmd
        .rows
        .unwrap_or_else(|| image.frames.last().map_or(1, |f| f.frame_number + 1));

    // Gap: use z=N if provided (clamped to >= 0 since z_index is i32), otherwise 0
    let gap_ms = cmd.z_index.unwrap_or(0).max(0) as u32;

    let frame = KittyFrame {
        frame_number,
        data: decoded_data,
        gap_ms,
    };

    image.frames.push(frame);

    ok_response(image_id, cmd.quiet)
}

/// Handle `a=a` (Animate): Control animation playback.
///
/// Animation control fields (overloaded from the Kitty protocol):
/// - `s=N` → set current frame (mapped to `cmd.pixel_width`)
/// - `v=N` → loop count (mapped to `cmd.pixel_height`)
/// - `z=N` → gap override in ms (mapped to `cmd.z_index`)
///
/// Zellij acknowledges the command; actual playback is handled by the
/// host terminal via APC passthrough.
pub fn handle_animate(cmd: &KittyCommand, store: &mut KittyImageStore) -> Vec<u8> {
    let image_id = match cmd.image_id {
        Some(id) if id > 0 => id,
        _ => return error_response(0, "EINVAL:image_id required for animate", cmd.quiet),
    };

    // Verify the image exists
    if store.get(image_id).is_none() {
        return error_response(image_id, "ENOENT:image not found", cmd.quiet);
    }

    // NOTE: Animation control state (current frame, loop count, gap override)
    // is not tracked in the store. The host terminal handles playback via
    // APC passthrough. We merely validate and acknowledge.

    ok_response(image_id, cmd.quiet)
}

#[cfg(test)]
mod tests {
    use super::super::command::{KittyAction, KittyCommand};
    use super::super::store::{ImageFormat, KittyImageStore};
    use super::super::transmit::build_kitty_response;
    use super::*;

    /// Helper: base64-encode test data.
    fn b64(data: &[u8]) -> Vec<u8> {
        base64::encode(data).into_bytes()
    }

    /// Helper: create a store with one image pre-loaded.
    fn store_with_image(id: u32) -> KittyImageStore {
        let mut store = KittyImageStore::new();
        store.store(Some(id), ImageFormat::Png, vec![0xDE, 0xAD]);
        store
    }

    /// Helper: create a frame command for the given image ID.
    fn frame_cmd(image_id: u32, payload: &[u8]) -> KittyCommand {
        let mut cmd = KittyCommand::default();
        cmd.action = Some(KittyAction::Frame);
        cmd.image_id = Some(image_id);
        cmd.payload = b64(payload);
        cmd
    }

    // ---- handle_frame tests ----

    #[test]
    fn frame_added_to_existing_image() {
        let mut store = store_with_image(42);
        let cmd = frame_cmd(42, b"frame pixel data");

        let response = handle_frame(&cmd, &mut store);

        assert_eq!(response, build_kitty_response(42, "OK"));
        let image = store.get(42).unwrap();
        assert_eq!(image.frames.len(), 1);
        assert_eq!(image.frames[0].data, b"frame pixel data");
    }

    #[test]
    fn frame_on_nonexistent_image_returns_enoent() {
        let mut store = KittyImageStore::new();
        let cmd = frame_cmd(999, b"data");

        let response = handle_frame(&cmd, &mut store);

        let resp_str = String::from_utf8_lossy(&response);
        assert!(resp_str.contains("ENOENT"));
        assert!(resp_str.contains("i=999"));
    }

    #[test]
    fn multiple_frames_stored_in_order() {
        let mut store = store_with_image(10);

        for i in 0..3u8 {
            let cmd = frame_cmd(10, &[i; 4]);
            handle_frame(&cmd, &mut store);
        }

        let image = store.get(10).unwrap();
        assert_eq!(image.frames.len(), 3);
        // Auto-assigned frame numbers: 1, 2, 3
        assert_eq!(image.frames[0].frame_number, 1);
        assert_eq!(image.frames[1].frame_number, 2);
        assert_eq!(image.frames[2].frame_number, 3);
        // Data matches
        assert_eq!(image.frames[0].data, vec![0u8; 4]);
        assert_eq!(image.frames[1].data, vec![1u8; 4]);
        assert_eq!(image.frames[2].data, vec![2u8; 4]);
    }

    #[test]
    fn frame_with_explicit_frame_number_and_gap() {
        let mut store = store_with_image(20);
        let mut cmd = frame_cmd(20, b"px");
        cmd.rows = Some(5); // r=5 → frame_number
        cmd.z_index = Some(100); // z=100 → gap_ms

        handle_frame(&cmd, &mut store);

        let image = store.get(20).unwrap();
        assert_eq!(image.frames[0].frame_number, 5);
        assert_eq!(image.frames[0].gap_ms, 100);
    }

    #[test]
    fn frame_payload_base64_decoded_correctly() {
        let mut store = store_with_image(30);
        let original = b"\x00\x01\x02\xFF\xFE\xFD";
        let cmd = frame_cmd(30, original);

        handle_frame(&cmd, &mut store);

        let image = store.get(30).unwrap();
        assert_eq!(image.frames[0].data, original.to_vec());
    }

    #[test]
    fn frame_invalid_base64_returns_ebadf() {
        let mut store = store_with_image(40);
        let mut cmd = KittyCommand::default();
        cmd.action = Some(KittyAction::Frame);
        cmd.image_id = Some(40);
        cmd.payload = b"!!!invalid-base64!!!".to_vec(); // raw, not b64-encoded

        let response = handle_frame(&cmd, &mut store);

        let resp_str = String::from_utf8_lossy(&response);
        assert!(resp_str.contains("EBADF"));
    }

    #[test]
    fn frame_quiet_1_suppresses_ok() {
        let mut store = store_with_image(50);
        let mut cmd = frame_cmd(50, b"data");
        cmd.quiet = 1;

        let response = handle_frame(&cmd, &mut store);

        assert!(response.is_empty());
        // Frame should still be stored
        assert_eq!(store.get(50).unwrap().frames.len(), 1);
    }

    #[test]
    fn frame_quiet_2_suppresses_all() {
        let mut store = store_with_image(51);

        // Suppress OK
        let mut cmd = frame_cmd(51, b"data");
        cmd.quiet = 2;
        let response = handle_frame(&cmd, &mut store);
        assert!(response.is_empty());

        // Suppress error too
        let mut store2 = KittyImageStore::new();
        let mut cmd2 = frame_cmd(999, b"data");
        cmd2.quiet = 2;
        let response2 = handle_frame(&cmd2, &mut store2);
        assert!(response2.is_empty());
    }

    #[test]
    fn frame_missing_image_id_returns_einval() {
        let mut store = store_with_image(1);
        let mut cmd = KittyCommand::default();
        cmd.action = Some(KittyAction::Frame);
        cmd.payload = b64(b"data");
        // image_id is None

        let response = handle_frame(&cmd, &mut store);

        let resp_str = String::from_utf8_lossy(&response);
        assert!(resp_str.contains("EINVAL"));
    }

    #[test]
    fn frame_negative_gap_clamped_to_zero() {
        let mut store = store_with_image(60);
        let mut cmd = frame_cmd(60, b"px");
        cmd.z_index = Some(-50); // Negative gap → clamped to 0

        handle_frame(&cmd, &mut store);

        let image = store.get(60).unwrap();
        assert_eq!(image.frames[0].gap_ms, 0);
    }

    // ---- handle_animate tests ----

    #[test]
    fn animate_existing_image_returns_ok() {
        let mut store = store_with_image(70);
        let mut cmd = KittyCommand::default();
        cmd.action = Some(KittyAction::Animate);
        cmd.image_id = Some(70);

        let response = handle_animate(&cmd, &mut store);

        assert_eq!(response, build_kitty_response(70, "OK"));
    }

    #[test]
    fn animate_nonexistent_image_returns_enoent() {
        let mut store = KittyImageStore::new();
        let mut cmd = KittyCommand::default();
        cmd.action = Some(KittyAction::Animate);
        cmd.image_id = Some(999);

        let response = handle_animate(&cmd, &mut store);

        let resp_str = String::from_utf8_lossy(&response);
        assert!(resp_str.contains("ENOENT"));
    }

    #[test]
    fn animate_missing_image_id_returns_einval() {
        let mut store = KittyImageStore::new();
        let mut cmd = KittyCommand::default();
        cmd.action = Some(KittyAction::Animate);

        let response = handle_animate(&cmd, &mut store);

        let resp_str = String::from_utf8_lossy(&response);
        assert!(resp_str.contains("EINVAL"));
    }

    #[test]
    fn animate_quiet_1_suppresses_ok() {
        let mut store = store_with_image(80);
        let mut cmd = KittyCommand::default();
        cmd.action = Some(KittyAction::Animate);
        cmd.image_id = Some(80);
        cmd.quiet = 1;

        let response = handle_animate(&cmd, &mut store);

        assert!(response.is_empty());
    }
}
