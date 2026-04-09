use crate::token;
use std::io::{self, Write};

pub fn run(site: &str) -> Result<(), Box<dyn std::error::Error>> {
    let site_lower = site.to_lowercase();
    let key_var = format!("$POSTAGENT.{}.API_KEY", site.to_uppercase());

    eprintln!("Go to the {} dashboard to find your API key or access token.\n", site_lower);
    let api_key = read_secret(&format!("Enter credentials (API key/access token) for \"{}\": ", site_lower))?;

    if api_key.is_empty() {
        eprintln!("Error: credentials cannot be empty.");
        std::process::exit(1);
    }

    match token::save_token(site, &api_key) {
        Ok(()) => {
            println!("\nCredentials saved. Pass {} in `postagent send` and it will be replaced with your saved credentials.", key_var);
            Ok(())
        }
        Err(e) => {
            let err_str = e.to_string();
            if err_str.contains("Permission denied") || err_str.contains("permission denied") {
                eprintln!("Error: Permission denied. Check directory permissions.");
            } else {
                eprintln!("Error: {}", err_str);
            }
            std::process::exit(1);
        }
    }
}

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

    // Save original terminal settings
    let original = unsafe {
        let mut t: libc::termios = std::mem::zeroed();
        libc::tcgetattr(fd, &mut t);
        t
    };

    // Disable echo and canonical mode (read char by char)
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
            // Backspace or DEL
            0x7f | 0x08 => {
                if !input.is_empty() {
                    input.pop();
                    eprint!("\x08 \x08");
                    io::stderr().flush().ok();
                }
            }
            // Ctrl-C
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

    // Restore original terminal settings
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
