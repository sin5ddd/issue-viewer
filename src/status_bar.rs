#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusBar {
    pub reloading: bool,
    pub relogin: bool,
    /// Translated notice, when `status` is a known i18n key (or contains `rate_limited`).
    pub notice_key: Option<&'static str>,
    /// Show `status` as-is. Mutually exclusive with `notice_key` and `relogin`.
    pub notice_raw: bool,
}

pub fn status_bar(reloading: bool, status: &str) -> StatusBar {
    let relogin = status == "auth required";
    let (notice_key, notice_raw) = if relogin || status.is_empty() {
        (None, false)
    } else if status.contains("rate_limited") {
        (Some("rate_limited"), false)
    } else if status == "need_oauth_app" {
        (Some("need_oauth_app"), false)
    } else {
        (None, true)
    };
    StatusBar {
        reloading,
        relogin,
        notice_key,
        notice_raw,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reloading_is_a_status_bar_item() {
        let bar = status_bar(true, "");
        assert!(bar.reloading);
        assert!(!bar.relogin);
    }

    #[test]
    fn auth_required_is_relogin_in_the_status_bar() {
        let bar = status_bar(false, "auth required");
        assert!(bar.relogin);
        assert!(!bar.reloading);
    }

    #[test]
    fn reload_and_relogin_can_show_together() {
        let bar = status_bar(true, "auth required");
        assert!(bar.reloading);
        assert!(bar.relogin);
    }

    #[test]
    fn other_status_keeps_its_i18n_key() {
        let limited = status_bar(false, "rate_limited");
        assert_eq!(limited.notice_key, Some("rate_limited"));
        assert!(!limited.notice_raw);
        assert!(!limited.relogin);

        let oauth = status_bar(false, "need_oauth_app");
        assert_eq!(oauth.notice_key, Some("need_oauth_app"));

        let raw = status_bar(false, "http: boom");
        assert!(raw.notice_raw);
        assert!(raw.notice_key.is_none());

        let wrapped = status_bar(false, "http: rate_limited");
        assert_eq!(wrapped.notice_key, Some("rate_limited"));
        assert!(!wrapped.notice_raw);
    }
}
