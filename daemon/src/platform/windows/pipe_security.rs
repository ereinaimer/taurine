//! Same-user access control for the daemon IPC named pipe.
//!
//! The pipe carries unauthenticated control RPCs, so the kernel must refuse
//! clients outside the allow-list: the current user, SYSTEM and Administrators.
//! That mirrors the `0600` Unix socket and replaces the deleted bearer token.
//! A same-user process is trusted by design; nothing can stop it, token or not.

use std::io;

use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use windows_sys::Win32::Foundation::{CloseHandle, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
};
use windows_sys::Win32::Security::{
    GetLengthSid, GetTokenInformation, PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES,
    TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// Self-relative security descriptor granting pipe access to the current user,
/// SYSTEM and Administrators. Frees its allocation on drop.
pub struct PipeSecurity {
    sd: PSECURITY_DESCRIPTOR,
}

// SAFETY: the descriptor is fully built before share and never mutated after.
unsafe impl Send for PipeSecurity {}
unsafe impl Sync for PipeSecurity {}

impl Drop for PipeSecurity {
    fn drop(&mut self) {
        // SAFETY: sd came from ConvertStringSecurityDescriptor (LocalAlloc)
        // and is freed exactly once here.
        unsafe {
            LocalFree(self.sd as _);
        }
    }
}

fn win32_err(context: &str) -> io::Error {
    let err = io::Error::last_os_error();
    tracing::warn!("{context}: {err}");
    err
}

fn current_user_sid() -> io::Result<Vec<u8>> {
    // SAFETY: the token handle is closed before any return after it is opened;
    // all buffers are sized by the API itself and validated before use.
    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(win32_err("OpenProcessToken failed"));
        }

        let mut needed = 0u32;
        GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut needed);
        let mut buf = vec![0u8; needed as usize];
        let ok = GetTokenInformation(
            token,
            TokenUser,
            buf.as_mut_ptr() as *mut _,
            needed,
            &mut needed,
        );
        CloseHandle(token);
        if ok == 0 {
            return Err(win32_err("GetTokenInformation failed"));
        }
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        let len = GetLengthSid(user.User.Sid) as usize;
        Ok(std::slice::from_raw_parts(user.User.Sid as *const u8, len).to_vec())
    }
}

impl PipeSecurity {
    /// Build the allow-list descriptor for whoever runs this process: the
    /// current user, SYSTEM and Administrators, read/write only.
    pub fn current_user_only() -> io::Result<Self> {
        let user_sid = current_user_sid()?;

        // SAFETY: every pointer below borrows a live buffer for the duration
        // of its call; the converter copies the descriptor before returning.
        // SDDL over the programmatic builder: the builder output validated yet
        // pipe creation rejected it (ERROR_PRIVILEGE_NOT_HELD).
        let sd = unsafe {
            let mut sid_str: *mut u16 = std::ptr::null_mut();
            if ConvertSidToStringSidW(user_sid.as_ptr() as PSID, &mut sid_str) == 0 {
                return Err(win32_err("ConvertSidToStringSidW failed"));
            }
            let sid_string = {
                let mut len = 0;
                while *sid_str.add(len) != 0 {
                    len += 1;
                }
                String::from_utf16_lossy(std::slice::from_raw_parts(sid_str, len))
            };
            LocalFree(sid_str as _);

            let sddl = format!("D:(A;;GRGW;;;SY)(A;;GRGW;;;BA)(A;;GRGW;;;{sid_string})");
            let sddl_utf16: Vec<u16> = sddl.encode_utf16().chain(std::iter::once(0)).collect();
            let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
            let mut sd_size = 0u32;
            let ok = ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl_utf16.as_ptr(),
                1,
                &mut sd,
                &mut sd_size,
            );
            if ok == 0 {
                return Err(win32_err("ConvertStringSecurityDescriptor failed"));
            }
            sd
        };
        Ok(Self { sd })
    }

    /// Create one pipe instance carrying this descriptor. Mirrors the previous
    /// `ServerOptions` setup; only the DACL is new.
    pub fn create_server(
        &self,
        pipe_path: &str,
        first_instance: bool,
    ) -> io::Result<NamedPipeServer> {
        let mut attrs = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.sd,
            bInheritHandle: 0,
        };
        let mut options = ServerOptions::new();
        options
            .first_pipe_instance(first_instance)
            .reject_remote_clients(true);
        // SAFETY: attrs borrows self.sd, which outlives the call, and
        // CreateNamedPipeW copies the descriptor before returning.
        unsafe {
            options.create_with_security_attributes_raw(
                pipe_path,
                &mut attrs as *mut _ as *mut std::ffi::c_void,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_builds_for_current_user() {
        PipeSecurity::current_user_only().expect("DACL must build");
    }

    #[tokio::test]
    async fn same_user_client_connects() {
        let pipe_path = format!(r"\\.\pipe\taurine-test-{}", std::process::id());
        let security = PipeSecurity::current_user_only().expect("DACL must build");
        let server = security
            .create_server(&pipe_path, true)
            .expect("pipe instance");
        let client_task = tokio::spawn(async move {
            tokio::net::windows::named_pipe::ClientOptions::new()
                .open(&pipe_path)
                .expect("same-user client must open the pipe");
        });
        server.connect().await.expect("server accepts client");
        client_task.await.expect("client task joins");
    }
}
