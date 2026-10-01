//! Device fingerprinting and platform identification for Taurine Cloud.

/// Retrieves a persistent hardware identifier for this device.
/// On Windows, reads `MachineGuid` from `HKLM\SOFTWARE\Microsoft\Cryptography`.
/// Falls back to a deterministic SHA-256 hash of system identifiers.
pub fn get_device_hardware_id() -> String {
    #[cfg(windows)]
    if let Some(guid) = query_windows_machine_guid() {
        return guid;
    }

    fallback_hardware_id()
}

/// Retrieves a human-readable name for this device (e.g., Computer Name).
pub fn get_device_name() -> String {
    if let Ok(name) = std::env::var("COMPUTERNAME")
        && !name.trim().is_empty()
    {
        return name.trim().to_string();
    }
    if let Ok(name) = std::env::var("HOSTNAME")
        && !name.trim().is_empty()
    {
        return name.trim().to_string();
    }
    if let Some(name) = sysinfo::System::host_name()
        && !name.trim().is_empty()
    {
        return name.trim().to_string();
    }
    "unknown-device".to_string()
}

/// Retrieves the platform identifier (e.g., "windows", "macos", "linux").
pub fn get_device_platform() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unknown"
    }
}

#[cfg(windows)]
fn query_windows_machine_guid() -> Option<String> {
    use winreg::RegKey;
    use winreg::enums::HKEY_LOCAL_MACHINE;

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let crypto = hklm.open_subkey("SOFTWARE\\Microsoft\\Cryptography").ok()?;
    let guid: String = crypto.get_value("MachineGuid").ok()?;
    let trimmed = guid.trim();
    if !trimmed.is_empty() {
        Some(trimmed.to_string())
    } else {
        None
    }
}

pub(crate) fn fallback_hardware_id() -> String {
    use sha2::{Digest, Sha256};

    let name = get_device_name();
    let platform = get_device_platform();
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown-user".to_string());

    let mut hasher = Sha256::new();
    hasher.update(name.as_bytes());
    hasher.update(b":");
    hasher.update(platform.as_bytes());
    hasher.update(b":");
    hasher.update(user.as_bytes());
    hex::encode(hasher.finalize())
}
