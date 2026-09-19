use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: Option<String>,
    pub error: Option<String>,
    pub interval: Option<u64>,
}

pub fn parse_device_code(json: &str) -> Result<DeviceCode, serde_json::Error> {
    serde_json::from_str(json)
}

pub fn is_placeholder_client_id(id: &str) -> bool {
    id.is_empty() || id == "REPLACE_ME"
}

/// Parse `gh auth token` stdout. Never log the return value.
pub fn parse_gh_auth_token_stdout(stdout: &str) -> Option<String> {
    let line = stdout.lines().find(|l| !l.trim().is_empty())?.trim();
    if line.starts_with("gho_")
        || line.starts_with("ghp_")
        || line.starts_with("github_pat_")
        || line.starts_with("ghu_")
    {
        Some(line.to_string())
    } else {
        None
    }
}

pub fn try_gh_cli_token() -> Option<String> {
    let mut cmd = std::process::Command::new("gh");
    cmd.args(["auth", "token"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    parse_gh_auth_token_stdout(&String::from_utf8_lossy(&output.stdout))
}

pub fn request_device_code(
    http: &reqwest::blocking::Client,
    client_id: &str,
    scope: &str,
) -> Result<DeviceCode, reqwest::Error> {
    let resp = http
        .post("https://github.com/login/device/code")
        .header("Accept", "application/json")
        .form(&[("client_id", client_id), ("scope", scope)])
        .send()?;
    resp.json()
}

pub fn poll_token(
    http: &reqwest::blocking::Client,
    client_id: &str,
    device_code: &str,
) -> Result<TokenResponse, reqwest::Error> {
    let resp = http
        .post("https://github.com/login/oauth/access_token")
        .header("Accept", "application/json")
        .form(&[
            ("client_id", client_id),
            ("device_code", device_code),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ])
        .send()?;
    resp.json()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_device_payload() {
        let d = parse_device_code(
            r#"{"device_code":"abc","user_code":"WDJB-MJHT","verification_uri":"https://github.com/login/device","expires_in":900,"interval":5}"#,
        )
        .unwrap();
        assert_eq!(d.user_code, "WDJB-MJHT");
        assert_eq!(d.interval, 5);
    }

    #[test]
    fn pending_has_no_token() {
        let t: TokenResponse =
            serde_json::from_str(r#"{"error":"authorization_pending"}"#).unwrap();
        assert!(t.access_token.is_none());
        assert_eq!(t.error.as_deref(), Some("authorization_pending"));
    }

    #[test]
    fn placeholder_client_id() {
        assert!(is_placeholder_client_id("REPLACE_ME"));
        assert!(is_placeholder_client_id(""));
        assert!(!is_placeholder_client_id("Iv1.abc"));
    }

    #[test]
    fn parses_gh_auth_token_stdout() {
        assert_eq!(
            parse_gh_auth_token_stdout("gho_exampletoken\n").as_deref(),
            Some("gho_exampletoken")
        );
        assert!(parse_gh_auth_token_stdout("error: not logged in\n").is_none());
    }
}
