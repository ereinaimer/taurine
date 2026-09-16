//! Same-user access control for the daemon IPC named pipe.
//!
//! The pipe carries unauthenticated control RPCs, so the kernel must refuse
//! clients outside the allow-list: the current user, SYSTEM and Administrators.
//! That mirrors the `0600` Unix socket and replaces the deleted bearer token.
//! A same-user process is trusted by design; nothing can stop it, token or not.

use std::io;

use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_ALL, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    BuildSecurityDescriptorW, BuildTrusteeWithSidW, EXPLICIT_ACCESS_W, GRANT_ACCESS, TRUSTEE_W,
};
use windows_sys::Win32::Security::{
    CreateWellKnownSid, GetLengthSid, GetTokenInformation, NO_INHERITANCE, PSECURITY_DESCRIPTOR,
    PSID, SECURITY_ATTRIBUTES, SECURITY_MAX_SID_SIZE, TOKEN_QUERY, TOKEN_USER, TokenUser,
    WinBuiltinAdministratorsSid, WinLocalSystemSid,
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
        // SAFETY: sd came from BuildSecurityDescriptorW (LocalAlloc) and is
        // freed exactly once here.
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

fn well_known_sid(kind: i32) -> io::Result<[u8; SECURITY_MAX_SID_SIZE as usize]> {
    let mut buf = [0u8; SECURITY_MAX_SID_SIZE as usize];
    let mut len = buf.len() as u32;
    // SAFETY: buf is a stack allocation of SECURITY_MAX_SID_SIZE bytes, which
    // fits any well-known SID; len is updated by the call.
    let ok = unsafe {
        CreateWellKnownSid(
            kind,
            std::ptr::null_mut(),
            buf.as_mut_ptr() as PSID,
            &mut len,
        )
    };
    if ok == 0 {
        return Err(win32_err("CreateWellKnownSid failed"));
    }
    Ok(buf)
}

impl PipeSecurity {
    /// Build the allow-list descriptor for whoever runs this process.
    pub fn current_user_only() -> io::Result<Self> {
        let user_sid = current_user_sid()?;
        let system_sid = well_known_sid(WinLocalSystemSid)?;
        let admin_sid = well_known_sid(WinBuiltinAdministratorsSid)?;

        // SAFETY: BuildTrusteeWithSidW only reads the SID for the duration of
        // the call; all three buffers outlive it. BuildSecurityDescriptorW
        // copies the entries into the new self-relative descriptor.
        let sd = unsafe {
            let mut trustees = [
                TRUSTEE_W::default(),
                TRUSTEE_W::default(),
                TRUSTEE_W::default(),
            ];
            BuildTrusteeWithSidW(&mut trustees[0], user_sid.as_ptr() as PSID);
            BuildTrusteeWithSidW(&mut trustees[1], system_sid.as_ptr() as PSID);
            BuildTrusteeWithSidW(&mut trustees[2], admin_sid.as_ptr() as PSID);

            let entries = trustees.map(|trustee| EXPLICIT_ACCESS_W {
                grfAccessPermissions: GENERIC_ALL,
                grfAccessMode: GRANT_ACCESS,
                grfInheritance: NO_INHERITANCE,
                Trustee: trustee,
            });

            let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
            let mut sd_size = 0u32;
            let status = BuildSecurityDescriptorW(
                &trustees[0],
                std::ptr::null(),
                entries.len() as u32,
                entries.as_ptr(),
                0,
                std::ptr::null(),
                std::ptr::null_mut(),
                &mut sd_size,
                &mut sd,
            );
            if status != 0 {
                return Err(io::Error::from_raw_os_error(status as i32));
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
