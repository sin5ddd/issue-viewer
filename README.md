# Nested GitHub Issue Viewer

Desktop viewer for GitHub sub-issues. Nested parent/child comes from GitHub's own sub-issue links, cached in local SQLite. Issues do not need to be in a GitHub Project.

See [SPEC.md](SPEC.md).

## Features (MVP)

- Sign in with GitHub OAuth Device Flow
- Pick a repository you can access
- Show **open** issues nested by parent/child
- Draw the SQLite cache immediately, then sync in the background
- UI in English and Japanese
- Bundled Noto Sans JP (SIL Open Font License)

Read-only: the app does not create, edit, or close issues.

## Setup

1. Create a GitHub OAuth App: https://github.com/settings/applications/new  
   Homepage URL can be `http://127.0.0.1`. Callback URL is unused for Device Flow.
2. Enable **Device Flow** on the app.
3. Request the `repo` scope (needed for private repositories).
4. Paste the public Client ID into `src/config.rs` (`GITHUB_CLIENT_ID`). It is not a secret.
5. Run:

```bash
cargo run
```

## Data

- Access token: OS keyring (`issue-viewer` / `github`), never stored in SQLite
- Issue cache: `cache.sqlite` under the user data directory (`issue-viewer`)

## Font

`assets/fonts/NotoSansJP-Regular.ttf` is Noto Sans JP, SIL OFL.

## Development

```bash
cargo test
```
