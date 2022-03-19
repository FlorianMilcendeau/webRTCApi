use anyhow::Result;

/// TODO - compress
pub fn encode(s: &str) -> String {
    base64::encode(s)
}

/// TODO - decompress
pub fn decode(s: &str) -> Result<String> {
    let b = base64::decode(s)?;

    Ok(String::from_utf8(b)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_test() {
        let str_encoded = encode("Hello world");
        assert_eq!(str_encoded, "SGVsbG8gd29ybGQ=");
    }

    #[test]
    fn decode_test() {
        let str_decoded = decode("SGVsbG8gd29ybGQ=").expect("Cannot decode");
        assert_eq!(str_decoded, "Hello world");
    }
}
