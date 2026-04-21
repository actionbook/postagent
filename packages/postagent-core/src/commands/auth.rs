use crate::descriptor::{self, AuthMethod};
use crate::markdown;
use crate::oauth;
use crate::token::{self, AppConfig, AuthFile, AuthKind};
use std::collections::BTreeMap;
use std::io::{self, BufRead, Write};
use std::time::Duration;

pub struct LoginArgs<'a> {
    pub site: &'a str,
    pub token: Option<&'a str>,
    pub method: Option<&'a str>,
    pub client_id: Option<&'a str>,
    pub client_secret: Option<&'a str>,
    pub dry_run: bool,
    pub params: &'a [(String, String)],
    pub scopes: &'a [String],
}

pub fn login(args: LoginArgs<'_>) -> Result<(), Box<dyn std::error::Error>> {
    let site_lower = args.site.to_lowercase();

    // Legacy fast path: --token forces a static save regardless of descriptor.
    if let Some(t) = args.token {
        return save_static(&site_lower, "default", t);
    }

    // Fetch descriptor; fall back to the pre-Phase-1 prompt flow if the server
    // didn't include auth_methods.
    let methods_opt = crate::commands::manual::fetch_site_auth_methods(&site_lower)?;
    let methods = match methods_opt {
        Some(m) => m,
        None => return prompt_and_save_legacy(&site_lower),
    };

    if methods.is_empty() {
        eprintln!(
            "This spec's OAuth configuration is not upgraded. Contact the registry maintainer."
        );
        std::process::exit(1);
    }

    let selected = select_method(&methods, args.method)?;
    match selected {
        AuthMethod::Static(s) => handle_static(&site_lower, s),
        AuthMethod::Oauth2(o) => handle_oauth2(&site_lower, o, &args),
    }
}

fn select_method<'a>(
    methods: &'a [AuthMethod],
    requested_id: Option<&str>,
) -> Result<&'a AuthMethod, Box<dyn std::error::Error>> {
    if let Some(id) = requested_id {
        let found = methods.iter().find(|m| m.id() == id).ok_or_else(|| {
            format!(
                "method id `{}` not found. Available: {}",
                id,
                methods
                    .iter()
                    .map(|m| m.id())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
        return Ok(found);
    }
    if methods.len() == 1 {
        return Ok(&methods[0]);
    }

    // Interactive arrow-key picker when stdin is a TTY; falls back to the
    // numbered-choice prompt for non-interactive cases (CI, piped input,
    // Windows). Keeps both paths reachable from tests.
    #[cfg(unix)]
    if atty_check() {
        match select_method_interactive(methods)? {
            Some(idx) => return Ok(&methods[idx]),
            None => return Err("auth cancelled".into()),
        }
    }

    select_method_numbered(methods)
}

fn select_method_numbered<'a>(
    methods: &'a [AuthMethod],
) -> Result<&'a AuthMethod, Box<dyn std::error::Error>> {
    eprintln!(
        "This site supports {} authentication methods:",
        methods.len()
    );
    for (i, m) in methods.iter().enumerate() {
        eprintln!("  {}) {}", i + 1, m.label());
    }
    eprint!("Choice [1]: ");
    io::stderr().flush()?;

    let mut line = String::new();
    io::stdin().lock().read_line(&mut line)?;
    let trimmed = line.trim();
    let idx = if trimmed.is_empty() {
        0
    } else {
        match trimmed.parse::<usize>() {
            Ok(n) if n >= 1 && n <= methods.len() => n - 1,
            _ => return Err(format!("invalid choice: {}", trimmed).into()),
        }
    };
    Ok(&methods[idx])
}

/// Raw-mode interactive picker. Returns `Ok(Some(idx))` on Enter,
/// `Ok(None)` on Esc/q. Terminal state is always restored via RAII guard,
/// even on panic.
///
/// Key bindings:
///   ↑ / k            move up
///   ↓ / j            move down
///   Enter / Return   confirm selection
///   Esc / q          cancel
///   1..=9            jump to that 1-based index
#[cfg(unix)]
fn select_method_interactive(
    methods: &[AuthMethod],
) -> Result<Option<usize>, Box<dyn std::error::Error>> {
    use std::io::Read;
    use std::os::unix::io::AsRawFd;

    let stdin = io::stdin();
    let fd = stdin.as_raw_fd();

    // Snapshot current termios so the guard can restore on exit / panic.
    let original = unsafe {
        let mut t: libc::termios = std::mem::zeroed();
        libc::tcgetattr(fd, &mut t);
        t
    };

    struct TtyGuard {
        fd: i32,
        original: libc::termios,
    }
    impl Drop for TtyGuard {
        fn drop(&mut self) {
            unsafe {
                libc::tcsetattr(self.fd, libc::TCSANOW, &self.original);
            }
            // Show cursor in case we hid it; harmless if already visible.
            eprint!("\x1b[?25h");
            let _ = io::stderr().flush();
        }
    }
    let _guard = TtyGuard { fd, original };

    // cbreak mode: no echo, no line buffering, but signals still fire so
    // Ctrl-C still kills the process and Drop still restores the terminal.
    unsafe {
        let mut t = original;
        t.c_lflag &= !(libc::ECHO | libc::ICANON);
        t.c_cc[libc::VMIN] = 1;
        t.c_cc[libc::VTIME] = 0;
        libc::tcsetattr(fd, libc::TCSANOW, &t);
    }

    eprintln!("Select an auth method:");
    eprint!("\x1b[?25l"); // hide cursor for cleaner redraws

    let n = methods.len();
    let mut cursor = 0usize;
    render_menu(methods, cursor);

    let mut buf = [0u8; 8];
    let result: Option<usize> = loop {
        let nread = match stdin.lock().read(&mut buf) {
            Ok(n) if n > 0 => n,
            _ => break None, // EOF on stdin -> treat as cancel
        };
        let chunk = &buf[..nread];

        // Match in priority: multi-byte escape seqs first, then single bytes.
        let action = classify_key(chunk);

        match action {
            KeyAction::Up => {
                if cursor > 0 {
                    cursor -= 1;
                }
            }
            KeyAction::Down => {
                if cursor + 1 < n {
                    cursor += 1;
                }
            }
            KeyAction::Enter => break Some(cursor),
            KeyAction::Cancel => break None,
            KeyAction::Digit(d) => {
                // 1-based digit → 0-based index; clamp to len.
                if d >= 1 && d <= n {
                    cursor = d - 1;
                }
            }
            KeyAction::Unknown => {}
        }
        rerender_menu(methods, cursor);
    };

    // Clear the menu lines so the final screen doesn't keep the selector
    // prefix hanging around, then fall through to print a 1-line confirmation.
    clear_menu(n);
    match result {
        Some(idx) => {
            eprintln!("  → [{}] {}", methods[idx].id(), methods[idx].label());
        }
        None => {
            eprintln!("  (cancelled)");
        }
    }
    Ok(result)
}

#[cfg(unix)]
enum KeyAction {
    Up,
    Down,
    Enter,
    Cancel,
    Digit(usize),
    Unknown,
}

#[cfg(unix)]
fn classify_key(chunk: &[u8]) -> KeyAction {
    // Most terminals deliver an arrow as all three bytes (ESC `[` A/B) in
    // one read call. We match the 3-byte form first; an ESC alone means
    // the user pressed the Escape key.
    if chunk.len() >= 3 && chunk[0] == 0x1b && chunk[1] == b'[' {
        return match chunk[2] {
            b'A' => KeyAction::Up,
            b'B' => KeyAction::Down,
            _ => KeyAction::Unknown,
        };
    }
    if chunk.len() == 1 {
        return match chunk[0] {
            b'\x1b' | b'q' | b'Q' => KeyAction::Cancel,
            b'\r' | b'\n' => KeyAction::Enter,
            b'k' | b'K' => KeyAction::Up,
            b'j' | b'J' => KeyAction::Down,
            b if b.is_ascii_digit() && b != b'0' => KeyAction::Digit((b - b'0') as usize),
            _ => KeyAction::Unknown,
        };
    }
    KeyAction::Unknown
}

/// Emit one line per method. Cursor at `idx` gets a `> ` prefix; the rest
/// get two spaces so the label columns stay aligned.
#[cfg(unix)]
fn render_menu(methods: &[AuthMethod], idx: usize) {
    for (i, m) in methods.iter().enumerate() {
        let marker = if i == idx { "> " } else { "  " };
        // \x1b[2K clears the full line wherever the cursor sits; \r returns
        // to column 1 before writing so partial overwrites from prior
        // renders can't leak through.
        eprint!("\x1b[2K\r{}[{}] {}\n", marker, m.id(), m.label());
    }
    let _ = io::stderr().flush();
}

/// Move the cursor back up N lines, then re-render. Must match the number
/// of lines `render_menu` produced (one per method).
#[cfg(unix)]
fn rerender_menu(methods: &[AuthMethod], idx: usize) {
    eprint!("\x1b[{}A", methods.len());
    render_menu(methods, idx);
}

#[cfg(unix)]
fn clear_menu(n: usize) {
    eprint!("\x1b[{}A", n);
    for _ in 0..n {
        eprint!("\x1b[2K\n");
    }
    eprint!("\x1b[{}A", n);
    let _ = io::stderr().flush();
}

/// Raw-mode multi-select checkbox picker for OAuth scopes. Row 0 is a
/// "select all" toggle; rows 1..=N are one per catalog entry. Entries in
/// `defaults` start pre-checked so Enter without changes yields the
/// descriptor default set.
///
/// Returns `Ok(Some(names))` on Enter (possibly empty — treated as an
/// explicit empty override), `Ok(None)` on Esc/q so the caller can fall
/// back to the descriptor default.
///
/// Key bindings:
///   ↑ / k            move up
///   ↓ / j            move down
///   Space            toggle the focused row (or toggle all from row 0)
///   a                toggle all (independent of cursor position)
///   Enter / Return   confirm
///   Esc / q          cancel (fall back to defaults)
#[cfg(unix)]
fn select_scopes_interactive(
    catalog: &[descriptor::ScopeCatalogEntry],
    defaults: &std::collections::BTreeSet<String>,
) -> Result<Option<Vec<String>>, Box<dyn std::error::Error>> {
    use std::io::Read;
    use std::os::unix::io::AsRawFd;

    let stdin = io::stdin();
    let fd = stdin.as_raw_fd();

    let original = unsafe {
        let mut t: libc::termios = std::mem::zeroed();
        libc::tcgetattr(fd, &mut t);
        t
    };

    struct TtyGuard {
        fd: i32,
        original: libc::termios,
    }
    impl Drop for TtyGuard {
        fn drop(&mut self) {
            unsafe {
                libc::tcsetattr(self.fd, libc::TCSANOW, &self.original);
            }
            eprint!("\x1b[?25h");
            let _ = io::stderr().flush();
        }
    }
    let _guard = TtyGuard { fd, original };

    unsafe {
        let mut t = original;
        t.c_lflag &= !(libc::ECHO | libc::ICANON);
        t.c_cc[libc::VMIN] = 1;
        t.c_cc[libc::VTIME] = 0;
        libc::tcsetattr(fd, libc::TCSANOW, &t);
    }

    let mut selected: Vec<bool> = catalog.iter().map(|e| defaults.contains(&e.name)).collect();

    eprintln!("Select OAuth scopes (↑/↓ move, Space toggle, a = all, Enter confirm, Esc cancel):");
    eprintln!("  Defaults are pre-selected; the first row toggles every scope at once.");
    eprint!("\x1b[?25l");

    let rows = catalog.len() + 1; // +1 for the "select all" row
    let mut cursor = 0usize;
    render_scope_menu(catalog, &selected, cursor);

    let mut buf = [0u8; 8];
    let confirmed = loop {
        let nread = match stdin.lock().read(&mut buf) {
            Ok(n) if n > 0 => n,
            _ => break false, // EOF -> treat as cancel
        };
        let chunk = &buf[..nread];

        let mut moved = false;
        if chunk.len() >= 3 && chunk[0] == 0x1b && chunk[1] == b'[' {
            match chunk[2] {
                b'A' => {
                    cursor = cursor.saturating_sub(1);
                    moved = true;
                }
                b'B' => {
                    if cursor + 1 < rows {
                        cursor += 1;
                        moved = true;
                    }
                }
                _ => {}
            }
        } else if chunk.len() == 1 {
            match chunk[0] {
                b'\x1b' | b'q' | b'Q' => break false,
                b'\r' | b'\n' => break true,
                b'k' | b'K' => {
                    cursor = cursor.saturating_sub(1);
                    moved = true;
                }
                b'j' | b'J' => {
                    if cursor + 1 < rows {
                        cursor += 1;
                        moved = true;
                    }
                }
                b' ' => {
                    if cursor == 0 {
                        toggle_all(&mut selected);
                    } else {
                        let i = cursor - 1;
                        selected[i] = !selected[i];
                    }
                    moved = true;
                }
                b'a' | b'A' => {
                    toggle_all(&mut selected);
                    moved = true;
                }
                _ => {}
            }
        }

        if moved {
            rerender_scope_menu(rows, catalog, &selected, cursor);
        }
    };

    clear_menu(rows);

    if !confirmed {
        eprintln!("  (cancelled — falling back to default scopes)");
        return Ok(None);
    }

    let chosen: Vec<String> = catalog
        .iter()
        .zip(selected.iter())
        .filter_map(|(e, s)| if *s { Some(e.name.clone()) } else { None })
        .collect();

    eprintln!(
        "  → {} scope{} selected",
        chosen.len(),
        if chosen.len() == 1 { "" } else { "s" }
    );
    Ok(Some(chosen))
}

#[cfg(unix)]
fn toggle_all(selected: &mut [bool]) {
    let all_on = selected.iter().all(|s| *s);
    let new_state = !all_on;
    for s in selected.iter_mut() {
        *s = new_state;
    }
}

#[cfg(unix)]
fn render_scope_menu(catalog: &[descriptor::ScopeCatalogEntry], selected: &[bool], cursor: usize) {
    let all_on = selected.iter().all(|s| *s);
    let arrow = if cursor == 0 { "> " } else { "  " };
    let mark = if all_on { "✓" } else { " " };
    eprint!("\x1b[2K\r{}[{}] (select all)\n", arrow, mark);

    let name_w = catalog.iter().map(|e| e.name.len()).max().unwrap_or(0);
    for (i, entry) in catalog.iter().enumerate() {
        let arrow = if cursor == i + 1 { "> " } else { "  " };
        let mark = if selected[i] { "✓" } else { " " };
        let desc = entry.description.as_deref().unwrap_or("—");
        eprint!(
            "\x1b[2K\r{}[{}] {:<nw$}  {}\n",
            arrow,
            mark,
            entry.name,
            desc,
            nw = name_w
        );
    }
    let _ = io::stderr().flush();
}

#[cfg(unix)]
fn rerender_scope_menu(
    rows: usize,
    catalog: &[descriptor::ScopeCatalogEntry],
    selected: &[bool],
    cursor: usize,
) {
    eprint!("\x1b[{}A", rows);
    render_scope_menu(catalog, selected, cursor);
}

fn handle_static(
    site: &str,
    method: &descriptor::StaticAuthMethod,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(instructions) = method.setup_instructions.as_deref() {
        eprintln!();
        eprintln!("{}", markdown::render(instructions, oauth::REDIRECT_URI));
    } else if let Some(url) = method.setup_url.as_deref() {
        eprintln!("Go to {} to find your API key or access token.", url);
    } else {
        eprintln!(
            "Go to the {} dashboard to find your API key or access token.",
            site
        );
    }

    let prompt = format!(
        "Enter credentials (API key/access token) for \"{}\": ",
        site
    );
    let secret = read_secret(&prompt)?;
    save_static(site, &method.id, &secret)
}

fn save_static(
    site: &str,
    method_id: &str,
    secret: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let secret = validate_static_secret(secret)?;
    let auth = AuthFile {
        kind: Some(AuthKind::Static),
        method_id: Some(method_id.to_string()),
        api_key: Some(secret.to_string()),
        ..Default::default()
    };
    token::save_auth(site, &auth)?;

    let key_var = format!("$POSTAGENT.{}.API_KEY", site.to_uppercase());
    println!(
        "\nCredentials saved. Pass {} in `postagent send` and it will be replaced with your saved credentials.",
        key_var
    );
    Ok(())
}

fn validate_static_secret(secret: &str) -> Result<&str, Box<dyn std::error::Error>> {
    let trimmed = secret.trim();
    if trimmed.is_empty() {
        return Err("Error: credentials cannot be empty.".into());
    }
    Ok(trimmed)
}

fn handle_oauth2(
    site: &str,
    method: &descriptor::OAuth2AuthMethod,
    args: &LoginArgs<'_>,
) -> Result<(), Box<dyn std::error::Error>> {
    let provider = method.provider.as_deref();
    if let Some(provider) = provider {
        if token::load_provider_app(provider).is_some() {
            eprintln!(
                "Reusing shared credentials for provider \"{}\" — client_id/secret will not be prompted.",
                provider
            );
        }
    }

    // Render setup_instructions, if any. `{{redirect_uri}}` is substituted.
    if let Some(instructions) = method.setup_instructions.as_deref() {
        eprintln!();
        eprintln!("{}", markdown::render(instructions, oauth::REDIRECT_URI));
    }

    // Resolve required placeholders (authorize.extra_params references like
    // `{tenant}` pair with `params_required: ["tenant"]`).
    let placeholders = collect_placeholders(method, args.params)?;

    // Resolve client credentials: CLI flags > saved app.yaml > interactive.
    let existing_app = provider
        .and_then(token::load_provider_app)
        .or_else(|| token::load_app(site));
    let desc_hash = descriptor::descriptor_hash(&AuthMethod::Oauth2(method.clone()));

    let client_id = match args.client_id {
        Some(c) => c.to_string(),
        None => match existing_app.as_ref().filter(|a| a.method_id == method.id) {
            Some(a) => a.client_id.clone(),
            None => {
                eprint!("Press Enter to continue, Ctrl-C to cancel.");
                io::stderr().flush()?;
                let mut s = String::new();
                io::stdin().lock().read_line(&mut s).ok();
                read_secret("Client ID: ")?
            }
        },
    };
    if client_id.is_empty() {
        return Err("client_id cannot be empty".into());
    }

    let client_secret: Option<String> = if method.client.client_type == "confidential" {
        let raw = match args.client_secret {
            Some(s) => s.to_string(),
            None => match existing_app
                .as_ref()
                .and_then(|a| a.client_secret.clone())
                .filter(|_| args.client_id.is_none())
            {
                Some(s) => s,
                None => read_secret("Client Secret: ")?,
            },
        };
        if raw.is_empty() {
            return Err("client_secret cannot be empty for confidential client".into());
        }
        Some(raw)
    } else {
        args.client_secret.map(|s| s.to_string())
    };

    // Persist the app.yaml before we go through the browser flow so the user
    // doesn't have to retype credentials if the callback step fails.
    let app = AppConfig {
        method_id: method.id.clone(),
        client_id: client_id.clone(),
        client_secret: client_secret.clone(),
        descriptor_hash: desc_hash.clone(),
    };
    if let Some(provider) = provider {
        token::save_provider_app(provider, &app)?;
    } else {
        token::save_app(site, &app)?;
    }

    // Resolve the scope set, in priority order:
    //   1. --scope flags on the CLI (explicit override, highest priority)
    //   2. Interactive checkbox picker (TTY only, and only when the
    //      descriptor publishes a catalog to pick from)
    //   3. Descriptor default (scopes_override = None)
    let mut scopes_override: Option<Vec<String>> = if args.scopes.is_empty() {
        None
    } else {
        Some(args.scopes.to_vec())
    };

    #[cfg(unix)]
    if scopes_override.is_none() && atty_check() {
        if let Some(catalog) = method.scopes.catalog.as_ref() {
            if !catalog.is_empty() {
                let defaults: std::collections::BTreeSet<String> =
                    method.scopes.default.iter().cloned().collect();
                if let Some(picked) = select_scopes_interactive(catalog, &defaults)? {
                    scopes_override = Some(picked);
                }
                // Esc / cancel → fall through with scopes_override = None so
                // the descriptor default applies. Kill with Ctrl-C to abort.
            }
        }
    }

    // Surface the effective scope set before the browser flow so the caller
    // (human or AI agent) can catch wrong-scope situations before granting
    // consent. The default set is easy to miss when --scope is omitted.
    print_scope_notice(site, method, scopes_override.as_deref());

    let params = oauth::AuthParams {
        client_id: &client_id,
        client_secret: client_secret.as_deref(),
        scopes_override,
        placeholder_values: placeholders,
        dry_run: args.dry_run,
        timeout: Duration::from_secs(120),
    };

    let tokens = oauth::run_authorization_code_flow(method, &params)?;

    let now = chrono::Utc::now();
    let auth = AuthFile {
        kind: Some(AuthKind::Oauth2),
        method_id: Some(method.id.clone()),
        access_token: Some(tokens.access_token.clone()),
        refresh_token: tokens.refresh_token.clone(),
        expires_at: tokens
            .expires_in
            .map(|s| now + chrono::Duration::seconds(s)),
        token_type: tokens.token_type.clone(),
        scope: tokens.scope.clone(),
        obtained_at: Some(now),
        extras: tokens.extras.clone(),
        ..Default::default()
    };
    if let Some(provider) = provider {
        token::save_provider_auth(provider, &auth)?;
        token::save_provider_pointer(site, provider)?;
    } else {
        token::save_auth(site, &auth)?;
    }

    let token_var = format!("$POSTAGENT.{}.TOKEN", site.to_uppercase());
    println!(
        "\nSigned in to {}. Pass {} in `postagent send`.",
        site, token_var
    );
    Ok(())
}

fn print_scope_notice(
    site: &str,
    method: &descriptor::OAuth2AuthMethod,
    override_scopes: Option<&[String]>,
) {
    let (effective, is_override) = match override_scopes {
        Some(s) => (s.to_vec(), true),
        None => (method.scopes.default.clone(), false),
    };

    eprintln!();
    if effective.is_empty() {
        eprintln!("OAuth scopes: (none declared — provider default will apply)");
    } else {
        eprintln!(
            "OAuth scopes ({}): {}",
            if is_override { "override" } else { "default" },
            effective.join(&method.scopes.separator),
        );
    }

    if is_override {
        eprintln!(
            "  Note: --scope REPLACES the default set — double-check nothing required is missing."
        );
    } else {
        eprintln!(
            "  This is the default request. If you need different permissions, Ctrl-C and re-run with --scope <name> [<name> ...]."
        );
        if method.scopes.catalog.is_some() {
            eprintln!(
                "  Full scope catalog: postagent auth {} scopes",
                site.to_lowercase()
            );
        }
    }
    eprintln!();
}

fn collect_placeholders(
    method: &descriptor::OAuth2AuthMethod,
    flag_pairs: &[(String, String)],
) -> Result<BTreeMap<String, String>, Box<dyn std::error::Error>> {
    let mut out: BTreeMap<String, String> = flag_pairs.iter().cloned().collect();
    let required: &[String] = method.authorize.params_required.as_deref().unwrap_or(&[]);
    for key in required {
        if out.contains_key(key) {
            continue;
        }
        eprint!("{}: ", key);
        io::stderr().flush()?;
        let mut line = String::new();
        io::stdin().lock().read_line(&mut line)?;
        let v = line.trim().to_string();
        if v.is_empty() {
            return Err(format!("authorize parameter `{}` is required", key).into());
        }
        out.insert(key.clone(), v);
    }
    Ok(out)
}

fn prompt_and_save_legacy(site: &str) -> Result<(), Box<dyn std::error::Error>> {
    eprintln!(
        "Go to the {} dashboard to find your API key or access token.\n",
        site
    );
    let secret = read_secret(&format!(
        "Enter credentials (API key/access token) for \"{}\": ",
        site
    ))?;
    save_static(site, "default", &secret)
}

// ---------- Subcommands ----------

pub fn logout(site: &str) -> Result<(), Box<dyn std::error::Error>> {
    let site_lower = site.to_lowercase();
    if let Some(provider) = token::provider_for_site(&site_lower) {
        eprintln!(
            "Warning: {} shares provider credentials for \"{}\". Logging out here will also clear shared tokens for sibling sites using that provider.",
            site_lower, provider
        );
    }
    token::logout(&site_lower)?;
    println!("Logged out of {}.", site_lower);
    Ok(())
}

pub fn reset(site: &str) -> Result<(), Box<dyn std::error::Error>> {
    let site_lower = site.to_lowercase();
    if let Some(provider) = token::provider_for_site(&site_lower) {
        eprintln!(
            "Warning: {} shares provider credentials for \"{}\". Resetting here will also clear shared app credentials and tokens for sibling sites using that provider.",
            site_lower, provider
        );
    }
    token::reset(&site_lower)?;
    println!(
        "Cleared OAuth app + tokens for {}. Run `postagent auth {}` to re-register.",
        site_lower, site_lower
    );
    Ok(())
}

/// Lists the OAuth scope catalog for a site (from the server descriptor).
///
/// Users run this to discover which `--scope X` values they can pass when
/// they want to escalate beyond the default set. Renders one method per
/// OAuth2 method on the site (usually one); static methods are skipped.
///
/// Failure modes:
///   - Site has no OAuth2 methods → friendly message, exit 0
///   - OAuth2 method exists but no `scopes.catalog` field populated → tell
///     the user the registry hasn't captured the full scope list yet and
///     point them at `setup_url` to read provider docs directly. This
///     matches the "paste-only fallback" philosophy elsewhere: never a
///     dead end, always give them a next step.
pub fn scopes(site: &str) -> Result<(), Box<dyn std::error::Error>> {
    let site_lower = site.to_lowercase();
    let methods =
        crate::commands::manual::fetch_site_auth_methods(&site_lower)?.unwrap_or_default();

    let oauth_methods: Vec<&descriptor::OAuth2AuthMethod> = methods
        .iter()
        .filter_map(|m| match m {
            descriptor::AuthMethod::Oauth2(o) => Some(o),
            _ => None,
        })
        .collect();

    if oauth_methods.is_empty() {
        println!(
            "{}: no OAuth methods declared. Scopes only apply to OAuth 2.0 flows.",
            site_lower
        );
        return Ok(());
    }

    // Load local auth state so we can mark what's currently GRANTED (from
    // the token endpoint's echoed `scope` field) rather than just what the
    // descriptor's `default` would request. Users who re-auth with a wider
    // scope set need to see their new grants reflected here.
    let local_auth = token::load_auth(&site_lower);

    for (i, method) in oauth_methods.iter().enumerate() {
        if i > 0 {
            println!();
        }
        println!("=== {} / {} (oauth2)", site_lower, method.id);

        // Decide whether to mark "GRANTED" (actual, from saved auth.yaml)
        // or "DEFAULT" (hypothetical, from descriptor). Granted wins only
        // when the saved credentials are for THIS method and were obtained
        // via the OAuth flow (i.e. the token endpoint returned a scope
        // string). Anything else falls back to default — `--token` saves
        // and static methods never carry a scope field.
        let granted: Option<Vec<String>> = local_auth
            .as_ref()
            .filter(|a| {
                a.effective_kind() == AuthKind::Oauth2 && a.effective_method_id() == method.id
            })
            .and_then(|a| a.scope.clone())
            .map(|s| {
                let sep = method.scopes.separator.as_str();
                s.split(sep)
                    .map(|x| x.trim().to_string())
                    .filter(|x| !x.is_empty())
                    .collect()
            });

        // One-line status header so users know which column they're looking
        // at without reading the legend at the bottom.
        match &granted {
            Some(g) => println!(
                "  Status: authenticated — {} scope(s) currently granted",
                g.len()
            ),
            None => println!("  Status: not authenticated (showing default scope request)"),
        }

        match method.scopes.catalog.as_ref() {
            Some(catalog) if !catalog.is_empty() => {
                let name_w = catalog.iter().map(|e| e.name.len()).max().unwrap_or(0);

                // Authenticated and unauthenticated render as two separate
                // tables because the semantics of the marker column differ.
                // Keeping them split avoids the historical bug where a ✓
                // next to an unauth'd default scope read as "granted".
                match &granted {
                    Some(g) => {
                        // ✓ = actually granted by the token endpoint.
                        let granted_set: std::collections::BTreeSet<&str> =
                            g.iter().map(|s| s.as_str()).collect();
                        println!(
                            "  {:<3} {:<nw$}  {}",
                            "",
                            "SCOPE",
                            "DESCRIPTION",
                            nw = name_w
                        );
                        for entry in catalog {
                            let marker = if granted_set.contains(entry.name.as_str()) {
                                "✓"
                            } else {
                                " "
                            };
                            let desc = entry.description.as_deref().unwrap_or("—");
                            println!("  {:<3} {:<nw$}  {}", marker, entry.name, desc, nw = name_w);
                        }
                    }
                    None => {
                        // No ✓ column — nothing is granted yet. Tag rows
                        // that are part of the default authorize request
                        // with a trailing literal "default" so the column
                        // can't be misread as a checkbox.
                        let default_set: std::collections::BTreeSet<&str> =
                            method.scopes.default.iter().map(|s| s.as_str()).collect();
                        let desc_w = catalog
                            .iter()
                            .map(|e| e.description.as_deref().unwrap_or("—").len())
                            .max()
                            .unwrap_or(0);
                        println!(
                            "  {:<nw$}  {:<dw$}  {}",
                            "SCOPE",
                            "DESCRIPTION",
                            "",
                            nw = name_w,
                            dw = desc_w
                        );
                        for entry in catalog {
                            let desc = entry.description.as_deref().unwrap_or("—");
                            let tag = if default_set.contains(entry.name.as_str()) {
                                "default"
                            } else {
                                ""
                            };
                            println!(
                                "  {:<nw$}  {:<dw$}  {}",
                                entry.name,
                                desc,
                                tag,
                                nw = name_w,
                                dw = desc_w
                            );
                        }
                    }
                }

                // If the provider returned scopes we don't have in the
                // catalog, surface them so users / maintainers notice the
                // drift. Otherwise they'd silently disappear from the
                // table even though they're technically usable.
                if let Some(g) = &granted {
                    let catalog_names: std::collections::BTreeSet<&str> =
                        catalog.iter().map(|e| e.name.as_str()).collect();
                    let orphan: Vec<&String> = g
                        .iter()
                        .filter(|s| !catalog_names.contains(s.as_str()))
                        .collect();
                    if !orphan.is_empty() {
                        println!();
                        println!(
                            "  ! Granted but missing from catalog (spec author should update):"
                        );
                        for s in orphan {
                            println!("    ✓   {}", s);
                        }
                    }
                }

                println!();
                match &granted {
                    Some(_) => println!(
                        "  ✓ = currently granted to your saved credentials"
                    ),
                    None => println!(
                        "  default = included in the default authorize request (override with --scope)"
                    ),
                }
                println!(
                    "  Escalate with: postagent auth {} --scope <name> [...]",
                    site_lower
                );
                println!(
                    "  Note: --scope OVERRIDES the default set; re-list any defaults you want to keep."
                );
            }
            _ => {
                println!("  No scope catalog is published for this method yet.");
                if let Some(url) = method.setup_url.as_deref() {
                    println!("  Refer to provider docs: {}", url);
                }
                match &granted {
                    Some(g) => println!(
                        "  Currently granted: {}",
                        if g.is_empty() {
                            "(none)".to_string()
                        } else {
                            g.join(method.scopes.separator.as_str())
                        }
                    ),
                    None => println!(
                        "  Defaults requested: {}",
                        if method.scopes.default.is_empty() {
                            "(none)".to_string()
                        } else {
                            method.scopes.default.join(method.scopes.separator.as_str())
                        }
                    ),
                }
            }
        }
    }

    Ok(())
}

pub fn status(site: &str) -> Result<(), Box<dyn std::error::Error>> {
    let site = site.to_lowercase();
    let auth = token::load_auth(&site);
    let app = token::load_app(&site);

    if auth.is_none() && app.is_none() {
        println!("{}: no credentials saved.", site);
        return Ok(());
    }

    println!("=== {}", site);
    if let Some(a) = &auth {
        println!("  kind:       {:?}", a.effective_kind());
        println!("  method_id:  {}", a.effective_method_id());
        if let Some(scope) = &a.scope {
            // Providers return the granted scope string joined by their own
            // separator (space for Google/GitHub, comma for Figma). For
            // readability we split on both and print one per line so long
            // Google-style URLs don't wrap unpredictably in narrow terminals.
            let items: Vec<&str> = scope
                .split(|c: char| c.is_whitespace() || c == ',')
                .filter(|s| !s.is_empty())
                .collect();
            match items.as_slice() {
                [] => println!("  scope:      {}", scope),
                [one] => println!("  scope:      {}", one),
                [first, rest @ ..] => {
                    println!("  scope:      {}", first);
                    for s in rest {
                        println!("              {}", s);
                    }
                }
            }
        }
        if let Some(exp) = &a.expires_at {
            println!("  expires_at: {}", exp);
        }
        if let Some(obt) = &a.obtained_at {
            println!("  obtained:   {}", obt);
        }
    }
    if let Some(app) = &app {
        println!("  app_client: {}", redact(&app.client_id));
        if app.client_secret.is_some() {
            println!("  app_secret: <set>");
        }

        // Warn when the stored descriptor_hash no longer matches the current
        // server descriptor — usually means the spec author rotated the method.
        if let Ok(Some(methods)) = crate::commands::manual::fetch_site_auth_methods(&site) {
            if let Some(m) = methods.iter().find(|m| m.id() == app.method_id) {
                let current = descriptor::descriptor_hash(m);
                if current != app.descriptor_hash {
                    println!(
                        "  warning:    stale app config (descriptor changed); run `postagent auth {} reset`",
                        site
                    );
                }
            }
        }
    }
    Ok(())
}

fn redact(s: &str) -> String {
    if s.len() <= 6 {
        return "***".into();
    }
    format!("{}...{}", &s[..3], &s[s.len() - 3..])
}

// ---------- Secret prompt (unchanged behavior) ----------

fn read_secret(prompt: &str) -> Result<String, Box<dyn std::error::Error>> {
    eprint!("{}", prompt);
    io::stderr().flush()?;

    if atty_check() {
        read_secret_tty()
    } else {
        read_secret_pipe()
    }
}

fn atty_check() -> bool {
    unsafe { libc_isatty(0) }
}

#[cfg(unix)]
unsafe fn libc_isatty(fd: i32) -> bool {
    extern "C" {
        fn isatty(fd: i32) -> i32;
    }
    unsafe { isatty(fd) != 0 }
}

#[cfg(windows)]
unsafe fn libc_isatty(fd: i32) -> bool {
    extern "C" {
        fn _isatty(fd: i32) -> i32;
    }
    unsafe { _isatty(fd) != 0 }
}

fn read_secret_pipe() -> Result<String, Box<dyn std::error::Error>> {
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;
    Ok(buf.trim().to_string())
}

#[cfg(unix)]
fn read_secret_tty() -> Result<String, Box<dyn std::error::Error>> {
    use std::io::Read;
    use std::os::unix::io::AsRawFd;

    let stdin = io::stdin();
    let fd = stdin.as_raw_fd();

    let original = unsafe {
        let mut t: libc::termios = std::mem::zeroed();
        libc::tcgetattr(fd, &mut t);
        t
    };

    unsafe {
        let mut t = original;
        t.c_lflag &= !(libc::ECHO | libc::ICANON);
        t.c_cc[libc::VMIN] = 1;
        t.c_cc[libc::VTIME] = 0;
        libc::tcsetattr(fd, libc::TCSANOW, &t);
    }

    let mut input = String::new();
    let mut buf = [0u8; 1];
    loop {
        if stdin.lock().read_exact(&mut buf).is_err() {
            break;
        }
        match buf[0] {
            b'\n' | b'\r' => break,
            0x7f | 0x08 => {
                if !input.is_empty() {
                    input.pop();
                    eprint!("\x08 \x08");
                    io::stderr().flush().ok();
                }
            }
            0x03 => {
                unsafe { libc::tcsetattr(fd, libc::TCSANOW, &original) };
                eprintln!();
                std::process::exit(130);
            }
            c if c >= 0x20 => {
                input.push(c as char);
                eprint!("*");
                io::stderr().flush().ok();
            }
            _ => {}
        }
    }

    unsafe {
        libc::tcsetattr(fd, libc::TCSANOW, &original);
    }
    eprintln!();

    Ok(input)
}

#[cfg(windows)]
fn read_secret_tty() -> Result<String, Box<dyn std::error::Error>> {
    read_secret_pipe()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn classify_arrow_up_and_down() {
        assert!(matches!(classify_key(b"\x1b[A"), KeyAction::Up));
        assert!(matches!(classify_key(b"\x1b[B"), KeyAction::Down));
    }

    #[test]
    fn classify_vim_bindings() {
        assert!(matches!(classify_key(b"k"), KeyAction::Up));
        assert!(matches!(classify_key(b"j"), KeyAction::Down));
        assert!(matches!(classify_key(b"K"), KeyAction::Up));
        assert!(matches!(classify_key(b"J"), KeyAction::Down));
    }

    #[test]
    fn classify_enter_and_cancel() {
        assert!(matches!(classify_key(b"\r"), KeyAction::Enter));
        assert!(matches!(classify_key(b"\n"), KeyAction::Enter));
        assert!(matches!(classify_key(b"\x1b"), KeyAction::Cancel));
        assert!(matches!(classify_key(b"q"), KeyAction::Cancel));
        assert!(matches!(classify_key(b"Q"), KeyAction::Cancel));
    }

    #[test]
    fn classify_digits_are_1_indexed() {
        // `0` is NOT a selector digit — menu indices are 1-based.
        assert!(matches!(classify_key(b"0"), KeyAction::Unknown));
        assert!(matches!(classify_key(b"1"), KeyAction::Digit(1)));
        assert!(matches!(classify_key(b"9"), KeyAction::Digit(9)));
    }

    #[test]
    fn classify_ignores_noise() {
        assert!(matches!(classify_key(b""), KeyAction::Unknown));
        assert!(matches!(classify_key(b"x"), KeyAction::Unknown));
        assert!(matches!(classify_key(b"\x1b[Z"), KeyAction::Unknown)); // Shift+Tab
    }

    #[test]
    fn validate_static_secret_rejects_whitespace_only_values() {
        let err = validate_static_secret("   ").unwrap_err().to_string();
        assert!(err.contains("cannot be empty"));
    }

    #[test]
    fn validate_static_secret_trims_surrounding_whitespace() {
        assert_eq!(validate_static_secret("  secret  ").unwrap(), "secret");
    }

    #[test]
    fn logout_warning_mentions_shared_provider_scope() {
        let msg = format!(
            "Warning: {} shares provider credentials for \"{}\". Logging out here will also clear shared tokens for sibling sites using that provider.",
            "google-docs", "google"
        );
        assert!(msg.contains("sibling sites"));
        assert!(msg.contains("shared tokens"));
    }
}
