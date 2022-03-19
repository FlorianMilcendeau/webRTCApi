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
