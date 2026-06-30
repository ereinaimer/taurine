use std::time::Duration;

use super::transformers::strip_argument_quotes;

pub fn resolve(key: &str) -> Option<String> {
    if !key.starts_with("http.") {
        return None;
    }

    let modifier = &key[5..];
    if let Some(rest) = modifier.strip_prefix("get(") {
        let url_str = rest.strip_suffix(')')?;
        resolve_get(url_str)
    } else if let Some(rest) = modifier.strip_prefix("status(") {
        let url_str = rest.strip_suffix(')')?;
        resolve_status(url_str)
    } else {
        None
    }
}

fn format_url(url: &str) -> String {
    let url = strip_argument_quotes(url.trim());
    if url.starts_with("http://") || url.starts_with("https://") {
        url.to_string()
    } else {
        format!("https://{}", url)
    }
}

fn resolve_get(url: &str) -> Option<String> {
    let url_str = format_url(url);
    if url_str == "https://" {
        return None;
    }

    let req = ureq::get(&url_str).timeout(Duration::from_secs(5));
    match req.call() {
        Ok(res) => res
            .into_string()
            .ok()
            .or_else(|| Some("[Error: Response not UTF-8]".to_string())),
        Err(ureq::Error::Status(code, _)) => Some(format!("[Error: HTTP {}]", code)),
        Err(_) => Some("[Error: HTTP request failed]".to_string()),
    }
}

fn resolve_status(url: &str) -> Option<String> {
    let url_str = format_url(url);
    if url_str == "https://" {
        return None;
    }

    let req = ureq::get(&url_str).timeout(Duration::from_secs(5));
    match req.call() {
        Ok(res) => Some(res.status().to_string()),
        Err(ureq::Error::Status(code, _)) => Some(code.to_string()),
        Err(_) => Some("[Error: Request failed]".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: We avoid making actual network requests in unit tests to prevent flaky CI.
    // However, we can test the URL formatting and error handling logic.

    #[test]
    fn test_format_url() {
        assert_eq!(format_url("example.com"), "https://example.com");
        assert_eq!(format_url("http://example.com"), "http://example.com");
        assert_eq!(format_url("https://example.com"), "https://example.com");
        assert_eq!(format_url("\"example.com\""), "https://example.com");
        assert_eq!(format_url("'example.com'"), "https://example.com");
        assert_eq!(format_url("  example.com  "), "https://example.com");
    }

    #[test]
    fn test_resolve_empty_url() {
        assert_eq!(resolve("http.get()"), None);
        assert_eq!(resolve("http.get(\"\")"), None);
        assert_eq!(resolve("http.status()"), None);
        assert_eq!(resolve("http.status(\"\")"), None);
    }

    #[test]
    fn test_resolve_invalid_modifier() {
        assert_eq!(resolve("http.post(example.com)"), None);
        assert_eq!(resolve("http.invalid(example.com)"), None);
    }

    #[test]
    fn test_resolve_get_success() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::thread;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0; 512];
                let _ = stream.read(&mut buf);
                let response = "HTTP/1.1 200 OK\r\nContent-Length: 12\r\n\r\nHello World!";
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });

        let url = format!("http://127.0.0.1:{}", port);
        let res = resolve_get(&url);
        assert_eq!(res, Some("Hello World!".to_string()));
    }

    #[test]
    fn test_resolve_get_timeout() {
        use std::io::Read;
        use std::net::TcpListener;
        use std::thread;
        use std::time::Duration;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0; 512];
                let _ = stream.read(&mut buf);
                thread::sleep(Duration::from_secs(6));
            }
        });

        let url = format!("http://127.0.0.1:{}", port);
        let res = resolve_get(&url);
        assert_eq!(res, Some("[Error: HTTP request failed]".to_string()));
    }

    #[test]
    fn test_resolve_invalid_url() {
        let res = resolve_get("http://127.0.0.1:1");
        assert_eq!(res, Some("[Error: HTTP request failed]".to_string()));
    }

    #[test]
    fn test_resolve_status_success() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::thread;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0; 512];
                let _ = stream.read(&mut buf);
                let response = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n";
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });

        let url = format!("http://127.0.0.1:{}", port);
        let res = resolve_status(&url);
        assert_eq!(res, Some("404".to_string()));
    }
}
