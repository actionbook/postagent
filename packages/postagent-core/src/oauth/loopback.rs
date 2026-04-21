use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

pub const REDIRECT_ADDR: &str = "127.0.0.1:9876";
const MAX_REQUEST_LINE_BYTES: usize = 8192;

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
                "Port 9876 is in use. Another `postagent auth` may be running, or free the port."
            ),
            LoopbackError::Timeout => write!(f, "Timed out waiting for OAuth callback."),
            LoopbackError::Io(e) => write!(f, "loopback IO error: {}", e),
        }
    }
}

impl std::error::Error for LoopbackError {}

/// Binds to `127.0.0.1:9876`, accepts one `GET /callback?...` request,
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
    read_request_line_from(stream)
}

fn read_request_line_from<R: Read>(reader: &mut R) -> Option<String> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];

    loop {
        let n = reader.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }

        buf.extend_from_slice(&chunk[..n]);
        if buf.len() > MAX_REQUEST_LINE_BYTES {
            return None;
        }

        if let Some(end) = buf.windows(2).position(|window| window == b"\r\n") {
            return std::str::from_utf8(&buf[..end])
                .ok()
                .map(std::string::ToString::to_string);
        }
    }
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
    // Self-contained offline page — no external requests so the loopback
    // server stays isolated. The check icon is inline SVG (no network /
    // emoji rendering variance). Auto-close attempts after 2.5s;
    // `window.close()` only succeeds in tabs opened by JS so the copy is
    // written to work either way.
    r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>postagent · authorized</title>
<style>
  :root {
    --bg:       #0b0d10;
    --bg-card:  #11151a;
    --border:   #1f262f;
    --fg:       #e8eaed;
    --muted:    #8892a0;
    --accent:   #4ade80;
    --accent-bg:#0f2218;
  }
  @media (prefers-color-scheme: light) {
    :root {
      --bg:       #f6f7f9;
      --bg-card:  #ffffff;
      --border:   #e4e7eb;
      --fg:       #0b0d10;
      --muted:    #5b6573;
      --accent:   #16a34a;
      --accent-bg:#dcfce7;
    }
  }
  * { box-sizing: border-box; }
  html, body { margin: 0; padding: 0; height: 100%; }
  body {
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto,
                 "Helvetica Neue", Arial, sans-serif;
    background: var(--bg);
    color: var(--fg);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px;
    -webkit-font-smoothing: antialiased;
  }
  .card {
    max-width: 440px;
    width: 100%;
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: 16px;
    padding: 40px 36px 32px;
    text-align: center;
    animation: card-in 360ms cubic-bezier(.2,.7,.2,1) both;
  }
  .ring {
    width: 72px; height: 72px;
    border-radius: 50%;
    background: var(--accent-bg);
    display: flex; align-items: center; justify-content: center;
    margin: 0 auto 20px;
    animation: pop 400ms cubic-bezier(.2,.7,.2,1) 120ms both;
  }
  .ring svg { width: 36px; height: 36px; color: var(--accent); }
  .ring svg path {
    stroke-dasharray: 32;
    stroke-dashoffset: 32;
    animation: draw 420ms ease-out 260ms forwards;
  }
  h1 {
    font-size: 22px;
    font-weight: 600;
    margin: 0 0 10px;
    letter-spacing: -0.01em;
  }
  p {
    margin: 0 0 4px;
    color: var(--muted);
    font-size: 14px;
    line-height: 1.55;
  }
  .hint {
    margin-top: 24px;
    padding-top: 16px;
    border-top: 1px solid var(--border);
    font-size: 12px;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  kbd {
    font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
    font-size: 11px;
    background: var(--border);
    padding: 2px 6px;
    border-radius: 4px;
    color: var(--fg);
  }
  @keyframes card-in {
    from { opacity: 0; transform: translateY(6px); }
    to   { opacity: 1; transform: translateY(0); }
  }
  @keyframes pop {
    from { transform: scale(.6); opacity: 0; }
    to   { transform: scale(1);  opacity: 1; }
  }
  @keyframes draw {
    to { stroke-dashoffset: 0; }
  }
</style>
</head>
<body>
  <main class="card" role="status" aria-live="polite">
    <div class="ring" aria-hidden="true">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor"
           stroke-width="3" stroke-linecap="round" stroke-linejoin="round">
        <path d="M5 12.5l4.5 4.5L19 7.5"/>
      </svg>
    </div>
    <h1>Authorization complete</h1>
    <p>postagent has received your credentials.</p>
    <p>You can close this tab and return to your terminal.</p>
    <div class="hint">This tab will try to close itself. If it doesn't, press <kbd>⌘W</kbd> / <kbd>Ctrl W</kbd>.</div>
  </main>
  <script>setTimeout(function(){ try { window.close(); } catch(e) {} }, 2500);</script>
</body>
</html>"#
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    struct ChunkedReader {
        chunks: Vec<Vec<u8>>,
        next: usize,
    }

    impl Read for ChunkedReader {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.next >= self.chunks.len() {
                return Ok(0);
            }
            let chunk = &self.chunks[self.next];
            self.next += 1;
            buf[..chunk.len()].copy_from_slice(chunk);
            Ok(chunk.len())
        }
    }

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
    fn read_request_line_handles_partial_reads() {
        let mut reader = ChunkedReader {
            chunks: vec![
                b"GET /callback?code=abc".to_vec(),
                b"&state=xyz HTTP/1.1\r\nHost: localhost\r\n\r\n".to_vec(),
            ],
            next: 0,
        };

        let line = read_request_line_from(&mut reader).unwrap();
        assert_eq!(line, "GET /callback?code=abc&state=xyz HTTP/1.1");
    }

    #[test]
    fn url_decode_basic() {
        assert_eq!(url_decode("hello%20world"), "hello world");
        assert_eq!(url_decode("a+b"), "a b");
        assert_eq!(url_decode("%2F"), "/");
    }
}
