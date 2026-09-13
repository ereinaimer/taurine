//! Test-only shared keystore for the CLI test binary.
//!
//! Mirrors `taurine_core::testing` (`core/src/utils/test_utils.rs`):
//! process-global `(service, user)` map so keyed reopens of one file see one
//! key. Pre-seeded with a fixed test key so parallel first-opens never race
//! on creation. Installed once per test-binary process.

use std::collections::HashMap;
use std::sync::{Mutex, Once, OnceLock};

static MOCK_KEYRING: Once = Once::new();

type TestPasswordMap = HashMap<(String, String), Vec<u8>>;

/// Fixed pre-seeded test key: hex of 32 `0x42` bytes.
const FIXED_TEST_DB_KEY_HEX: &[u8] =
    b"4242424242424242424242424242424242424242424242424242424242424242";

static SHARED_TEST_PASSWORDS: OnceLock<Mutex<TestPasswordMap>> = OnceLock::new();

fn shared_passwords() -> &'static Mutex<TestPasswordMap> {
    SHARED_TEST_PASSWORDS.get_or_init(|| {
        let mut map = HashMap::new();
        map.insert(
            ("taurine".to_string(), "db-key".to_string()),
            FIXED_TEST_DB_KEY_HEX.to_vec(),
        );
        Mutex::new(map)
    })
}

#[derive(Debug)]
struct SharedTestCredential {
    service: String,
    user: String,
}

impl SharedTestCredential {
    fn key(&self) -> (String, String) {
        (self.service.clone(), self.user.clone())
    }

    fn is_fixed_db_key(&self) -> bool {
        self.service == "taurine" && self.user == "db-key"
    }
}

impl keyring::credential::CredentialApi for SharedTestCredential {
    fn set_secret(&self, secret: &[u8]) -> keyring::Result<()> {
        shared_passwords()
            .lock()
            .expect("shared test keystore poisoned")
            .insert(self.key(), secret.to_vec());
        Ok(())
    }

    fn get_secret(&self) -> keyring::Result<Vec<u8>> {
        shared_passwords()
            .lock()
            .expect("shared test keystore poisoned")
            .get(&self.key())
            .cloned()
            .ok_or(keyring::Error::NoEntry)
    }

    fn delete_credential(&self) -> keyring::Result<()> {
        // Re-seed the fixed db-key instead of dropping it: deleting the
        // pre-seed would make later re-creation mint a different random key
        // and break reopen of earlier temp DBs encrypted with the fixed key.
        if self.is_fixed_db_key() {
            shared_passwords()
                .lock()
                .expect("shared test keystore poisoned")
                .insert(self.key(), FIXED_TEST_DB_KEY_HEX.to_vec());
            return Ok(());
        }
        let removed = shared_passwords()
            .lock()
            .expect("shared test keystore poisoned")
            .remove(&self.key())
            .is_some();
        removed.then_some(()).ok_or(keyring::Error::NoEntry)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn debug_fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

#[derive(Debug)]
struct SharedTestCredentialBuilder;

impl keyring::credential::CredentialBuilderApi for SharedTestCredentialBuilder {
    fn build(
        &self,
        _target: Option<&str>,
        service: &str,
        user: &str,
    ) -> keyring::Result<Box<keyring::credential::Credential>> {
        Ok(Box::new(SharedTestCredential {
            service: service.to_string(),
            user: user.to_string(),
        }))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn persistence(&self) -> keyring::credential::CredentialPersistence {
        keyring::credential::CredentialPersistence::ProcessOnly
    }
}

/// Installs the shared test keystore (once per CLI test-binary process).
/// Call from every test helper that opens a keyed DB, before the first open.
pub(crate) fn use_shared_test_keyring() {
    MOCK_KEYRING.call_once(|| {
        keyring::set_default_credential_builder(Box::new(SharedTestCredentialBuilder));
    });
}
