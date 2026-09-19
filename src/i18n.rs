#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    En,
    Ja,
}

impl Lang {
    pub fn detect() -> Self {
        let loc = sys_locale::get_locale().unwrap_or_default();
        if loc.to_ascii_lowercase().starts_with("ja") {
            Lang::Ja
        } else {
            Lang::En
        }
    }

    pub fn t(self, key: &'static str) -> &'static str {
        match (self, key) {
            (Lang::En, "sign_in") => "Sign in with GitHub",
            (Lang::Ja, "sign_in") => "GitHub でサインイン",
            (Lang::En, "sign_out") => "Sign out",
            (Lang::Ja, "sign_out") => "サインアウト",
            (Lang::En, "refresh") => "Refresh",
            (Lang::Ja, "refresh") => "更新",
            (Lang::En, "last_synced") => "Last synced",
            (Lang::Ja, "last_synced") => "最終同期",
            (Lang::En, "open_issue") => "Open on GitHub",
            (Lang::Ja, "open_issue") => "GitHub で開く",
            (Lang::En, "loading") => "Loading…",
            (Lang::Ja, "loading") => "読み込み中…",
            (Lang::En, "offline") => "Offline — showing cache",
            (Lang::Ja, "offline") => "オフライン — キャッシュを表示",
            (Lang::En, "user_code") => "Enter this code at GitHub",
            (Lang::Ja, "user_code") => "このコードを GitHub に入力",
            (Lang::En, "language") => "Language",
            (Lang::Ja, "language") => "言語",
            _ => key,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ja_sign_in() {
        assert_eq!(Lang::Ja.t("sign_in"), "GitHub でサインイン");
    }

    #[test]
    fn unknown_key_echoes() {
        assert_eq!(Lang::En.t("nope"), "nope");
    }
}
