use chrono::{DateTime, Local, TimeZone, Utc};

pub fn format_rfc3339_local(raw: &str) -> String {
    if let Ok(dt) = DateTime::parse_from_rfc3339(raw) {
        return dt.with_timezone(&Local).format("%Y-%m-%d %H:%M %Z").to_string();
    }
    raw.to_string()
}

pub fn format_unix_local(raw: &str) -> String {
    let Ok(secs) = raw.parse::<i64>() else {
        return format_rfc3339_local(raw);
    };
    match Utc.timestamp_opt(secs, 0) {
        chrono::LocalResult::Single(dt) => dt
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M %Z")
            .to_string(),
        _ => raw.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_is_not_github_raw() {
        let s = format_rfc3339_local("2026-09-19T00:04:46Z");
        assert!(!s.contains('T'), "{s}");
        assert!(s.contains("2026-09-19"), "{s}");
    }

    #[test]
    fn unix_is_not_raw_digits_only() {
        let s = format_unix_local("1789795222");
        assert_ne!(s, "1789795222");
        assert!(s.contains('-'), "{s}");
    }
}
