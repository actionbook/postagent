/// Opens `url` in the system browser. Prints the URL to stderr and returns
/// `false` when the platform can't launch a browser — caller should advise the
/// user to copy it manually.
pub fn open(url: &str) -> bool {
    match webbrowser::open(url) {
        Ok(_) => true,
        Err(_) => {
            eprintln!("Could not open a browser. Visit this URL manually:\n  {}", url);
            false
        }
    }
}
