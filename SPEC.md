# Nested GitHub Issue Viewer — Specification

## 1. Purpose

GitHub’s native issue list shows sub-issues as a flat list, so parent/child order is hard to follow and it is easy to close a parent by mistake. GitHub Projects can show hierarchy, but every issue must be added to a project first.

This app is a lightweight desktop viewer: sign in with GitHub OAuth, pick a repository the user can access, and show that repository’s issues nested by GitHub’s own parent/child (sub-issue) relationships. Issues do not need to be in a GitHub Project.

## 2. Surfaces

| Name | Screen | Auth |
|------|--------|------|
| Sign in | Sign in with GitHub OAuth | GitHub OAuth |
| Repository picker | Choose one accessible repository | Same |
| Issue tree | Nested parent/child list for the selected repository | Same |

## 3. Functional requirements

- The user can sign in with a GitHub account via OAuth, and can sign out.
- After sign-in, the app lists repositories the user can access. The user picks one and the app shows that repository’s issues. The user can switch repository.
- Parent/child comes from GitHub sub-issues. Adding issues to GitHub Projects is not required.
- Issue tree data is stored in a local SQLite database so the UI does not hit GitHub on every view, nested parent/child rows are cheap to query, and a previously synced tree remains usable offline.
- The cache stores issue number, title, body, open/closed, parent, `created_at`, GitHub `updated_at`, labels (name and color), issue type (name and color), and issue field values (field name, value, and color when the value is a single-select option). Issue comments (id, author, body, created_at, updated_at, minimized) are stored per issue in SQLite. They are not loaded with the issue tree. Project fields are not stored.
- Refresh upserts issues whose GitHub `updated_at` is at or after the newest cached `updated_at` for that repository. A full replace runs only when that repository has no cached rows, or once after an upgrade that adds label, type, and field columns. Issues deleted on GitHub are not removed by an incremental refresh. If that upgrade fetch stops early because the rate limit is low, cached rows are kept and the full fetch is retried on the next refresh.
- Selecting an issue reads the cached body and does not refetch the issue. It shows cached comments immediately, and fetches that issue's comments only when none are cached or the cached issue updated_at differs.
- On open, the app draws the SQLite tree immediately, then syncs from GitHub in the background when the network is available. The UI shows last-synced time. The user can refresh manually. Only one issue sync runs at a time.
- The OAuth token is not stored in the SQLite issue database in plaintext. It is kept in the OS keyring until the user signs out, so a restart does not ask for login again.
- If the OAuth App has expiring user tokens enabled, the access token expires after eight hours. The app stores the device-flow refresh token in the same OS keyring entry and renews the access token without showing Sign in. A refresh does not use a client secret. After an upgrade from a build that stored only the access token, the user signs in once so a refresh token can be issued. Sign out still deletes the keyring entry. A failed refresh (revoked token, or a refresh token unused for six months) shows the re-login prompt and leaves the cached tree on screen.
- Children nest under their parent. Deeper sub-issues nest further. Rows can expand and collapse.
- Each row shows open/closed in a left column, to the left of the sub-issue indent rules. The collapse control, issue number, and title follow those rules, plus the issue type, up to two labels, and the first issue field when those are set. State, type, label, and field chips use a pale tint rather than a solid fill. Further labels and fields stay on the row tooltip.
- Choosing a row opens that GitHub issue in the default browser.
- The center pane shows the issue type, every label, and every issue field value from the cache, above the body. Issue fields are GitHub issue fields (sidebar metadata such as Priority), not GitHub Project fields. Comments hang below the body as cards in the same scroll area, oldest first.
- First release is read-only. The app does not create, edit, or close issues, and does not change parent/child links.
- Loading and failure states are visible in the bottom status bar, not under the toolbar. Reload progress and a re-login prompt (expired or rejected token) stay there while the cached tree remains on screen.
- UI chrome is available in Japanese and English from the first release. Default follows the OS locale when it is Japanese or English; otherwise English. The user can switch language in the app.
- Japanese and other CJK in issue titles must render as real glyphs, not tofu. The app sets an explicit UI font from the first release: bundled **Noto Sans JP** (SIL OFL), registered in egui as the proportional family (with monospace kept for numbers/IDs if needed).

## 4. Constraints / assumptions

- Desktop application (native window). Not a TUI. Not a browser-only app.
- UI toolkit is **egui** (winit). Not Tauri and not Electron; those stay closer to a web-runtime footprint.
- Implementation language is Rust.
- Target is GitHub.com. GitHub Enterprise is out of scope.
- Data source of truth is GitHub issues and sub-issues. SQLite is a local cache, not a second issue tracker. GitHub Projects hierarchy view is not used.
- Prefer a small footprint. SQLite is the local store (one file, no extra server).
- The project will be published as open source. All documentation and source comments must be in English. UI strings may be Japanese or English.
- Nesting depth and children per parent follow GitHub’s sub-issue limits.
- Do not rely on egui’s default fonts for body text; they do not cover Japanese.
