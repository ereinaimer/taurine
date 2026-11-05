//! OAuth helpers shared by the CLI login command and the TUI sign-in gate.

/// Extracts the authorization code from an OAuth callback request line.
pub fn extract_code_from_request_line(line: &str) -> Option<String> {
    let code_idx = line.find("code=")?;
    let after_code = &line[code_idx + 5..];
    let end_idx = after_code
        .find(['&', ' ', '\r', '\n'])
        .unwrap_or(after_code.len());

    let raw_code = &after_code[..end_idx];
    if raw_code.is_empty() {
        None
    } else {
        Some(raw_code.to_string())
    }
}

#[cfg(target_os = "windows")]
fn to_wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Opens `url` in the user's default browser.
pub fn open_browser(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        use std::ptr;
        let wide_verb = to_wide_null("open");
        let wide_url = to_wide_null(url);
        // SAFETY: ShellExecuteW invokes the default system handler for the URL
        // protocol. Both strings are valid null-terminated wide buffers that
        // outlive the call, and the call does not retain the pointers.
        let result = unsafe {
            windows_sys::Win32::UI::Shell::ShellExecuteW(
                ptr::null_mut(),
                wide_verb.as_ptr(),
                wide_url.as_ptr(),
                ptr::null(),
                ptr::null(),
                windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
            )
        };
        if result as isize > 32 {
            Ok(())
        } else {
            Err(std::io::Error::other("failed to open the default browser"))
        }
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map(|_| ())
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_code_from_callback_line() {
        assert_eq!(
            extract_code_from_request_line("GET /callback?code=abc123xyz HTTP/1.1"),
            Some("abc123xyz".to_string())
        );
        assert_eq!(
            extract_code_from_request_line("GET /callback?code=abc123xyz&state=state1 HTTP/1.1"),
            Some("abc123xyz".to_string())
        );
        assert_eq!(
            extract_code_from_request_line("GET /callback HTTP/1.1"),
            None
        );
        assert_eq!(
            extract_code_from_request_line("GET /callback?code= HTTP/1.1"),
            None
        );
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn to_wide_null_preserves_query_parameters() {
        let url = "https://example.supabase.co/auth/v1/authorize?provider=google&code_challenge=dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk&code_challenge_method=s256&redirect_to=http%3A%2F%2F127.0.0.1%3A54321%2Fcallback";
        let wide = to_wide_null(url);
        assert_eq!(*wide.last().unwrap(), 0);

        let decoded = String::from_utf16(&wide[..wide.len() - 1]).expect("valid utf-16");
        assert_eq!(decoded, url);
        assert_eq!(decoded.matches('&').count(), 3);
        assert!(decoded.contains("provider=google"));
        assert!(decoded.contains("code_challenge="));
        assert!(decoded.contains("code_challenge_method="));
        assert!(decoded.contains("redirect_to="));
    }
}
