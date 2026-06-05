/// Percent-encode per RFC 3986 unreserved set.
/// Unreserved: A-Z a-z 0-9 - _ . ~
pub fn encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for &b in input.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push_str(&format!("{:02X}", b));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::encode;

    #[test]
    fn unreserved_passthrough() {
        assert_eq!(encode("abcXYZ123-_.~"), "abcXYZ123-_.~");
    }

    #[test]
    fn slash_encoded() {
        assert_eq!(encode("owner/repo"), "owner%2Frepo");
    }

    #[test]
    fn space_and_special() {
        assert_eq!(encode("a b/c"), "a%20b%2Fc");
    }

    #[test]
    fn utf8_multibyte() {
        assert_eq!(encode("ñ"), "%C3%B1");
    }
}
