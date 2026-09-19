pub trait TokenStore {
    fn load(&self) -> Option<String>;
    fn save(&self, token: &str);
    fn clear(&self);
}

#[derive(Default)]
pub struct MemoryTokenStore {
    pub token: std::sync::Mutex<Option<String>>,
}

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
}
