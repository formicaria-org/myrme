//! The minimal HTTP/1.1 plumbing the model-server and search seams share — hand-rolled over
//! `std::net`, no framework and no TLS (localhost only). Kept tiny and in one place so [`openai`]
//! and [`search`] don't each grow their own copy.
//!
//! [`openai`]: crate::openai
//! [`search`]: crate::search

use crate::AgentError;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// Send a raw HTTP/1.1 request to `host:port` and read the whole response. The request **must** set
/// `Connection: close`, so the server closes when done and the read ends at EOF. Bounded by `timeout`.
pub fn send(
    host: &str,
    port: u16,
    request: &[u8],
    timeout: Duration,
) -> Result<Vec<u8>, AgentError> {
    let mut stream = TcpStream::connect((host, port)).map_err(|e| {
        AgentError::new(format!("cannot reach {host}:{port} — is it running? ({e})"))
    })?;
    let net = |e: std::io::Error| AgentError::new(format!("{host}:{port} I/O failed: {e}"));
    stream.set_read_timeout(Some(timeout)).map_err(net)?;
    stream.set_write_timeout(Some(timeout)).map_err(net)?;
    stream.write_all(request).map_err(net)?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).map_err(net)?;
    Ok(raw)
}

/// Split an HTTP response into head/body, require a `200`, decode a chunked body if present, and
/// return the body. A non-200 is an error naming the status line, not a parse attempt.
pub fn body(raw: &[u8]) -> Result<String, AgentError> {
    let text = String::from_utf8_lossy(raw);
    let (head, body) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| AgentError::new("malformed HTTP response"))?;
    let status = head.lines().next().unwrap_or("");
    if !status.contains(" 200") {
        return Err(AgentError::new(format!("server returned: {}", status.trim())));
    }
    if head.to_ascii_lowercase().contains("transfer-encoding: chunked") {
        dechunk(body)
    } else {
        Ok(body.to_string())
    }
}

/// Minimal HTTP/1.1 chunked-body decoder: `<hex-size>\r\n<data>\r\n…0\r\n\r\n`.
fn dechunk(body: &str) -> Result<String, AgentError> {
    let mut out = String::new();
    let mut rest = body;
    loop {
        let (size_line, after) = rest
            .split_once("\r\n")
            .ok_or_else(|| AgentError::new("truncated chunked response"))?;
        let size = usize::from_str_radix(size_line.trim(), 16)
            .map_err(|_| AgentError::new("bad chunk size in response"))?;
        if size == 0 {
            break;
        }
        if after.len() < size {
            return Err(AgentError::new("truncated chunk in response"));
        }
        out.push_str(&after[..size]);
        rest = after[size..].strip_prefix("\r\n").unwrap_or(&after[size..]);
    }
    Ok(out)
}

/// Percent-encode one query-string value: RFC 3986 unreserved characters pass through, everything
/// else becomes `%XX`. Enough for a search query in a URL, without pulling a URL crate.
pub fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_reads_a_content_length_response() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello";
        assert_eq!(body(raw).unwrap(), "hello");
    }

    #[test]
    fn body_decodes_a_chunked_response() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n";
        assert_eq!(body(raw).unwrap(), "hello");
    }

    #[test]
    fn body_reports_a_non_200_as_an_error() {
        let raw = b"HTTP/1.1 503 Service Unavailable\r\n\r\n";
        let err = body(raw).unwrap_err();
        assert!(format!("{err}").contains("503"), "got: {err}");
    }

    #[test]
    fn encode_keeps_unreserved_and_escapes_the_rest() {
        assert_eq!(encode("a b&c"), "a%20b%26c");
        assert_eq!(encode("plain-1.0_x~"), "plain-1.0_x~");
    }
}
