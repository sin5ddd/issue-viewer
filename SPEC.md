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
- The cache stores issue number, title, body, open/closed, parent, `created_at`, and GitHub `updated_at`. Comments are not stored.
- Refresh upserts issues whose GitHub `updated_at` is at or after the newest cached `updated_at` for that repository. A full replace runs only when that repository has no cached rows. Issues deleted on GitHub are not removed by an incremental refresh.
- Selecting an issue reads the cached body. The app does not call a per-issue GitHub detail API on select or refresh.
- On open, the app draws the SQLite tree immediately, then syncs from GitHub in the background when the network is available. The UI shows last-synced time. The user can refresh manually. Only one issue sync runs at a time.
- The OAuth token is not stored in the SQLite issue database in plaintext. It is kept in the OS keyring until the user signs out, so a restart does not ask for login again.
- Children nest under their parent. Deeper sub-issues nest further. Rows can expand and collapse.
- Each row shows at least issue number, title, and open/closed.
- Choosing a row opens that GitHub issue in the default browser.
- First release is read-only. The app does not create, edit, or close issues, and does not change parent/child links.
- Loading and failure states are visible in the UI.
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
