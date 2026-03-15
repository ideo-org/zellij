/// Kitty graphics protocol Query action (a=q) handler.
///
/// Handles capability queries and image existence queries, returning the correct APC response bytes.
use super::command::KittyCommand;
use super::store::KittyImageStore;

/// Build a Kitty APC response in the format: ESC + `_G` + `i=<id>` + `;` + `<message>` + ESC + `\`
///
/// # Arguments
/// * `image_id` - The image ID to include in the response
/// * `message` - The message content (e.g., "OK", "ENOENT:image not found")
///
/// # Returns
/// A Vec<u8> containing the complete APC response bytes
///
/// # Example
/// ```ignore
/// let response = build_kitty_response(31, "OK");
/// // Returns: b"\x1b_Gi=31;OK\x1b\\"
/// ```
pub fn build_kitty_response(image_id: u32, message: &str) -> Vec<u8> {
    let mut response = Vec::new();

    // ESC + _G
    response.push(0x1B); // ESC
    response.push(b'_');
    response.push(b'G');

    // i=<image_id>
    response.push(b'i');
    response.push(b'=');
    let id_str = image_id.to_string();
    response.extend_from_slice(id_str.as_bytes());

    // ;
    response.push(b';');

    // message
    response.extend_from_slice(message.as_bytes());

    // ESC + \
    response.push(0x1B); // ESC
    response.push(0x5C); // backslash

    response
}

/// Handle a Kitty graphics protocol Query action (a=q).
///
/// Dispatches based on whether this is a capability query or an image existence query.
/// Respects the `quiet` flag:
/// - q=0 (default): send response
/// - q=1: send only errors (not OK)
/// - q=2: suppress ALL responses
///
/// # Arguments
/// * `cmd` - The parsed KittyCommand with action=Query
/// * `store` - The KittyImageStore to check for image existence
///
/// # Returns
/// A Vec<u8> containing the APC response bytes, or empty Vec if suppressed by quiet flag
pub fn handle_query(cmd: &KittyCommand, store: &KittyImageStore) -> Vec<u8> {
    // q=2: suppress all responses
    if cmd.quiet == 2 {
        return Vec::new();
    }

    // Determine the image ID to use in the response
    let response_id = cmd.image_id.unwrap_or(0);

    // If image_id is specified, check if it exists
    if let Some(id) = cmd.image_id {
        if store.get(id).is_some() {
            // Image found
            // q=1: suppress OK responses, only send errors
            if cmd.quiet == 1 {
                return Vec::new();
            }
            return build_kitty_response(response_id, "OK");
        } else {
            // Image not found - this is an error, always send
            return build_kitty_response(response_id, "ENOENT:image not found");
        }
    }

    // No image_id specified: this is a capability query
    // Respond with OK to indicate Zellij supports Kitty graphics
    // q=1: suppress OK responses
    if cmd.quiet == 1 {
        return Vec::new();
    }

    build_kitty_response(response_id, "OK")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panes::kitty_graphics::store::ImageFormat;

    #[test]
    fn test_build_kitty_response_format() {
        let response = build_kitty_response(31, "OK");

        // Check starts with ESC + _G
        assert_eq!(response[0], 0x1B);
        assert_eq!(response[1], b'_');
        assert_eq!(response[2], b'G');

        // Check ends with ESC + \
        assert_eq!(response[response.len() - 2], 0x1B);
        assert_eq!(response[response.len() - 1], 0x5C);

        // Check contains the message
        let response_str = String::from_utf8_lossy(&response);
        assert!(response_str.contains("i=31"));
        assert!(response_str.contains("OK"));
    }

    #[test]
    fn test_query_existing_image() {
        let mut store = KittyImageStore::new();
        store.store(Some(42), ImageFormat::Png, vec![1, 2, 3]);

        let mut cmd = KittyCommand::default();
        cmd.image_id = Some(42);
        cmd.quiet = 0;

        let response = handle_query(&cmd, &store);

        assert!(!response.is_empty());
        let response_str = String::from_utf8_lossy(&response);
        assert!(response_str.contains("OK"));
        assert!(response_str.contains("i=42"));
    }

    #[test]
    fn test_query_nonexistent_image() {
        let store = KittyImageStore::new();

        let mut cmd = KittyCommand::default();
        cmd.image_id = Some(999);
        cmd.quiet = 0;

        let response = handle_query(&cmd, &store);

        assert!(!response.is_empty());
        let response_str = String::from_utf8_lossy(&response);
        assert!(response_str.contains("ENOENT"));
        assert!(response_str.contains("i=999"));
    }

    #[test]
    fn test_capability_query_no_image_id() {
        let store = KittyImageStore::new();

        let mut cmd = KittyCommand::default();
        cmd.image_id = None;
        cmd.quiet = 0;

        let response = handle_query(&cmd, &store);

        assert!(!response.is_empty());
        let response_str = String::from_utf8_lossy(&response);
        assert!(response_str.contains("OK"));
        assert!(response_str.contains("i=0"));
    }

    #[test]
    fn test_quiet_2_suppresses_all() {
        let mut store = KittyImageStore::new();
        store.store(Some(1), ImageFormat::Png, vec![1, 2, 3]);

        let mut cmd = KittyCommand::default();
        cmd.image_id = Some(1);
        cmd.quiet = 2;

        let response = handle_query(&cmd, &store);

        assert!(response.is_empty());
    }

    #[test]
    fn test_quiet_1_suppresses_ok_for_existing() {
        let mut store = KittyImageStore::new();
        store.store(Some(5), ImageFormat::Png, vec![1, 2, 3]);

        let mut cmd = KittyCommand::default();
        cmd.image_id = Some(5);
        cmd.quiet = 1;

        let response = handle_query(&cmd, &store);

        // Should suppress OK for existing image
        assert!(response.is_empty());
    }

    #[test]
    fn test_quiet_1_sends_error_for_missing() {
        let store = KittyImageStore::new();

        let mut cmd = KittyCommand::default();
        cmd.image_id = Some(999);
        cmd.quiet = 1;

        let response = handle_query(&cmd, &store);

        // Should send error even with q=1
        assert!(!response.is_empty());
        let response_str = String::from_utf8_lossy(&response);
        assert!(response_str.contains("ENOENT"));
    }

    #[test]
    fn test_response_format_with_different_ids() {
        for id in &[0, 1, 42, 999, 4294967295u32] {
            let response = build_kitty_response(*id, "OK");
            let response_str = String::from_utf8_lossy(&response);
            assert!(response_str.contains(&format!("i={}", id)));
        }
    }

    #[test]
    fn test_response_format_with_different_messages() {
        for msg in &["OK", "ENOENT:image not found", "ERROR:test"] {
            let response = build_kitty_response(1, msg);
            let response_str = String::from_utf8_lossy(&response);
            assert!(response_str.contains(msg));
        }
    }
}
