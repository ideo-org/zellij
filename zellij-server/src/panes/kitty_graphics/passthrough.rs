/// Kitty graphics protocol host terminal passthrough.
use super::command::KittyAction;

/// Reconstruct a complete Kitty graphics APC sequence from raw payload data,
/// injecting `q=2` to suppress the host terminal's response.
///
/// When Zellij forwards an APC to the host terminal, the host would normally
/// send back a response APC (e.g., `\x1b_Gi=31;OK\x1b\\`). Without `q=2`,
/// that response leaks into Zellij's stdin as garbage text input.
/// With `q=2`, the host processes the APC silently.
pub fn build_passthrough_apc(apc_data: &[u8]) -> Vec<u8> {
    // Find the semicolon separating control data from payload
    let semicolon_pos = apc_data.iter().position(|&b| b == b';');
    let mut out = Vec::with_capacity(3 + apc_data.len() + 4 + 2);
    out.extend_from_slice(b"\x1b_G");
    match semicolon_pos {
        Some(pos) => {
            // Insert q=2 into control data before the semicolon
            out.extend_from_slice(&apc_data[..pos]);
            out.extend_from_slice(b",q=2");
            out.extend_from_slice(&apc_data[pos..]);
        },
        None => {
            // No payload — just append q=2 to control data
            out.extend_from_slice(apc_data);
            out.extend_from_slice(b",q=2");
        },
    }
    out.extend_from_slice(b"\x1b\\");
    out
}

/// Determine whether a given action should be passed through to the host terminal.
///
/// Only Transmit actions are forwarded - the host terminal caches the image data.
/// Placement, deletion, and animations are handled locally by Zellij via Unicode placeholders.
pub fn should_passthrough(action: &KittyAction) -> bool {
    matches!(
        action,
        KittyAction::TransmitAndDisplay | KittyAction::Transmit
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_passthrough_apc_injects_quiet_flag() {
        let data = b"a=T,i=1;AAAA";
        let result = build_passthrough_apc(data);
        assert!(result.starts_with(b"\x1b_G"));
        assert!(result.ends_with(b"\x1b\\"));
        let inner = String::from_utf8_lossy(&result[3..result.len() - 2]);
        assert!(
            inner.contains("q=2"),
            "passthrough must include q=2, got: {}",
            inner
        );
        assert!(
            inner.contains("a=T,i=1"),
            "control data preserved, got: {}",
            inner
        );
        assert!(inner.contains("AAAA"), "payload preserved, got: {}", inner);
    }

    #[test]
    fn build_passthrough_apc_no_payload_still_has_quiet() {
        let result = build_passthrough_apc(b"a=d,d=A");
        let inner = String::from_utf8_lossy(&result[3..result.len() - 2]);
        assert!(inner.contains("q=2"), "must have q=2: {}", inner);
        assert!(
            inner.contains("a=d,d=A"),
            "control data preserved: {}",
            inner
        );
    }

    #[test]
    fn should_passthrough_transmit_only() {
        // Only Transmit actions should be forwarded to host terminal
        assert!(should_passthrough(&KittyAction::TransmitAndDisplay));
        assert!(should_passthrough(&KittyAction::Transmit));
        
        // All other actions are handled locally by Zellij
        assert!(!should_passthrough(&KittyAction::Place));
        assert!(!should_passthrough(&KittyAction::Query));
        assert!(!should_passthrough(&KittyAction::Delete));
        assert!(!should_passthrough(&KittyAction::Frame));
        assert!(!should_passthrough(&KittyAction::Animate));
    }
}
