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
            (Lang::En, "need_oauth_app") => {
                "Create an OAuth App (not a GitHub App) at https://github.com/settings/developers — enable Device Flow, then paste the Client ID into src/config.rs. gh cannot create OAuth Apps."
            }
            (Lang::Ja, "need_oauth_app") => {
                "GitHub App ではなく OAuth App が必要です。https://github.com/settings/developers で作成し、Device Flow を有効にして Client ID を src/config.rs に入れてください。OAuth App の作成は gh ではできません。"
            }
            (Lang::En, "select_issue") => "Select an issue",
            (Lang::Ja, "select_issue") => "Issue を選択",
            (Lang::En, "filter_hint") => "is:open title",
            (Lang::Ja, "filter_hint") => "is:open タイトル",
            (Lang::En, "opened") => "Opened",
            (Lang::Ja, "opened") => "Open",
            (Lang::En, "closed") => "Closed",
            (Lang::Ja, "closed") => "Closed",
            (Lang::En, "created") => "Created",
            (Lang::Ja, "created") => "作成",
            (Lang::En, "updated") => "Updated",
            (Lang::Ja, "updated") => "更新日時",
            (Lang::En, "asc") => "Asc",
            (Lang::Ja, "asc") => "昇順",
            (Lang::En, "desc") => "Desc",
            (Lang::Ja, "desc") => "降順",
            (Lang::En, "rate_remaining") => "Rate remaining",
            (Lang::Ja, "rate_remaining") => "レート残",
            (Lang::En, "related") => "Related",
            (Lang::Ja, "related") => "関連",
            (Lang::En, "rate_limited") => "GitHub rate limit — showing cache",
            (Lang::Ja, "rate_limited") => "レート制限 — キャッシュを表示",
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
