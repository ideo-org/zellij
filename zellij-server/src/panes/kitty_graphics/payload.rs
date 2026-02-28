use base64::decode;
use flate2::read::ZlibDecoder;
use std::io::Read;

#[derive(Debug, Clone, PartialEq)]
pub enum Compression {
    None,
    Zlib,
}

#[derive(Debug)]
pub enum DecodeError {
    InvalidBase64(String),
    DecompressionFailed(String),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::InvalidBase64(e) => write!(f, "invalid base64: {}", e),
            DecodeError::DecompressionFailed(e) => write!(f, "decompression failed: {}", e),
        }
    }
}

/// Decode a Kitty graphics protocol payload.
/// The payload is base64-encoded raw bytes, optionally zlib-compressed.
pub fn decode_kitty_payload(data: &[u8], compression: Compression) -> Result<Vec<u8>, DecodeError> {
    // 1. base64 decode
    let decoded = decode(data).map_err(|e| DecodeError::InvalidBase64(e.to_string()))?;

    // 2. if compression == Zlib, decompress using flate2
    match compression {
        Compression::None => Ok(decoded),
        Compression::Zlib => {
            let mut decoder = ZlibDecoder::new(&decoded[..]);
            let mut decompressed = Vec::new();
            decoder
                .read_to_end(&mut decompressed)
                .map_err(|e| DecodeError::DecompressionFailed(e.to_string()))?;
            Ok(decompressed)
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_decode_plain_base64() {
        // Known bytes: "Hello, World!"
        let original = b"Hello, World!";
        let encoded = base64::encode(original);
        let result = decode_kitty_payload(encoded.as_bytes(), Compression::None);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), original);
    }

    #[test]
    fn test_decode_zlib() {
        use flate2::write::ZlibEncoder;
        use flate2::Compression as FlateCompression;

        // Known bytes: "Test data for zlib compression"
        let original = b"Test data for zlib compression";

        // Compress with zlib
        let mut encoder = ZlibEncoder::new(Vec::new(), FlateCompression::default());
        encoder.write_all(original).unwrap();
        let compressed = encoder.finish().unwrap();

        // Base64 encode the compressed data
        let encoded = base64::encode(&compressed);

        // Decode with our function
        let result = decode_kitty_payload(encoded.as_bytes(), Compression::Zlib);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), original);
    }

    #[test]
    fn test_invalid_base64() {
        let invalid = b"!!not-valid!!";
        let result = decode_kitty_payload(invalid, Compression::None);
        assert!(result.is_err());
        match result {
            Err(DecodeError::InvalidBase64(_)) => (),
            _ => panic!("Expected InvalidBase64 error"),
        }
    }

    #[test]
    fn test_empty_payload() {
        let empty = b"";
        let result = decode_kitty_payload(empty, Compression::None);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Vec::<u8>::new());
    }
}
