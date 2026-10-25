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

/// Send one `POST` and read the whole raw response — the **one** place a request line and its
/// framing headers are written, so the model, whisper and `fm-serve` seams cannot drift apart.
/// `extra` is zero or more complete `Name: value\r\n` header lines. The body is bytes because one
/// caller sends audio.
pub fn post(
    host: &str,
    port: u16,
    path: &str,
    content_type: &str,
    extra: &str,
    body: &[u8],
    timeout: Duration,
) -> Result<Vec<u8>, AgentError> {
    send(host, port, &request(host, port, path, content_type, extra, body), timeout)
}

/// A `POST` request's bytes: the request line, its framing headers and the body.
fn request(
    host: &str,
    port: u16,
    path: &str,
    content_type: &str,
    extra: &str,
    body: &[u8],
) -> Vec<u8> {
    let mut request = format!(
        "POST {path} HTTP/1.1\r\n\
         Host: {host}:{port}\r\n\
         {extra}Content-Type: {content_type}\r\n\
         Content-Length: {len}\r\n\
         Connection: close\r\n\r\n",
        len = body.len(),
    )
    .into_bytes();
    request.extend_from_slice(body);
    request
}

/// [`post`], with the response body handed to `on_line` **one line at a time as it arrives** — how
/// a streamed answer (server-sent events) is read while it is still being written (2026-10-25).
/// `timeout` bounds each read, not the whole answer. A non-200 is an error naming the status line.
pub fn post_lines(
    host: &str,
    port: u16,
    path: &str,
    content_type: &str,
    body: &[u8],
    timeout: Duration,
    on_line: &mut dyn FnMut(&str),
) -> Result<(), AgentError> {
    let mut stream = TcpStream::connect((host, port)).map_err(|e| {
        AgentError::new(format!("cannot reach {host}:{port} — is it running? ({e})"))
    })?;
    let net = |e: std::io::Error| AgentError::new(format!("{host}:{port} I/O failed: {e}"));
    stream.set_read_timeout(Some(timeout)).map_err(net)?;
    stream.set_write_timeout(Some(timeout)).map_err(net)?;
    stream.write_all(&request(host, port, path, content_type, "", body)).map_err(net)?;
    let mut lines = Lines::default();
    let mut buf = [0u8; 4096];
    loop {
        let n = stream.read(&mut buf).map_err(net)?;
        if n == 0 {
            break;
        }
        lines.feed(&buf[..n], on_line)?;
    }
    lines.finish(on_line)
}

/// An HTTP response read **as it arrives**: the head first (a `200` is required), then the body,
/// unchunked if it is chunked, cut into lines. Pure, so how a response split anywhere across
/// reads still comes out whole is tested without a socket.
#[derive(Default)]
struct Lines {
    raw: Vec<u8>,
    head: Option<bool>,
    left: usize,
    crlf: bool,
    text: Vec<u8>,
}

impl Lines {
    fn feed(&mut self, data: &[u8], on_line: &mut dyn FnMut(&str)) -> Result<(), AgentError> {
        self.raw.extend_from_slice(data);
        if self.head.is_none() {
            let Some(end) = self.raw.windows(4).position(|w| w == b"\r\n\r\n") else {
                return Ok(());
            };
            let head = String::from_utf8_lossy(&self.raw[..end]).into_owned();
            let status = head.lines().next().unwrap_or("");
            if !status.contains(" 200") {
                return Err(AgentError::new(format!("server returned: {}", status.trim())));
            }
            self.head = Some(head.to_ascii_lowercase().contains("transfer-encoding: chunked"));
            self.raw.drain(..end + 4);
        }
        if self.head == Some(true) {
            self.dechunk();
        } else {
            self.text.append(&mut self.raw);
        }
        while let Some(i) = self.text.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.text.drain(..=i).collect();
            on_line(String::from_utf8_lossy(&line).trim_end_matches(['\r', '\n']));
        }
        Ok(())
    }

    /// Move whole chunk data from `raw` to `text`, keeping a partial size line or chunk for later.
    fn dechunk(&mut self) {
        loop {
            if self.left > 0 {
                let n = self.left.min(self.raw.len());
                self.text.extend(self.raw.drain(..n));
                self.left -= n;
                self.crlf = self.left == 0;
                if self.raw.is_empty() {
                    return;
                }
            } else if self.crlf {
                if self.raw.len() < 2 {
                    return;
                }
                self.raw.drain(..2);
                self.crlf = false;
            } else {
                let Some(i) = self.raw.windows(2).position(|w| w == b"\r\n") else { return };
                let size = String::from_utf8_lossy(&self.raw[..i]);
                let size = usize::from_str_radix(size.split(';').next().unwrap_or("").trim(), 16);
                self.raw.drain(..i + 2);
                match size {
                    Ok(0) | Err(_) => {
                        self.raw.clear();
                        return;
                    }
                    Ok(n) => self.left = n,
                }
            }
        }
    }

    /// The end of the response: the last line, if it had no newline. No head at all is an error.
    fn finish(mut self, on_line: &mut dyn FnMut(&str)) -> Result<(), AgentError> {
        if self.head.is_none() {
            return Err(AgentError::new("malformed HTTP response"));
        }
        if !self.text.is_empty() {
            on_line(String::from_utf8_lossy(&std::mem::take(&mut self.text)).trim_end());
        }
        Ok(())
    }
}

/// Split an HTTP response into head/body, require a `200`, decode a chunked body if present, and
/// return the body. A non-200 is an error naming the status line, not a parse attempt.
pub fn body(raw: &[u8]) -> Result<String, AgentError> {
    let text = String::from_utf8_lossy(raw);
    let (head, body) =
        text.split_once("\r\n\r\n").ok_or_else(|| AgentError::new("malformed HTTP response"))?;
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
        let (size_line, after) =
            rest.split_once("\r\n").ok_or_else(|| AgentError::new("truncated chunked response"))?;
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
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
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

    /// A response cut anywhere across reads — inside the head, a chunk's size, its data or its
    /// closing CRLF — gives the same lines as one read.
    #[test]
    fn lines_come_out_whole_however_the_response_is_split() {
        let raw: &[u8] = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n\
            e\r\ndata: {\"a\":1}\n\r\nf\r\n\ndata: [DONE]\n\n\r\n0\r\n\r\n";
        let want = vec!["data: {\"a\":1}", "", "data: [DONE]", ""];
        for cut in 1..raw.len() {
            let mut got = Vec::new();
            let mut l = Lines::default();
            for part in [&raw[..cut], &raw[cut..]] {
                l.feed(part, &mut |s| got.push(s.to_string())).unwrap();
            }
            l.finish(&mut |s| got.push(s.to_string())).unwrap();
            assert_eq!(got, want, "cut at {cut}");
        }
        // Not chunked: the body as it comes, the last line without its newline too.
        let mut got = Vec::new();
        let mut l = Lines::default();
        l.feed(b"HTTP/1.1 200 OK\r\n\r\none\ntw", &mut |s| got.push(s.to_string())).unwrap();
        l.feed(b"o", &mut |s| got.push(s.to_string())).unwrap();
        l.finish(&mut |s| got.push(s.to_string())).unwrap();
        assert_eq!(got, ["one", "two"]);
        // A refusal is an error naming it.
        let err = Lines::default().feed(b"HTTP/1.1 503 Busy\r\n\r\n", &mut |_| {}).unwrap_err();
        assert!(format!("{err}").contains("503"), "{err}");
    }

    #[test]
    fn encode_keeps_unreserved_and_escapes_the_rest() {
        assert_eq!(encode("a b&c"), "a%20b%26c");
        assert_eq!(encode("plain-1.0_x~"), "plain-1.0_x~");
    }
}
