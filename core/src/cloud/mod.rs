pub mod client;
pub mod crypto;
pub mod device;
pub mod keyring;
pub mod lease;
pub mod types;

pub use client::{CloudClient, extract_jwt_email, generate_pkce_challenge};
pub use crypto::{
    EncryptedEnvelope, clear_vault_dek, decrypt_payload, derive_vault_keys,
    get_or_create_vault_dek, get_vault_dek, store_vault_dek,
};
pub use device::{get_device_hardware_id, get_device_name, get_device_platform};
pub use keyring::{
    clear_cloud_tokens, clear_tokens, get_cloud_tokens, get_tokens, store_cloud_tokens,
    store_tokens,
};
pub use lease::{CapabilityLease, TimeAnchor, canonical_lease_payload, sign_lease, validate_lease};
pub use types::{AuthTokens, CloudConfig, CloudProfile};

#[cfg(test)]
mod crypto_tests;
#[cfg(test)]
mod lease_tests;
#[cfg(test)]
mod tests;
