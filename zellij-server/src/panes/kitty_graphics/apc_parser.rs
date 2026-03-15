/// APC (Application Program Command) byte sequence detector for Kitty graphics protocol.
///
/// Kitty graphics protocol uses APC sequences starting with `\x1b_G` and ending with `\x1b\`.
/// This parser intercepts bytes before they reach the VTE parser, detecting Kitty graphics
/// APC sequences while passing through all other bytes unchanged.

/// Result of advancing the APC parser by one byte.
#[derive(Debug, PartialEq)]
pub enum ApcParserResult {
    /// Byte is NOT part of an APC sequence — forward to VTE.
    PassThrough(u8),
    /// Byte IS part of an in-progress APC sequence — do NOT forward to VTE.
    Collecting,
    /// Full APC captured. Contains payload bytes (after 'G', before `\x1b\`).
    Complete(Vec<u8>),
    /// APC was not a valid Kitty sequence or exceeded max size — re-emit these bytes to VTE.
    Aborted(Vec<u8>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    Normal,
    EscSeen,
    ApcOpen,
    ApcPayload,
    ApcEscSeen,
}

const DEFAULT_MAX_SIZE: usize = 16 * 1024 * 1024; // 16 MiB

/// Byte-at-a-time state machine that detects Kitty graphics APC sequences.
///
/// Bytes are fed one at a time via [`advance`](ApcParser::advance). The return value
/// tells the caller whether to forward the byte to VTE, buffer it, or handle a
/// completed/aborted APC payload.
pub struct ApcParser {
    state: State,
    buffer: Vec<u8>,
    max_size: usize,
}

impl ApcParser {
    pub fn new() -> Self {
        Self {
            state: State::Normal,
            buffer: Vec::new(),
            max_size: DEFAULT_MAX_SIZE,
        }
    }

    /// Advance the parser state machine by one byte.
    pub fn advance(&mut self, byte: u8) -> ApcParserResult {
        match self.state {
            State::Normal => {
                if byte == 0x1B {
                    self.state = State::EscSeen;
                    ApcParserResult::Collecting
                } else {
                    ApcParserResult::PassThrough(byte)
                }
            },
            State::EscSeen => {
                if byte == 0x5F {
                    // ESC _ → APC opener
                    self.state = State::ApcOpen;
                    ApcParserResult::Collecting
                } else {
                    // Not APC, re-emit absorbed ESC + current byte
                    self.state = State::Normal;
                    ApcParserResult::Aborted(vec![0x1B, byte])
                }
            },
            State::ApcOpen => {
                if byte == b'G' {
                    // Kitty graphics APC confirmed
                    self.state = State::ApcPayload;
                    self.buffer.clear();
                    ApcParserResult::Collecting
                } else {
                    // Not a Kitty APC, re-emit all absorbed bytes
                    self.state = State::Normal;
                    ApcParserResult::Aborted(vec![0x1B, 0x5F, byte])
                }
            },
            State::ApcPayload => {
                if byte == 0x1B {
                    // Possible start of APC terminator
                    self.state = State::ApcEscSeen;
                    ApcParserResult::Collecting
                } else {
                    self.buffer.push(byte);
                    if self.buffer.len() > self.max_size {
                        let aborted = std::mem::take(&mut self.buffer);
                        self.state = State::Normal;
                        ApcParserResult::Aborted(aborted)
                    } else {
                        ApcParserResult::Collecting
                    }
                }
            },
            State::ApcEscSeen => {
                if byte == 0x5C {
                    // ESC \ → APC terminator, sequence complete
                    let payload = std::mem::take(&mut self.buffer);
                    self.state = State::Normal;
                    ApcParserResult::Complete(payload)
                } else if byte == 0x1B {
                    // Consecutive ESC: first ESC is data, second ESC might be start of new terminator
                    self.buffer.push(0x1B);
                    // Stay in ApcEscSeen state for the new ESC
                    if self.buffer.len() > self.max_size {
                        let aborted = std::mem::take(&mut self.buffer);
                        self.state = State::Normal;
                        ApcParserResult::Aborted(aborted)
                    } else {
                        ApcParserResult::Collecting
                    }
                } else {
                    // ESC was just data inside the payload, not a terminator
                    self.buffer.push(0x1B);
                    self.buffer.push(byte);
                    self.state = State::ApcPayload;
                    if self.buffer.len() > self.max_size {
                        let aborted = std::mem::take(&mut self.buffer);
                        self.state = State::Normal;
                        ApcParserResult::Aborted(aborted)
                    } else {
                        ApcParserResult::Collecting
                    }
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: feed all bytes and collect results.
    fn feed_all(parser: &mut ApcParser, input: &[u8]) -> Vec<ApcParserResult> {
        input.iter().map(|&b| parser.advance(b)).collect()
    }

    #[test]
    fn test_simple_apc() {
        let mut parser = ApcParser::new();
        let input = b"\x1b_Ghello\x1b\\";
        let results = feed_all(&mut parser, input);

        let last = results.last().unwrap();
        assert_eq!(*last, ApcParserResult::Complete(b"hello".to_vec()));
    }

    #[test]
    fn test_non_kitty_apc_passthrough() {
        let mut parser = ApcParser::new();
        let input = b"\x1b_Xhello\x1b\\";
        let results = feed_all(&mut parser, input);

        // No Complete should be produced for non-Kitty APC
        assert!(
            !results
                .iter()
                .any(|r| matches!(r, ApcParserResult::Complete(_))),
            "Should not produce Complete for non-Kitty APC"
        );

        // All original bytes must be re-emitted (via PassThrough or Aborted)
        let mut emitted = Vec::new();
        for r in &results {
            match r {
                ApcParserResult::PassThrough(b) => emitted.push(*b),
                ApcParserResult::Aborted(bytes) => emitted.extend(bytes),
                _ => {},
            }
        }
        assert_eq!(emitted, input.to_vec());
    }

    #[test]
    fn test_mixed_text_and_apc() {
        let mut parser = ApcParser::new();
        let input = b"hello\x1b_Gworld\x1b\\foo";
        let results = feed_all(&mut parser, input);

        let mut passthrough_before = Vec::new();
        let mut complete_payload = None;
        let mut passthrough_after = Vec::new();

        for r in &results {
            match r {
                ApcParserResult::PassThrough(b) => {
                    if complete_payload.is_some() {
                        passthrough_after.push(*b);
                    } else {
                        passthrough_before.push(*b);
                    }
                },
                ApcParserResult::Complete(payload) => {
                    complete_payload = Some(payload.clone());
                },
                _ => {},
            }
        }

        assert_eq!(passthrough_before, b"hello".to_vec());
        assert_eq!(complete_payload.unwrap(), b"world".to_vec());
        assert_eq!(passthrough_after, b"foo".to_vec());
    }

    #[test]
    fn test_split_across_calls() {
        let mut parser = ApcParser::new();

        // Feed first half — should not complete yet
        let first_half = b"\x1b_Ghel";
        for &byte in first_half.iter() {
            assert!(
                !matches!(parser.advance(byte), ApcParserResult::Complete(_)),
                "Should not complete during first half"
            );
        }

        // Feed second half — last byte should produce Complete
        let second_half = b"lo\x1b\\";
        let mut completed = None;
        for &byte in second_half.iter() {
            if let ApcParserResult::Complete(payload) = parser.advance(byte) {
                completed = Some(payload);
            }
        }

        assert_eq!(completed.unwrap(), b"hello".to_vec());
    }

    #[test]
    fn test_recovery_after_complete() {
        let mut parser = ApcParser::new();

        // First APC
        let first = b"\x1b_Gfirst\x1b\\";
        let mut first_payload = None;
        for &byte in first.iter() {
            if let ApcParserResult::Complete(payload) = parser.advance(byte) {
                first_payload = Some(payload);
            }
        }
        assert_eq!(first_payload.unwrap(), b"first".to_vec());

        // Second APC — parser must recover and handle correctly
        let second = b"\x1b_Gsecond\x1b\\";
        let mut second_payload = None;
        for &byte in second.iter() {
            if let ApcParserResult::Complete(payload) = parser.advance(byte) {
                second_payload = Some(payload);
            }
        }
        assert_eq!(second_payload.unwrap(), b"second".to_vec());
    }
}
