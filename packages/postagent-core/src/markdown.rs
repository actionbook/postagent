//! Tiny Markdown renderer for `setup_instructions`. Supports headings, bold,
//! inline code, fenced blocks, links, and `{{redirect_uri}}` substitution.
//! No external dependencies; falls back to plain text when stderr isn't a TTY
//! or NO_COLOR is set.

const BOLD: &str = "\x1b[1m";
const INVERSE: &str = "\x1b[7m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

/// Render `input` as ANSI-decorated text. `{{redirect_uri}}` is replaced with
/// `redirect_uri`.
pub fn render(input: &str, redirect_uri: &str) -> String {
    render_with(input, redirect_uri, ansi_enabled())
}

fn ansi_enabled() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    // stderr is where we print to during auth flow.
    is_tty_stderr()
}

#[cfg(unix)]
fn is_tty_stderr() -> bool {
    extern "C" {
        fn isatty(fd: i32) -> i32;
    }
    unsafe { isatty(2) != 0 }
}

#[cfg(windows)]
fn is_tty_stderr() -> bool {
    extern "C" {
        fn _isatty(fd: i32) -> i32;
    }
    unsafe { _isatty(2) != 0 }
}

fn render_with(input: &str, redirect_uri: &str, ansi: bool) -> String {
    let bold_on = if ansi { BOLD } else { "" };
    let bold_off = if ansi { RESET } else { "" };
    let code_on = if ansi { INVERSE } else { "" };
    let code_off = if ansi { RESET } else { "" };
    let dim_on = if ansi { DIM } else { "" };
    let dim_off = if ansi { RESET } else { "" };

    // Preserve line-termination semantics: only emit a trailing `\n` after a
    // segment when the input had one. `split_inclusive` keeps the separator so
    // a source ending in `\n` produces the same count of output lines.
    let mut out = String::new();
    let mut in_fence = false;
    for raw_segment in input.split_inclusive('\n') {
        let (content, nl) = if let Some(stripped) = raw_segment.strip_suffix('\n') {
            (stripped, "\n")
        } else {
            (raw_segment, "")
        };
        let line = content.replace("{{redirect_uri}}", redirect_uri);

        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }

        if in_fence {
            out.push_str("    ");
            out.push_str(dim_on);
            out.push_str(&line);
            out.push_str(dim_off);
            out.push_str(nl);
            continue;
        }

        if let Some(rest) = heading_text(&line) {
            out.push_str(bold_on);
            out.push_str(rest);
            out.push_str(bold_off);
            out.push_str(nl);
            continue;
        }

        let transformed = transform_inline(&line, bold_on, bold_off, code_on, code_off);
        out.push_str(&transformed);
        out.push_str(nl);
    }

    out
}

fn heading_text(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    for prefix in ["### ", "## ", "# "] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return Some(rest);
        }
    }
    None
}

fn transform_inline(
    line: &str,
    bold_on: &str,
    bold_off: &str,
    code_on: &str,
    code_off: &str,
) -> String {
    let mut out = String::with_capacity(line.len());
    let bytes = line.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        // Markdown markers (`, *, [) are all ASCII, so byte-indexed lookups
        // around them are valid str slice boundaries.
        if bytes[i] == b'`' {
            if let Some(end) = find_next(bytes, b'`', i + 1) {
                out.push_str(code_on);
                out.push_str(&line[i + 1..end]);
                out.push_str(code_off);
                i = end + 1;
                continue;
            }
        }
        if i + 1 < bytes.len() && bytes[i] == b'*' && bytes[i + 1] == b'*' {
            if let Some(end) = find_double_star(bytes, i + 2) {
                out.push_str(bold_on);
                out.push_str(&line[i + 2..end]);
                out.push_str(bold_off);
                i = end + 2;
                continue;
            }
        }
        if bytes[i] == b'[' {
            if let Some((text_end, url_end)) = parse_link(bytes, i) {
                let text = &line[i + 1..text_end];
                let url = &line[text_end + 2..url_end];
                out.push_str(text);
                out.push_str(" (");
                out.push_str(url);
                out.push(')');
                i = url_end + 1;
                continue;
            }
        }
        // Fall-through: copy one UTF-8 char verbatim (byte-step would corrupt
        // multi-byte sequences like CJK).
        let step = utf8_char_len(bytes[i]);
        out.push_str(&line[i..i + step]);
        i += step;
    }
    out
}

fn utf8_char_len(first_byte: u8) -> usize {
    if first_byte < 0x80 {
        1
    } else if first_byte < 0xC0 {
        // Continuation byte (shouldn't occur at a boundary, but stay safe).
        1
    } else if first_byte < 0xE0 {
        2
    } else if first_byte < 0xF0 {
        3
    } else {
        4
    }
}

fn find_next(bytes: &[u8], target: u8, start: usize) -> Option<usize> {
    bytes[start..].iter().position(|&b| b == target).map(|p| start + p)
}

fn find_double_star(bytes: &[u8], start: usize) -> Option<usize> {
    let mut i = start;
    while i + 1 < bytes.len() {
        if bytes[i] == b'*' && bytes[i + 1] == b'*' {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn parse_link(bytes: &[u8], start: usize) -> Option<(usize, usize)> {
    // `[text](url)` — return (idx of `]`, idx of `)`).
    let text_end = find_next(bytes, b']', start + 1)?;
    if bytes.get(text_end + 1)? != &b'(' {
        return None;
    }
    let url_end = find_next(bytes, b')', text_end + 2)?;
    Some((text_end, url_end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_render_as_bold() {
        let out = render_with("# Title\n## Sub\n### Subsub\n", "http://x", true);
        assert!(out.contains("\x1b[1mTitle\x1b[0m"));
        assert!(out.contains("\x1b[1mSub\x1b[0m"));
        assert!(out.contains("\x1b[1mSubsub\x1b[0m"));
    }

    #[test]
    fn headings_plain_when_ansi_disabled() {
        let out = render_with("# Title\n", "http://x", false);
        assert_eq!(out, "Title\n");
    }

    #[test]
    fn bold_inline() {
        let out = render_with("hello **world** !", "http://x", true);
        assert!(out.contains("\x1b[1mworld\x1b[0m"));
    }

    #[test]
    fn code_inline() {
        let out = render_with("use `foo()` here", "http://x", true);
        assert!(out.contains("\x1b[7mfoo()\x1b[0m"));
    }

    #[test]
    fn fenced_block_indents_and_dims() {
        let out = render_with("before\n```\nline1\nline2\n```\nafter\n", "http://x", true);
        assert!(out.contains("before\n"));
        assert!(out.contains("    \x1b[2mline1\x1b[0m\n"));
        assert!(out.contains("    \x1b[2mline2\x1b[0m\n"));
        assert!(out.contains("after\n"));
    }

    #[test]
    fn link_rewrites_to_text_and_url() {
        let out = render_with("see [Notion](https://notion.so)", "http://x", false);
        assert!(out.contains("Notion (https://notion.so)"));
    }

    #[test]
    fn redirect_uri_substitution() {
        let out = render_with(
            "callback: {{redirect_uri}}\n",
            "http://127.0.0.1:9876/callback",
            false,
        );
        assert!(out.contains("http://127.0.0.1:9876/callback"));
        assert!(!out.contains("{{redirect_uri}}"));
    }

    #[test]
    fn list_markers_preserved() {
        let out = render_with("- item one\n1. item two\n", "http://x", false);
        assert_eq!(out, "- item one\n1. item two\n");
    }

    #[test]
    fn golden_notion_instructions() {
        let src = "### Create a Notion public integration\n\n1. Open [Notion integrations](https://www.notion.so/my-integrations)\n2. Click **New integration**\n3. Set the redirect URI:\n\n   ```\n   {{redirect_uri}}\n   ```\n\n4. Save\n";
        let out = render_with(src, "http://127.0.0.1:9876/callback", false);
        let expected = "Create a Notion public integration\n\n1. Open Notion integrations (https://www.notion.so/my-integrations)\n2. Click New integration\n3. Set the redirect URI:\n\n       http://127.0.0.1:9876/callback\n\n4. Save\n";
        assert_eq!(out, expected);
    }
}
