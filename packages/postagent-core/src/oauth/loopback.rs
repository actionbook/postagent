use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

pub const REDIRECT_ADDR: &str = "127.0.0.1:33421";

/// Result of a successful callback hit.
#[derive(Debug, Clone)]
pub struct CallbackData {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

/// Error returned by `listen_for_callback`.
#[derive(Debug)]
pub enum LoopbackError {
    PortInUse,
    Timeout,
    Io(std::io::Error),
}

impl std::fmt::Display for LoopbackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoopbackError::PortInUse => write!(
                f,
                "Port 33421 is in use. Another `postagent auth` may be running, or free the port."
            ),
            LoopbackError::Timeout => write!(f, "Timed out waiting for OAuth callback."),
            LoopbackError::Io(e) => write!(f, "loopback IO error: {}", e),
        }
    }
}

impl std::error::Error for LoopbackError {}

/// Binds to `127.0.0.1:33421`, accepts one `GET /callback?...` request,
/// returns the query-string params. Times out after `timeout`.
pub fn listen_for_callback(timeout: Duration) -> Result<CallbackData, LoopbackError> {
    let listener = TcpListener::bind(REDIRECT_ADDR).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AddrInUse {
            LoopbackError::PortInUse
        } else {
            LoopbackError::Io(e)
        }
    })?;

    // Non-blocking accept with a bounded deadline: simpler than threading, and
    // works fine for a single callback. Ctrl-C bypasses this through the OS
    // default SIGINT handler (exit 130).
    listener
        .set_nonblocking(true)
        .map_err(LoopbackError::Io)?;

    let deadline = std::time::Instant::now() + timeout;
    loop {
        if std::time::Instant::now() >= deadline {
            return Err(LoopbackError::Timeout);
        }

        match listener.accept() {
            Ok((mut stream, _)) => {
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .ok();
                stream
                    .set_write_timeout(Some(Duration::from_secs(5)))
                    .ok();

                let request_line = read_request_line(&mut stream).unwrap_or_default();
                let params = parse_query_from_request_line(&request_line);

                let data = CallbackData {
                    code: params.iter().find(|(k, _)| k == "code").map(|(_, v)| v.clone()),
                    state: params.iter().find(|(k, _)| k == "state").map(|(_, v)| v.clone()),
                    error: params.iter().find(|(k, _)| k == "error").map(|(_, v)| v.clone()),
                    error_description: params
                        .iter()
                        .find(|(k, _)| k == "error_description")
                        .map(|(_, v)| v.clone()),
                };

                let body = success_page();
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes());
                let _ = stream.flush();
                return Ok(data);
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            Err(e) => return Err(LoopbackError::Io(e)),
        }
    }
}

fn read_request_line(stream: &mut std::net::TcpStream) -> Option<String> {
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf).ok()?;
    if n == 0 {
        return None;
    }
    let text = std::str::from_utf8(&buf[..n]).ok()?;
    text.lines().next().map(|s| s.to_string())
}

/// Parse `GET /callback?code=X&state=Y HTTP/1.1` → [("code","X"),("state","Y")].
fn parse_query_from_request_line(line: &str) -> Vec<(String, String)> {
    let mut parts = line.split_whitespace();
    let _method = parts.next();
    let target = match parts.next() {
        Some(t) => t,
        None => return Vec::new(),
    };
    let q = match target.find('?') {
        Some(idx) => &target[idx + 1..],
        None => return Vec::new(),
    };
    q.split('&')
        .filter_map(|pair| {
            let mut kv = pair.splitn(2, '=');
            let k = kv.next()?;
            let v = kv.next().unwrap_or("");
            Some((url_decode(k), url_decode(v)))
        })
        .collect()
}

fn url_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hex = &input[i + 1..i + 3];
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    out.push(byte);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn success_page() -> String {
    // Bundled offline HTML with a 3-second auto-close hint. `window.close()`
    // only works for windows scripts opened; we still show the message either way.
    r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>postagent: signed in</title>
<style>
  body { font-family: -apple-system, BlinkMacSystemFont, Segoe UI, Roboto, sans-serif;
         background: #0b0d10; color: #e8eaed; margin: 0; min-height: 100vh;
         display: flex; align-items: center; justify-content: center; }
  .card { padding: 32px 40px; background: #11151a; border: 1px solid #232a33;
          border-radius: 12px; text-align: center; max-width: 420px; }
  h1 { font-size: 18px; margin: 0 0 8px; color: #a5d6ff; }
  p { margin: 0; color: #9ba7b4; font-size: 14px; line-height: 1.5; }
</style>
</head>
<body>
  <div class="card">
    <h1>Signed in</h1>
    <p>You can close this tab and return to your terminal.</p>
  </div>
  <script>setTimeout(function(){ try { window.close(); } catch(e) {} }, 3000);</script>
</body>
</html>"#
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_query_basic() {
        let q = parse_query_from_request_line("GET /callback?code=abc&state=xyz HTTP/1.1");
        assert_eq!(q.len(), 2);
        assert_eq!(q[0], ("code".into(), "abc".into()));
        assert_eq!(q[1], ("state".into(), "xyz".into()));
    }

    #[test]
    fn parse_query_url_encoded() {
        let q = parse_query_from_request_line(
            "GET /callback?state=a%2Fb&error_description=Access%20denied HTTP/1.1",
        );
        assert_eq!(q.iter().find(|(k, _)| k == "state").unwrap().1, "a/b");
        assert_eq!(
            q.iter().find(|(k, _)| k == "error_description").unwrap().1,
            "Access denied"
        );
    }

    #[test]
    fn parse_query_empty() {
        assert!(parse_query_from_request_line("GET / HTTP/1.1").is_empty());
        assert!(parse_query_from_request_line("").is_empty());
    }

    #[test]
    fn url_decode_basic() {
        assert_eq!(url_decode("hello%20world"), "hello world");
        assert_eq!(url_decode("a+b"), "a b");
        assert_eq!(url_decode("%2F"), "/");
    }
}
