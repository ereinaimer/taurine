use super::types::AuthTokens;

const KEYRING_SERVICE: &str = "taurine-cloud";
const KEYRING_USER: &str = "session";

/// Stores the user's cloud authentication tokens in the OS credential vault.
pub fn store_tokens(tokens: &AuthTokens) -> crate::Result<()> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).map_err(keystore_unavailable)?;
    store_tokens_with(&entry, tokens)
}

/// Retrieves stored cloud authentication tokens from the OS credential vault, if any.
pub fn get_tokens() -> crate::Result<Option<AuthTokens>> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).map_err(keystore_unavailable)?;
    get_tokens_with(&entry)
}

/// Clears stored cloud authentication tokens from the OS credential vault.
pub fn clear_tokens() -> crate::Result<()> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).map_err(keystore_unavailable)?;
    clear_tokens_with(&entry)
}

pub use clear_tokens as clear_cloud_tokens;
pub use get_tokens as get_cloud_tokens;
pub use store_tokens as store_cloud_tokens;

pub(crate) fn store_tokens_with(entry: &keyring::Entry, tokens: &AuthTokens) -> crate::Result<()> {
    let json = serde_json::to_string(tokens)?;
    entry.set_password(&json).map_err(keystore_unavailable)?;
    Ok(())
}

pub(crate) fn get_tokens_with(entry: &keyring::Entry) -> crate::Result<Option<AuthTokens>> {
    match entry.get_password() {
        Ok(json) => {
            let tokens = serde_json::from_str(&json)?;
            Ok(Some(tokens))
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(keystore_unavailable(err)),
    }
}

pub(crate) fn clear_tokens_with(entry: &keyring::Entry) -> crate::Result<()> {
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(keystore_unavailable(err)),
    }
}

fn keystore_unavailable(err: keyring::Error) -> crate::Error {
    crate::Error::Service(format!("Cloud auth keystore error: {err}"))
}
