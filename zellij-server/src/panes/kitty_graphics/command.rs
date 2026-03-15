/// Kitty graphics protocol control data parser.
///
/// Parses the APC data bytes (after 'G', before ST) into structured commands.
/// Format: `key=value,key=value,...;payload`

#[derive(Debug, Clone, PartialEq)]
pub enum KittyAction {
    TransmitAndDisplay, // a=T (default)
    Transmit,           // a=t
    Place,              // a=p
    Query,              // a=q
    Delete,             // a=d
    Frame,              // a=f (animation frame)
    Animate,            // a=a (animation control)
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImageFormat {
    Rgba, // f=32
    Rgb,  // f=24
    Png,  // f=100
}

#[derive(Debug, Clone, PartialEq)]
pub enum TransmissionMedium {
    Direct,    // t=d (default)
    File,      // t=f
    TempFile,  // t=t
    SharedMem, // t=s
}

#[derive(Debug, Clone, PartialEq)]
pub enum Compression {
    None,
    Zlib, // o=z
}

impl Default for Compression {
    fn default() -> Self {
        Compression::None
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum DeleteTarget {
    All,         // d=A
    ById,        // d=I
    ByPlacement, // d=P
    AtCursor,    // d=C
    InRange,     // d=R
    ByZIndex,    // d=Z
}

#[derive(Debug, Clone)]
pub struct KittyCommand {
    pub action: Option<KittyAction>,
    pub format: Option<ImageFormat>,
    pub transmission: Option<TransmissionMedium>,
    pub image_id: Option<u32>,
    pub placement_id: Option<u32>,
    pub more_chunks: bool,
    pub compression: Compression,
    pub source_x: Option<u32>,
    pub source_y: Option<u32>,
    pub source_width: Option<u32>,
    pub source_height: Option<u32>,
    pub pixel_offset_x: Option<u32>,
    pub pixel_offset_y: Option<u32>,
    pub columns: Option<u32>,
    pub rows: Option<u32>,
    pub unicode_placeholder: bool,
    pub quiet: u8,
    pub delete_target: Option<DeleteTarget>,
    pub z_index: Option<i32>,
    pub pixel_width: Option<u32>,
    pub pixel_height: Option<u32>,
    pub payload: Vec<u8>,
}

impl Default for KittyCommand {
    fn default() -> Self {
        KittyCommand {
            action: None,
            format: None,
            transmission: None,
            image_id: None,
            placement_id: None,
            more_chunks: false,
            compression: Compression::None,
            source_x: None,
            source_y: None,
            source_width: None,
            source_height: None,
            pixel_offset_x: None,
            pixel_offset_y: None,
            columns: None,
            rows: None,
            unicode_placeholder: false,
            quiet: 0,
            delete_target: None,
            z_index: None,
            pixel_width: None,
            pixel_height: None,
            payload: Vec::new(),
        }
    }
}

impl KittyCommand {
    /// Parse from raw APC data bytes (everything after 'G', before the ST terminator).
    ///
    /// The format is: `key=value,key=value,...;payload`
    /// If no semicolon is present, all data is control data and payload is empty.
    pub fn parse(data: &[u8]) -> Result<Self, String> {
        let mut cmd = KittyCommand::default();

        // Find the semicolon separating control data from payload
        let (control_data, payload) = match data.iter().position(|&b| b == b';') {
            Some(pos) => (&data[..pos], &data[pos + 1..]),
            None => (data, &[] as &[u8]),
        };

        cmd.payload = payload.to_vec();

        // Empty control data is valid (just payload)
        if control_data.is_empty() {
            return Ok(cmd);
        }

        let control_str = std::str::from_utf8(control_data)
            .map_err(|e| format!("invalid UTF-8 in control data: {}", e))?;

        // Split by commas and parse each key=value pair
        for pair in control_str.split(',') {
            if pair.is_empty() {
                continue;
            }

            let eq_pos = pair
                .find('=')
                .ok_or_else(|| format!("malformed key=value pair: '{}'", pair))?;

            let key = &pair[..eq_pos];
            let value = &pair[eq_pos + 1..];

            // Single-character keys only; multi-char keys are silently ignored
            // for forward compatibility
            if key.len() != 1 {
                continue;
            }

            let key_char = key.as_bytes()[0];
            Self::apply_key_value(&mut cmd, key_char, value)?;
        }

        Ok(cmd)
    }

    fn apply_key_value(cmd: &mut KittyCommand, key: u8, value: &str) -> Result<(), String> {
        match key {
            b'a' => {
                cmd.action = Some(match value {
                    "T" => KittyAction::TransmitAndDisplay,
                    "t" => KittyAction::Transmit,
                    "p" => KittyAction::Place,
                    "q" => KittyAction::Query,
                    "d" => KittyAction::Delete,
                    "f" => KittyAction::Frame,
                    "a" => KittyAction::Animate,
                    _ => return Err(format!("unknown action value: '{}'", value)),
                });
            },
            b'f' => {
                cmd.format = Some(match value {
                    "32" => ImageFormat::Rgba,
                    "24" => ImageFormat::Rgb,
                    "100" => ImageFormat::Png,
                    _ => return Err(format!("unknown format value: '{}'", value)),
                });
            },
            b't' => {
                cmd.transmission = Some(match value {
                    "d" => TransmissionMedium::Direct,
                    "f" => TransmissionMedium::File,
                    "t" => TransmissionMedium::TempFile,
                    "s" => TransmissionMedium::SharedMem,
                    _ => return Err(format!("unknown transmission value: '{}'", value)),
                });
            },
            b'i' => {
                cmd.image_id = Some(parse_u32(value, "image_id")?);
            },
            b'p' => {
                cmd.placement_id = Some(parse_u32(value, "placement_id")?);
            },
            b'm' => {
                cmd.more_chunks = match value {
                    "0" => false,
                    "1" => true,
                    _ => return Err(format!("invalid more_chunks value: '{}'", value)),
                };
            },
            b'o' => {
                cmd.compression = match value {
                    "z" => Compression::Zlib,
                    _ => Compression::None,
                };
            },
            b'x' => {
                cmd.source_x = Some(parse_u32(value, "source_x")?);
            },
            b'y' => {
                cmd.source_y = Some(parse_u32(value, "source_y")?);
            },
            b'w' => {
                cmd.source_width = Some(parse_u32(value, "source_width")?);
            },
            b'h' => {
                cmd.source_height = Some(parse_u32(value, "source_height")?);
            },
            b'X' => {
                cmd.pixel_offset_x = Some(parse_u32(value, "pixel_offset_x")?);
            },
            b'Y' => {
                cmd.pixel_offset_y = Some(parse_u32(value, "pixel_offset_y")?);
            },
            b'c' => {
                cmd.columns = Some(parse_u32(value, "columns")?);
            },
            b'r' => {
                cmd.rows = Some(parse_u32(value, "rows")?);
            },
            b'U' => {
                cmd.unicode_placeholder = value == "1";
            },
            b'q' => {
                let q = parse_u32(value, "quiet")?;
                if q > 2 {
                    return Err(format!("quiet value out of range: {}", q));
                }
                cmd.quiet = q as u8;
            },
            b'd' => {
                // Kitty protocol uses uppercase letters - be strict per spec
                cmd.delete_target = Some(match value {
                    "A" => DeleteTarget::All,
                    "I" => DeleteTarget::ById,
                    "P" => DeleteTarget::ByPlacement,
                    "C" => DeleteTarget::AtCursor,
                    "R" => DeleteTarget::InRange,
                    "Z" => DeleteTarget::ByZIndex,
                    _ => return Err(format!("unknown delete target: '{}'", value)),
                });
            },
            b'z' => {
                cmd.z_index = Some(
                    value
                        .parse::<i32>()
                        .map_err(|e| format!("invalid z_index '{}': {}", value, e))?,
                );
            },
            b's' => {
                cmd.pixel_width = Some(parse_u32(value, "pixel_width")?);
            },
            b'v' => {
                cmd.pixel_height = Some(parse_u32(value, "pixel_height")?);
            },
            // Unknown single-char keys: silently ignore for forward compatibility
            _ => {},
        }
        Ok(())
    }
}

fn parse_u32(value: &str, field: &str) -> Result<u32, String> {
    value
        .parse::<u32>()
        .map_err(|e| format!("invalid {} value '{}': {}", field, value, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_transmit_command() {
        let data = b"a=T,f=100,s=640,v=480,i=31,m=0;base64data";
        let cmd = KittyCommand::parse(data).unwrap();
        assert_eq!(cmd.action, Some(KittyAction::TransmitAndDisplay));
        assert_eq!(cmd.format, Some(ImageFormat::Png));
        assert_eq!(cmd.pixel_width, Some(640));
        assert_eq!(cmd.pixel_height, Some(480));
        assert_eq!(cmd.image_id, Some(31));
        assert!(!cmd.more_chunks);
        assert_eq!(cmd.payload, b"base64data");
    }

    #[test]
    fn parse_place_command() {
        let data = b"a=p,i=42,c=10,r=5";
        let cmd = KittyCommand::parse(data).unwrap();
        assert_eq!(cmd.action, Some(KittyAction::Place));
        assert_eq!(cmd.image_id, Some(42));
        assert_eq!(cmd.columns, Some(10));
        assert_eq!(cmd.rows, Some(5));
        assert!(cmd.payload.is_empty());
    }

    #[test]
    fn parse_delete_command() {
        let data = b"a=d,d=I,i=42";
        let cmd = KittyCommand::parse(data).unwrap();
        assert_eq!(cmd.action, Some(KittyAction::Delete));
        assert_eq!(cmd.delete_target, Some(DeleteTarget::ById));
        assert_eq!(cmd.image_id, Some(42));
    }

    #[test]
    fn unknown_key_ignored() {
        // Multi-char key "UNKNOWN" is silently ignored
        let data = b"a=T,UNKNOWN=99,i=1";
        let cmd = KittyCommand::parse(data).unwrap();
        assert_eq!(cmd.action, Some(KittyAction::TransmitAndDisplay));
        assert_eq!(cmd.image_id, Some(1));
    }

    #[test]
    fn no_semicolon_empty_payload() {
        let data = b"a=q";
        let cmd = KittyCommand::parse(data).unwrap();
        assert_eq!(cmd.action, Some(KittyAction::Query));
        assert!(cmd.payload.is_empty());
    }

    #[test]
    fn payload_extracted() {
        let data = b"a=T;PAYLOADBYTES";
        let cmd = KittyCommand::parse(data).unwrap();
        assert_eq!(cmd.action, Some(KittyAction::TransmitAndDisplay));
        assert_eq!(cmd.payload, b"PAYLOADBYTES");
    }

    #[test]
    fn malformed_pair_returns_error() {
        let data = b"a=T,badpair";
        assert!(KittyCommand::parse(data).is_err());
    }

    #[test]
    fn empty_control_data_with_payload() {
        let data = b";somedata";
        let cmd = KittyCommand::parse(data).unwrap();
        assert!(cmd.action.is_none());
        assert_eq!(cmd.payload, b"somedata");
    }

    #[test]
    fn zlib_compression() {
        let data = b"a=T,o=z,i=5";
        let cmd = KittyCommand::parse(data).unwrap();
        assert_eq!(cmd.compression, Compression::Zlib);
        assert_eq!(cmd.image_id, Some(5));
    }

    #[test]
    fn unicode_placeholder_flag() {
        let data = b"a=T,U=1,i=7";
        let cmd = KittyCommand::parse(data).unwrap();
        assert!(cmd.unicode_placeholder);
        assert_eq!(cmd.image_id, Some(7));
    }

    #[test]
    fn z_index_signed() {
        let data = b"a=p,z=-10,i=1";
        let cmd = KittyCommand::parse(data).unwrap();
        assert_eq!(cmd.z_index, Some(-10));
    }

    #[test]
    fn more_chunks_flag() {
        let data = b"a=T,m=1,i=3;chunk1";
        let cmd = KittyCommand::parse(data).unwrap();
        assert!(cmd.more_chunks);
        assert_eq!(cmd.payload, b"chunk1");
    }

    #[test]
    fn all_transmission_mediums() {
        for (val, expected) in &[
            ("d", TransmissionMedium::Direct),
            ("f", TransmissionMedium::File),
            ("t", TransmissionMedium::TempFile),
            ("s", TransmissionMedium::SharedMem),
        ] {
            let data = format!("t={}", val);
            let cmd = KittyCommand::parse(data.as_bytes()).unwrap();
            assert_eq!(cmd.transmission, Some(expected.clone()));
        }
    }

    #[test]
    fn all_image_formats() {
        for (val, expected) in &[
            ("32", ImageFormat::Rgba),
            ("24", ImageFormat::Rgb),
            ("100", ImageFormat::Png),
        ] {
            let data = format!("f={}", val);
            let cmd = KittyCommand::parse(data.as_bytes()).unwrap();
            assert_eq!(cmd.format, Some(expected.clone()));
        }
    }

    #[test]
    fn all_delete_targets() {
        for (val, expected) in &[
            ("A", DeleteTarget::All),
            ("I", DeleteTarget::ById),
            ("P", DeleteTarget::ByPlacement),
            ("C", DeleteTarget::AtCursor),
            ("R", DeleteTarget::InRange),
            ("Z", DeleteTarget::ByZIndex),
        ] {
            let data = format!("d={}", val);
            let cmd = KittyCommand::parse(data.as_bytes()).unwrap();
            assert_eq!(cmd.delete_target, Some(expected.clone()));
        }
    }
}
