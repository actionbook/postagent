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
    pub no_browser: bool,
    pub params: &'a [(String, String)],
    pub scopes: &'a [String],
}

pub fn login(args: LoginArgs<'_>) -> Result<(), Box<dyn std::error::Error>> {
    let site_lower = args.site.to_lowercase();

    // Legacy fast path: --token forces a static save regardless of descriptor.
    if let Some(t) = args.token {
        return save_static(&site_lower, "default", t.trim());
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
                methods.iter().map(|m| m.id()).collect::<Vec<_>>().join(", ")
            )
        })?;
        return Ok(found);
    }
    if methods.len() == 1 {
        return Ok(&methods[0]);
    }

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
        eprintln!("Go to the {} dashboard to find your API key or access token.", site);
    }

    let prompt = format!(
        "Enter credentials (API key/access token) for \"{}\": ",
        site
    );
    let secret = read_secret(&prompt)?;
    if secret.is_empty() {
        eprintln!("Error: credentials cannot be empty.");
        std::process::exit(1);
    }

    save_static(site, &method.id, &secret)
}

fn save_static(
    site: &str,
    method_id: &str,
    secret: &str,
) -> Result<(), Box<dyn std::error::Error>> {
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

fn handle_oauth2(
    site: &str,
    method: &descriptor::OAuth2AuthMethod,
    args: &LoginArgs<'_>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Render setup_instructions, if any. `{{redirect_uri}}` is substituted.
    if let Some(instructions) = method.setup_instructions.as_deref() {
        eprintln!();
        eprintln!("{}", markdown::render(instructions, oauth::REDIRECT_URI));
    }

    // Resolve required placeholders (authorize.extra_params references like
    // `{tenant}` pair with `params_required: ["tenant"]`).
    let placeholders = collect_placeholders(method, args.params)?;

    // Resolve client credentials: CLI flags > saved app.yaml > interactive.
    let existing_app = token::load_app(site);
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
    token::save_app(site, &app)?;

    let scopes_override = if args.scopes.is_empty() {
        None
    } else {
        Some(args.scopes.to_vec())
    };

    let params = oauth::AuthParams {
        client_id: &client_id,
        client_secret: client_secret.as_deref(),
        scopes_override,
        placeholder_values: placeholders,
        no_browser: args.no_browser,
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
    token::save_auth(site, &auth)?;

    let token_var = format!("$POSTAGENT.{}.TOKEN", site.to_uppercase());
    println!(
        "\nSigned in to {}. Pass {} in `postagent send`.",
        site, token_var
    );
    Ok(())
}

fn collect_placeholders(
    method: &descriptor::OAuth2AuthMethod,
    flag_pairs: &[(String, String)],
) -> Result<BTreeMap<String, String>, Box<dyn std::error::Error>> {
    let mut out: BTreeMap<String, String> = flag_pairs.iter().cloned().collect();
    let required: &[String] = method
        .authorize
        .params_required
        .as_deref()
        .unwrap_or(&[]);
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
    if secret.is_empty() {
        eprintln!("Error: credentials cannot be empty.");
        std::process::exit(1);
    }
    save_static(site, "default", &secret)
}

// ---------- Subcommands ----------

pub fn logout(site: &str) -> Result<(), Box<dyn std::error::Error>> {
    token::logout(&site.to_lowercase())?;
    println!("Logged out of {}.", site.to_lowercase());
    Ok(())
}

pub fn reset_app(site: &str) -> Result<(), Box<dyn std::error::Error>> {
    token::reset_app(&site.to_lowercase())?;
    println!(
        "Cleared OAuth app + tokens for {}. Run `postagent auth {}` to re-register.",
        site.to_lowercase(),
        site.to_lowercase()
    );
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
            println!("  scope:      {}", scope);
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
                        "  warning:    stale app config (descriptor changed); run `postagent auth {} reset-app`",
                        site
                    );
                }
            }
        }
    }
    Ok(())
}

pub fn list() -> Result<(), Box<dyn std::error::Error>> {
    let sites = token::list_sites();
    if sites.is_empty() {
        println!("No credentials saved.");
        return Ok(());
    }
    for site in sites {
        let auth = token::load_auth(&site);
        let kind = match auth.as_ref().map(|a| a.effective_kind()) {
            Some(AuthKind::Static) => "static",
            Some(AuthKind::Oauth2) => "oauth2",
            None => "app-only",
        };
        let method = auth
            .as_ref()
            .map(|a| a.effective_method_id().to_string())
            .unwrap_or_else(|| "-".into());
        println!("  {}  {}  method={}", site, kind, method);
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
