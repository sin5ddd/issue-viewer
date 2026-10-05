use serde::{Deserialize, Serialize};

/// Keyring payload. Legacy entries are a raw access token; new entries are this JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredAuth {
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_expires_at: Option<i64>,
}

/// Refresh this many seconds before GitHub's expires_in, so a request does not race expiry.
pub const REFRESH_SKEW_SECS: i64 = 60;

pub fn encode_stored_auth(auth: &StoredAuth) -> String {
    serde_json::to_string(auth).unwrap_or_else(|_| auth.access_token.clone())
}

pub fn decode_stored_auth(raw: &str) -> StoredAuth {
    let trimmed = raw.trim();
    if let Ok(auth) = serde_json::from_str::<StoredAuth>(trimmed) {
        if !auth.access_token.is_empty() {
            return auth;
        }
    }
    StoredAuth {
        access_token: trimmed.to_string(),
        refresh_token: None,
        access_expires_at: None,
    }
}

pub fn expires_at_from_ttl(now_unix: i64, expires_in: Option<u64>) -> Option<i64> {
    let secs = expires_in?;
    let secs = i64::try_from(secs).unwrap_or(i64::MAX);
    Some(now_unix.saturating_add(secs))
}

pub fn should_refresh(auth: &StoredAuth, now_unix: i64) -> bool {
    match (auth.refresh_token.as_deref(), auth.access_expires_at) {
        (Some(refresh), Some(exp)) if !refresh.is_empty() => now_unix + REFRESH_SKEW_SECS >= exp,
        _ => false,
    }
}

pub trait TokenStore {
    fn load(&self) -> Option<String>;
    fn save(&self, token: &str);
    fn clear(&self);
}

#[cfg(test)]
#[derive(Default)]
pub struct MemoryTokenStore {
    pub token: std::sync::Mutex<Option<String>>,
}

#[cfg(test)]
impl TokenStore for MemoryTokenStore {
    fn load(&self) -> Option<String> {
        self.token.lock().ok().and_then(|g| g.clone())
    }
    fn save(&self, token: &str) {
        if let Ok(mut g) = self.token.lock() {
            *g = Some(token.to_string());
        }
    }
    fn clear(&self) {
        if let Ok(mut g) = self.token.lock() {
            *g = None;
        }
    }
}

/// Windows Credential Manager / macOS Keychain / Linux Secret Service.
/// Requires native keyring crate features; without them storage is in-process mock.
pub struct KeyringTokenStore;

impl TokenStore for KeyringTokenStore {
    fn load(&self) -> Option<String> {
        keyring::Entry::new("issue-viewer", "github")
            .ok()
            .and_then(|e| e.get_password().ok())
    }
    fn save(&self, token: &str) {
        if let Ok(e) = keyring::Entry::new("issue-viewer", "github") {
            let _ = e.set_password(token);
        }
    }
    fn clear(&self) {
        if let Ok(e) = keyring::Entry::new("issue-viewer", "github") {
            let _ = e.delete_credential();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_roundtrip() {
        let s = MemoryTokenStore::default();
        assert!(s.load().is_none());
        s.save("gho_test");
        assert_eq!(s.load().as_deref(), Some("gho_test"));
        s.clear();
        assert!(s.load().is_none());
    }

    #[test]
    fn legacy_raw_token_has_no_refresh() {
        let auth = decode_stored_auth("gho_test");
        assert_eq!(auth.access_token, "gho_test");
        assert!(auth.refresh_token.is_none());
        assert!(auth.access_expires_at.is_none());
        assert!(!should_refresh(&auth, 1_700_000_000));
    }

    #[test]
    fn json_roundtrip_and_refresh_skew() {
        let auth = StoredAuth {
            access_token: "gho_test".into(),
            refresh_token: Some("ghr_test".into()),
            access_expires_at: Some(1_700_000_000),
        };
        let decoded = decode_stored_auth(&encode_stored_auth(&auth));
        assert_eq!(decoded, auth);
        assert!(!should_refresh(&decoded, 1_700_000_000 - 61));
        assert!(should_refresh(&decoded, 1_700_000_000 - 60));
        assert!(should_refresh(&decoded, 1_700_000_060));
    }

    #[test]
    fn empty_refresh_token_is_not_refreshable() {
        let auth = StoredAuth {
            access_token: "gho_test".into(),
            refresh_token: Some(String::new()),
            access_expires_at: Some(10),
        };
        assert!(!should_refresh(&auth, 10_000));
    }
}
