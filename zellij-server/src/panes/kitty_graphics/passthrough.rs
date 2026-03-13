/// Kitty graphics protocol host terminal passthrough.
use super::command::KittyAction;

/// Reconstruct a complete Kitty graphics APC sequence from raw payload data.
pub fn build_passthrough_apc(apc_data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(3 + apc_data.len() + 2);
    out.extend_from_slice(b"\x1b_G");
    out.extend_from_slice(apc_data);
    out.extend_from_slice(b"\x1b\\");
    out
}

/// Determine whether a given action should be passed through to the host terminal.
pub fn should_passthrough(action: &KittyAction) -> bool {
    matches!(
        action,
        KittyAction::TransmitAndDisplay
            | KittyAction::Transmit
            | KittyAction::Place
            | KittyAction::Frame
            | KittyAction::Animate
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_passthrough_apc_wraps_correctly() {
        let data = b"a=T,i=1;AAAA";
        let result = build_passthrough_apc(data);
        assert!(result.starts_with(b"\x1b_G"));
        assert!(result.ends_with(b"\x1b\\"));
        assert_eq!(&result[3..result.len() - 2], data);
    }

    #[test]
    fn build_passthrough_apc_empty_data() {
        let result = build_passthrough_apc(b"");
        assert_eq!(result, b"\x1b_G\x1b\\");
    }

    #[test]
    fn should_passthrough_transmit_actions() {
        assert!(should_passthrough(&KittyAction::TransmitAndDisplay));
        assert!(should_passthrough(&KittyAction::Transmit));
        assert!(should_passthrough(&KittyAction::Place));
    }

    #[test]
    fn should_not_passthrough_query_delete() {
        assert!(!should_passthrough(&KittyAction::Query));
        assert!(!should_passthrough(&KittyAction::Delete));
    }
}
