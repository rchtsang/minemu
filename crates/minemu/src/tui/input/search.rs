pub fn ascii(value: &str) -> Result<Vec<u8>, String> {
    if value.is_empty() {
        return Err("ASCII search pattern cannot be empty".into());
    }
    Ok(value.as_bytes().to_vec())
}

pub fn bytes(value: &str) -> Result<Vec<u8>, String> {
    let bytes = value
        .split_whitespace()
        .map(|byte| {
            if byte.len() != 2 {
                return Err(format!("invalid hex byte: {byte}"));
            }
            u8::from_str_radix(byte, 16).map_err(|_| format!("invalid hex byte: {byte}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if bytes.is_empty() {
        return Err("byte search pattern cannot be empty".into());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_ascii_and_hex_patterns() {
        assert_eq!(super::ascii("hello").unwrap(), b"hello");
        assert_eq!(
            super::bytes("de ad BE ef").unwrap(),
            [0xde, 0xad, 0xbe, 0xef]
        );
        assert!(super::bytes("xyz").is_err());
    }
}
