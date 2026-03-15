/// Kitty graphics protocol transmit action handler.
///
/// Handles `a=T` (TransmitAndDisplay) and `a=t` (Transmit) actions.
/// Decodes the base64 payload, optionally decompresses, and stores
/// the image data in the KittyImageStore.
use super::command::{self, KittyAction, KittyCommand, TransmissionMedium};
use super::payload::{self, decode_kitty_payload};
use super::store::{self, KittyImageStore, KittyPlacement};

/// Build a Kitty graphics protocol APC response.
///
/// Returns `\x1b_Gi=<image_id>;<message>\x1b\` as bytes.
pub fn build_kitty_response(image_id: u32, message: &str) -> Vec<u8> {
    format!("\x1b_Gi={};{}\x1b\\", image_id, message).into_bytes()
}

/// Convert `command::Compression` to `payload::Compression`.
fn map_compression(c: &command::Compression) -> payload::Compression {
    match c {
        command::Compression::None => payload::Compression::None,
        command::Compression::Zlib => payload::Compression::Zlib,
    }
}

/// Convert `command::ImageFormat` to `store::ImageFormat`.
///
/// Defaults to RGBA (f=32) when no format is specified, matching
/// the Kitty protocol default.
fn map_format(f: &Option<command::ImageFormat>) -> store::ImageFormat {
    match f {
        Some(command::ImageFormat::Rgba) => store::ImageFormat::Rgba,
        Some(command::ImageFormat::Rgb) => store::ImageFormat::Rgb,
        Some(command::ImageFormat::Png) => store::ImageFormat::Png,
        None => store::ImageFormat::Rgba,
    }
}

/// Build an OK response, respecting the quiet flag.
///
/// - `q=0`: send OK response
/// - `q=1`: suppress OK (success) responses
/// - `q=2`: suppress ALL responses
fn ok_response(image_id: u32, quiet: u8) -> Vec<u8> {
    match quiet {
        0 => build_kitty_response(image_id, "OK"),
        _ => Vec::new(),
    }
}

/// Build an error response, respecting the quiet flag.
///
/// - `q=0`: send error response
/// - `q=1`: send error response
/// - `q=2`: suppress ALL responses (including errors)
fn error_response(image_id: u32, message: &str, quiet: u8) -> Vec<u8> {
    if quiet >= 2 {
        Vec::new()
    } else {
        build_kitty_response(image_id, message)
    }
}

/// Handle a Kitty graphics transmit action (`a=T` or `a=t`).
///
/// `raw_payload` is the base64-encoded bytes from the APC (after the `;`
/// separator). This function decodes the payload, optionally decompresses
/// it, stores the resulting image data, and returns the APC response bytes.
///
/// Returns an empty `Vec<u8>` when the quiet flag suppresses the response.
pub fn handle_transmit(
    cmd: &KittyCommand,
    raw_payload: &[u8],
    store: &mut KittyImageStore,
) -> Vec<u8> {
    let image_id_for_errors = cmd.image_id.unwrap_or(0);

    // Check transmission medium — only Direct is supported for now
    let medium = cmd
        .transmission
        .clone()
        .unwrap_or(TransmissionMedium::Direct);

    match medium {
        TransmissionMedium::Direct => {}, // handled below
        TransmissionMedium::File | TransmissionMedium::TempFile => {
            // TODO(Task 20): implement file-based transmission
            return error_response(
                image_id_for_errors,
                "ENOSYS:file transmission not yet implemented",
                cmd.quiet,
            );
        },
        TransmissionMedium::SharedMem => {
            // Shared memory transmission not planned
            return error_response(
                image_id_for_errors,
                "ENOSYS:shared memory transmission not supported",
                cmd.quiet,
            );
        },
    }

    // Decode the base64 payload, with optional zlib decompression
    let compression = map_compression(&cmd.compression);
    let decoded_data = match decode_kitty_payload(raw_payload, compression) {
        Ok(data) => data,
        Err(e) => {
            return error_response(image_id_for_errors, &format!("EBADF:{}", e), cmd.quiet);
        },
    };

    // Map image format (command enum → store enum)
    let format = map_format(&cmd.format);

    // Store the image data
    let assigned_id = store.store(cmd.image_id, format, decoded_data);

    // Update pixel dimensions if provided
    if cmd.pixel_width.is_some() || cmd.pixel_height.is_some() {
        if let Some(image) = store.get_mut(assigned_id) {
            if cmd.pixel_width.is_some() {
                image.pixel_width = cmd.pixel_width;
            }
            if cmd.pixel_height.is_some() {
                image.pixel_height = cmd.pixel_height;
            }
        }
    }

    // For a=T (TransmitAndDisplay, the default): create a default placement
    let action = cmd
        .action
        .clone()
        .unwrap_or(KittyAction::TransmitAndDisplay);
    if action == KittyAction::TransmitAndDisplay {
        let placement = KittyPlacement {
            placement_id: cmd.placement_id.unwrap_or(0),
            columns: cmd.columns,
            rows: cmd.rows,
            source_x: cmd.source_x,
            source_y: cmd.source_y,
            source_width: cmd.source_width,
            source_height: cmd.source_height,
            cell_offset_x: cmd.pixel_offset_x.unwrap_or(0),
            cell_offset_y: cmd.pixel_offset_y.unwrap_or(0),
            z_index: cmd.z_index.unwrap_or(0),
            virtual_placement: cmd.unicode_placeholder,
        };
        store.add_placement(assigned_id, placement);
    }

    // Return response based on quiet flag
    ok_response(assigned_id, cmd.quiet)
}

#[cfg(test)]
mod tests {
    use super::super::command::{
        Compression as CmdCompression, ImageFormat as CmdImageFormat, KittyAction, KittyCommand,
        TransmissionMedium,
    };
    use super::super::store::KittyImageStore;
    use super::*;

    /// Create a minimal KittyCommand for testing.
    fn make_cmd() -> KittyCommand {
        KittyCommand::default()
    }

    /// Base64-encode test data.
    fn b64(data: &[u8]) -> Vec<u8> {
        base64::encode(data).into_bytes()
    }

    // ---- build_kitty_response tests ----

    #[test]
    fn test_build_response_ok() {
        let resp = build_kitty_response(31, "OK");
        assert_eq!(resp, b"\x1b_Gi=31;OK\x1b\\");
    }

    #[test]
    fn test_build_response_error() {
        let resp = build_kitty_response(42, "EBADF:bad data");
        assert_eq!(resp, b"\x1b_Gi=42;EBADF:bad data\x1b\\");
    }

    #[test]
    fn test_build_response_id_zero() {
        let resp = build_kitty_response(0, "OK");
        assert_eq!(resp, b"\x1b_Gi=0;OK\x1b\\");
    }

    // ---- handle_transmit: basic storage tests ----

    #[test]
    fn test_transmit_direct_png() {
        let mut store = KittyImageStore::new();
        let mut cmd = make_cmd();
        cmd.action = Some(KittyAction::Transmit);
        cmd.format = Some(CmdImageFormat::Png);
        cmd.image_id = Some(10);

        let test_data = b"fake PNG data";
        let raw_payload = b64(test_data);

        let response = handle_transmit(&cmd, &raw_payload, &mut store);

        assert_eq!(response, build_kitty_response(10, "OK"));
        let image = store.get(10).unwrap();
        assert_eq!(image.data, test_data);
        assert_eq!(image.format, store::ImageFormat::Png);
    }

    #[test]
    fn test_transmit_direct_rgba() {
        let mut store = KittyImageStore::new();
        let mut cmd = make_cmd();
        cmd.action = Some(KittyAction::Transmit);
        cmd.format = Some(CmdImageFormat::Rgba);
        cmd.image_id = Some(20);

        let test_data = vec![255u8, 0, 0, 255, 0, 255, 0, 128];
        let raw_payload = b64(&test_data);

        let response = handle_transmit(&cmd, &raw_payload, &mut store);

        assert_eq!(response, build_kitty_response(20, "OK"));
        let image = store.get(20).unwrap();
        assert_eq!(image.data, test_data);
        assert_eq!(image.format, store::ImageFormat::Rgba);
    }

    #[test]
    fn test_transmit_with_zlib_compression() {
        use flate2::write::ZlibEncoder;
        use flate2::Compression as FlateCompression;
        use std::io::Write;

        let mut store = KittyImageStore::new();
        let mut cmd = make_cmd();
        cmd.action = Some(KittyAction::Transmit);
        cmd.format = Some(CmdImageFormat::Rgb);
        cmd.compression = CmdCompression::Zlib;
        cmd.image_id = Some(30);

        let original_data = b"RGB pixel data for testing";

        // Zlib compress, then base64 encode
        let mut encoder = ZlibEncoder::new(Vec::new(), FlateCompression::default());
        encoder.write_all(original_data).unwrap();
        let compressed = encoder.finish().unwrap();
        let raw_payload = b64(&compressed);

        let response = handle_transmit(&cmd, &raw_payload, &mut store);

        assert_eq!(response, build_kitty_response(30, "OK"));
        let image = store.get(30).unwrap();
        assert_eq!(image.data, original_data.to_vec());
        assert_eq!(image.format, store::ImageFormat::Rgb);
    }

    // ---- auto-assign ID tests ----

    #[test]
    fn test_auto_assign_id_when_none() {
        let mut store = KittyImageStore::new();
        let mut cmd = make_cmd();
        cmd.action = Some(KittyAction::Transmit);

        let raw_payload = b64(b"data1");
        let response = handle_transmit(&cmd, &raw_payload, &mut store);

        assert!(!response.is_empty());
        assert_eq!(store.image_count(), 1);
        let ids = store.all_image_ids();
        assert!(ids[0] > 0);
    }

    #[test]
    fn test_auto_assign_id_when_zero() {
        let mut store = KittyImageStore::new();
        let mut cmd = make_cmd();
        cmd.action = Some(KittyAction::Transmit);
        cmd.image_id = Some(0);

        let raw_payload = b64(b"data2");
        let response = handle_transmit(&cmd, &raw_payload, &mut store);

        assert!(!response.is_empty());
        assert_eq!(store.image_count(), 1);
        let ids = store.all_image_ids();
        assert!(ids[0] > 0);
    }

    // ---- quiet flag tests ----

    #[test]
    fn test_quiet_0_sends_ok() {
        let mut store = KittyImageStore::new();
        let mut cmd = make_cmd();
        cmd.action = Some(KittyAction::Transmit);
        cmd.image_id = Some(50);
        cmd.quiet = 0;

        let raw_payload = b64(b"test");
        let response = handle_transmit(&cmd, &raw_payload, &mut store);

        assert_eq!(response, build_kitty_response(50, "OK"));
    }

    #[test]
    fn test_quiet_1_suppresses_ok() {
        let mut store = KittyImageStore::new();
        let mut cmd = make_cmd();
        cmd.action = Some(KittyAction::Transmit);
        cmd.image_id = Some(51);
        cmd.quiet = 1;

        let raw_payload = b64(b"test");
        let response = handle_transmit(&cmd, &raw_payload, &mut store);

        // q=1 suppresses OK but would send errors
        assert!(response.is_empty());
        // Image should still be stored
        assert!(store.get(51).is_some());
    }

    #[test]
    fn test_quiet_1_sends_errors() {
        let mut store = KittyImageStore::new();
        let mut cmd = make_cmd();
        cmd.action = Some(KittyAction::Transmit);
        cmd.image_id = Some(52);
        cmd.quiet = 1;

        let raw_payload = b"!!!invalid_base64!!!";
        let response = handle_transmit(&cmd, raw_payload, &mut store);

        // q=1 still sends error responses
        assert!(!response.is_empty());
        let resp_str = String::from_utf8_lossy(&response);
        assert!(resp_str.contains("EBADF"));
    }

    #[test]
    fn test_quiet_2_suppresses_all() {
        let mut store = KittyImageStore::new();
        let mut cmd = make_cmd();
        cmd.action = Some(KittyAction::Transmit);
        cmd.image_id = Some(53);
        cmd.quiet = 2;

        // Invalid data: q=2 suppresses error too
        let response = handle_transmit(&cmd, b"!!!invalid!!!", &mut store);
        assert!(response.is_empty());

        // Valid data: q=2 suppresses OK
        let raw_payload = b64(b"good data");
        let response = handle_transmit(&cmd, &raw_payload, &mut store);
        assert!(response.is_empty());
    }

    // ---- action tests (a=T vs a=t) ----

    #[test]
    fn test_transmit_and_display_creates_placement() {
        let mut store = KittyImageStore::new();
        let mut cmd = make_cmd();
        cmd.action = Some(KittyAction::TransmitAndDisplay);
        cmd.image_id = Some(60);
        cmd.columns = Some(10);
        cmd.rows = Some(5);

        let raw_payload = b64(b"image data");
        let _response = handle_transmit(&cmd, &raw_payload, &mut store);

        let image = store.get(60).unwrap();
        assert_eq!(image.placements.len(), 1);
        let placement = image.placements.get(&0).unwrap();
        assert_eq!(placement.columns, Some(10));
        assert_eq!(placement.rows, Some(5));
    }

    #[test]
    fn test_transmit_only_no_placement() {
        let mut store = KittyImageStore::new();
        let mut cmd = make_cmd();
        cmd.action = Some(KittyAction::Transmit);
        cmd.image_id = Some(61);

        let raw_payload = b64(b"image data");
        let _response = handle_transmit(&cmd, &raw_payload, &mut store);

        let image = store.get(61).unwrap();
        assert!(image.placements.is_empty());
    }

    #[test]
    fn test_default_action_is_transmit_and_display() {
        let mut store = KittyImageStore::new();
        let mut cmd = make_cmd();
        // action is None → should behave as TransmitAndDisplay
        cmd.image_id = Some(62);

        let raw_payload = b64(b"image data");
        let _response = handle_transmit(&cmd, &raw_payload, &mut store);

        let image = store.get(62).unwrap();
        assert_eq!(image.placements.len(), 1);
    }

    // ---- error handling tests ----

    #[test]
    fn test_invalid_base64_error() {
        let mut store = KittyImageStore::new();
        let mut cmd = make_cmd();
        cmd.action = Some(KittyAction::Transmit);
        cmd.image_id = Some(70);
        cmd.quiet = 0;

        let response = handle_transmit(&cmd, b"!!not-valid-base64!!", &mut store);

        let resp_str = String::from_utf8_lossy(&response);
        assert!(resp_str.contains("EBADF"));
        assert!(resp_str.contains("i=70"));
        // Image should NOT be stored
        assert!(store.get(70).is_none());
    }

    // ---- pixel dimension tests ----

    #[test]
    fn test_pixel_dimensions_stored() {
        let mut store = KittyImageStore::new();
        let mut cmd = make_cmd();
        cmd.action = Some(KittyAction::Transmit);
        cmd.image_id = Some(80);
        cmd.pixel_width = Some(640);
        cmd.pixel_height = Some(480);

        let raw_payload = b64(b"data");
        let _response = handle_transmit(&cmd, &raw_payload, &mut store);

        let image = store.get(80).unwrap();
        assert_eq!(image.pixel_width, Some(640));
        assert_eq!(image.pixel_height, Some(480));
    }

    // ---- unsupported transmission medium tests ----

    #[test]
    fn test_file_transmission_returns_enosys() {
        let mut store = KittyImageStore::new();
        let mut cmd = make_cmd();
        cmd.action = Some(KittyAction::Transmit);
        cmd.transmission = Some(TransmissionMedium::File);
        cmd.image_id = Some(90);

        let response = handle_transmit(&cmd, b"", &mut store);
        let resp_str = String::from_utf8_lossy(&response);
        assert!(resp_str.contains("ENOSYS"));
    }

    #[test]
    fn test_tempfile_transmission_returns_enosys() {
        let mut store = KittyImageStore::new();
        let mut cmd = make_cmd();
        cmd.action = Some(KittyAction::Transmit);
        cmd.transmission = Some(TransmissionMedium::TempFile);
        cmd.image_id = Some(91);

        let response = handle_transmit(&cmd, b"", &mut store);
        let resp_str = String::from_utf8_lossy(&response);
        assert!(resp_str.contains("ENOSYS"));
    }

    #[test]
    fn test_shared_mem_returns_enosys() {
        let mut store = KittyImageStore::new();
        let mut cmd = make_cmd();
        cmd.action = Some(KittyAction::Transmit);
        cmd.transmission = Some(TransmissionMedium::SharedMem);
        cmd.image_id = Some(92);

        let response = handle_transmit(&cmd, b"", &mut store);
        let resp_str = String::from_utf8_lossy(&response);
        assert!(resp_str.contains("ENOSYS"));
    }

    // ---- default format test ----

    #[test]
    fn test_default_format_is_rgba() {
        let mut store = KittyImageStore::new();
        let mut cmd = make_cmd();
        cmd.action = Some(KittyAction::Transmit);
        cmd.image_id = Some(100);
        // format is None → defaults to RGBA

        let raw_payload = b64(b"rgba pixels");
        let _response = handle_transmit(&cmd, &raw_payload, &mut store);

        let image = store.get(100).unwrap();
        assert_eq!(image.format, store::ImageFormat::Rgba);
    }

    // ---- edge case tests ----

    #[test]
    fn test_empty_payload_stores_empty_image() {
        let mut store = KittyImageStore::new();
        let mut cmd = make_cmd();
        cmd.action = Some(KittyAction::Transmit);
        cmd.image_id = Some(110);

        let raw_payload = b64(b"");
        let response = handle_transmit(&cmd, &raw_payload, &mut store);

        assert_eq!(response, build_kitty_response(110, "OK"));
        let image = store.get(110).unwrap();
        assert!(image.data.is_empty());
    }
}
