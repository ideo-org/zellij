/// Kitty graphics protocol response parsing and routing.
///
/// Handles parsing APC responses from the host terminal (e.g., `\x1b_Gi=<id>;OK\x1b\\`)
/// and building response bytes to forward to application panes.

/// Parse an APC response from the host terminal.
///
/// The `apc_payload` should be the raw bytes between `ESC _G` and `ST`,
/// i.e., just the `i=<id>;<message>` portion (without the `G` prefix byte,
/// which was already stripped by the APC parser).
///
/// Returns `(image_id, message)` if the payload is a valid Kitty graphics response.
///
/// # Examples
/// - `b"i=42;OK"` → `Some((42, "OK".to_string()))`
/// - `b"i=99;ENOENT:image not found"` → `Some((99, "ENOENT:image not found".to_string()))`
/// - `b"not-kitty-data"` → `None`
pub fn parse_kitty_response(apc_payload: &[u8]) -> Option<(u32, String)> {
    let payload_str = std::str::from_utf8(apc_payload).ok()?;

    // Split on ';' to separate control data from message
    let (control, message) = payload_str.split_once(';')?;

    // Look for i=<number> in the control section
    let mut image_id: Option<u32> = None;
    for pair in control.split(',') {
        if let Some(val) = pair.strip_prefix("i=") {
            image_id = val.parse::<u32>().ok();
            break;
        }
    }

    let id = image_id?;
    Some((id, message.to_string()))
}

/// Build response bytes to forward to a pane's PTY.
///
/// Takes the raw APC payload (between `ESC _G` and `ST`) and wraps it
/// back into a complete APC sequence that the application can parse.
///
/// The returned bytes are: `ESC _ G <payload> ESC \`
pub fn build_pane_response(apc_payload: &[u8]) -> Vec<u8> {
    let mut response = Vec::with_capacity(apc_payload.len() + 4);
    response.push(0x1B); // ESC
    response.push(b'_'); // APC start
    response.push(b'G'); // Kitty graphics
    response.extend_from_slice(apc_payload);
    response.push(0x1B); // ESC
    response.push(b'\\'); // ST
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_kitty_response_valid_ok() {
        let payload = b"i=42;OK";
        let result = parse_kitty_response(payload);
        assert_eq!(result, Some((42, "OK".to_string())));
    }

    #[test]
    fn parse_kitty_response_valid_enoent() {
        let payload = b"i=99;ENOENT:image not found";
        let result = parse_kitty_response(payload);
        assert_eq!(result, Some((99, "ENOENT:image not found".to_string())));
    }

    #[test]
    fn parse_kitty_response_non_kitty_data() {
        let payload = b"this is not kitty data";
        let result = parse_kitty_response(payload);
        assert_eq!(result, None);
    }

    #[test]
    fn parse_kitty_response_missing_image_id() {
        // Has semicolon but no i= key
        let payload = b"a=T;some data";
        let result = parse_kitty_response(payload);
        assert_eq!(result, None);
    }

    #[test]
    fn parse_kitty_response_invalid_image_id() {
        let payload = b"i=notanumber;OK";
        let result = parse_kitty_response(payload);
        assert_eq!(result, None);
    }

    #[test]
    fn parse_kitty_response_multiple_control_keys() {
        // Response with extra keys besides i=
        let payload = b"i=7,q=1;OK";
        let result = parse_kitty_response(payload);
        assert_eq!(result, Some((7, "OK".to_string())));
    }

    #[test]
    fn build_pane_response_wraps_correctly() {
        let payload = b"i=42;OK";
        let result = build_pane_response(payload);
        // Should be: ESC _ G i=42;OK ESC \
        assert_eq!(result[0], 0x1B);
        assert_eq!(result[1], b'_');
        assert_eq!(result[2], b'G');
        assert_eq!(&result[3..10], b"i=42;OK");
        assert_eq!(result[10], 0x1B);
        assert_eq!(result[11], b'\\');
    }

    #[test]
    fn build_pane_response_preserves_payload() {
        let payload = b"i=99;ENOENT:image not found";
        let result = build_pane_response(payload);
        // Verify the payload is preserved between ESC_G and ESC\
        assert_eq!(&result[3..result.len() - 2], payload);
    }
}
