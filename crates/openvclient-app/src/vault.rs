//! At-rest encryption for the parts of `workspace.json` that can hold secrets
//! (SQL history, connection URIs with embedded passwords).
//!
//! Real secrets (passwords, keys, tokens, passphrases) never touch the file:
//! they live in the OS credential store. This module covers what is left.
//! The 256-bit key is generated once and kept in that same credential store,
//! so the workspace file alone cannot be read on another machine or account.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use std::sync::OnceLock;

const PREFIX: &str = "ovc-sealed:v1:";
const KEY_ACCOUNT: &str = "workspace-encryption-key";

static KEY: OnceLock<Option<[u8; 32]>> = OnceLock::new();

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

/// The workspace key, created on first use. `None` when the OS credential
/// store is unavailable (then data stays as it was, and a warning is logged).
fn key() -> Option<[u8; 32]> {
    *KEY.get_or_init(|| {
        // Tests (including the screenshot walkthroughs) never touch the real
        // credential store: they would prompt, and could replace the user's key.
        if cfg!(test) {
            return Some([7u8; 32]);
        }
        let entry = match keyring::Entry::new(crate::store::KEYCHAIN_SERVICE, KEY_ACCOUNT) {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!(target: "openvclient", "credential store unavailable, workspace history is not encrypted: {e}");
                return None;
            }
        };
        match entry.get_password() {
            Ok(existing) => {
                if let Ok(bytes) = b64().decode(existing.trim())
                    && bytes.len() == 32
                {
                    let mut k = [0u8; 32];
                    k.copy_from_slice(&bytes);
                    return Some(k);
                }
                // Never replace a key that exists: that would make everything
                // sealed with it unreadable.
                tracing::warn!(target: "openvclient", "the workspace key in the credential store is unreadable; history stays as it is");
                return None;
            }
            Err(keyring::Error::NoEntry) => {} // first run: create the key below
            Err(e) => {
                // Locked store, denied prompt…: try again next launch, never overwrite.
                tracing::warn!(target: "openvclient", "could not read the workspace key, history is not encrypted this session: {e}");
                return None;
            }
        }
        let mut k = [0u8; 32];
        getrandom::getrandom(&mut k).ok()?;
        match entry.set_password(&b64().encode(k)) {
            Ok(()) => Some(k),
            Err(e) => {
                tracing::warn!(target: "openvclient", "could not store the workspace key, history is not encrypted: {e}");
                None
            }
        }
    })
}

pub fn is_sealed(s: &str) -> bool {
    s.starts_with(PREFIX)
}

/// Encrypt `plain` (AES-256-GCM, random 96-bit nonce). Returns the input
/// unchanged when no key is available.
pub fn seal(plain: &str) -> String {
    let Some(k) = key() else { return plain.to_string() };
    let cipher = Aes256Gcm::new(&k.into());
    let mut nonce = [0u8; 12];
    if getrandom::getrandom(&mut nonce).is_err() {
        return plain.to_string();
    }
    match cipher.encrypt(Nonce::from_slice(&nonce), plain.as_bytes()) {
        Ok(ct) => {
            let mut blob = nonce.to_vec();
            blob.extend_from_slice(&ct);
            format!("{PREFIX}{}", b64().encode(blob))
        }
        Err(_) => plain.to_string(),
    }
}

/// Decrypt a value made by [`seal`]; plain values pass through. `None` when
/// a sealed value cannot be opened (different machine/account, or tampered).
pub fn open(s: &str) -> Option<String> {
    let Some(body) = s.strip_prefix(PREFIX) else { return Some(s.to_string()) };
    let k = key()?;
    let blob = b64().decode(body).ok()?;
    if blob.len() < 12 + 16 {
        return None;
    }
    let (nonce, ct) = blob.split_at(12);
    let pt = Aes256Gcm::new(&k.into()).decrypt(Nonce::from_slice(nonce), ct).ok()?;
    String::from_utf8(pt).ok()
}

/// Whether a connection parameter value carries a secret worth sealing
/// (a URL with `user:password@`).
pub fn param_has_secret(v: &str) -> bool {
    openvclient_core::redact(v) != v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_when_store_available() {
        let s = seal("SELECT 'secret'");
        match key() {
            Some(_) => {
                assert!(is_sealed(&s) && !s.contains("secret"));
                assert_eq!(open(&s).as_deref(), Some("SELECT 'secret'"));
                let mut bad = s.clone();
                bad.pop();
                bad.push('A');
                assert!(open(&bad).is_none() || open(&bad).as_deref() != Some("SELECT 'secret'"));
            }
            None => assert_eq!(s, "SELECT 'secret'"),
        }
        assert_eq!(open("plain").as_deref(), Some("plain"));
        assert!(param_has_secret("mongodb://u:p@h/x") && !param_has_secret("mongodb://h/x"));
    }
}
