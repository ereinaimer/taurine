pub mod client;
pub mod device;
pub mod keyring;
pub mod lease;
pub mod types;

pub use client::{CloudClient, generate_pkce_challenge};
pub use device::{get_device_hardware_id, get_device_name, get_device_platform};
pub use keyring::{
    clear_cloud_tokens, clear_tokens, get_cloud_tokens, get_tokens, store_cloud_tokens,
    store_tokens,
};
pub use lease::{CapabilityLease, TimeAnchor, canonical_lease_payload, sign_lease, validate_lease};
pub use types::{AuthTokens, CloudConfig, CloudProfile};

#[cfg(test)]
mod lease_tests;
#[cfg(test)]
mod tests;
