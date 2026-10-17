use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use crate::auth::{
    is_placeholder_client_id, poll_token, refresh_access_token, request_device_code,
    try_gh_cli_token,
};
use crate::config::{GITHUB_CLIENT_ID, GITHUB_SCOPE};
use crate::db::{Cache, UiLayout};
use crate::filter::{self, SortDir, SortKey};
use crate::github::{GitHubClient, LiveClient, RepoRef};
use crate::i18n::Lang;
use crate::model::{IssueComment, IssueRow};
use crate::status_bar::status_bar;
use crate::sync::sync_repo;
use crate::token::{
    KeyringTokenStore, StoredAuth, TokenStore, UnauthorizedAction, decode_stored_auth,
    encode_stored_auth, expires_at_from_ttl, on_unauthorized, should_refresh,
};
use crate::tree::{TreeNode, build_tree, focus_tree};

enum UiMsg {
    DeviceCode {
        user_code: String,
        verification_uri: String,
    },
    LoggedIn {
        access_token: String,
        refresh_token: Option<String>,
        expires_in: Option<u64>,
    },
    Refreshed {
        access_token: String,
        refresh_token: Option<String>,
        expires_in: Option<u64>,
    },
    RefreshFailed,
    Repos(Vec<RepoRef>),
    SyncDone {
        remaining: Option<u32>,
    },
    Error(String),
    CommentsReady {
        owner: String,
        repo: String,
        number: u64,
        rate_remaining: Option<u32>,
    },
    CommentsFailed {
        owner: String,
        repo: String,
        number: u64,
        error: String,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pane {
    Left,
    Right,
}

pub struct IssueViewerApp {
    lang: Lang,
    tokens: KeyringTokenStore,
    token_value: Option<String>,
    refresh_token: Option<String>,
    access_expires_at: Option<i64>,
    refreshing: bool,
    user_code: Option<String>,
    verification_uri: Option<String>,
    repos: Vec<RepoRef>,
    selected: Option<(String, String)>,
    selected_issue: Option<u64>,
    selection_pane: Pane,
    tree: Vec<TreeNode>,
    all_rows: Vec<IssueRow>,
    query: String,
    show_open: bool,
    show_closed: bool,
    sort_key: SortKey,
    sort_dir: SortDir,
    rate_remaining: Option<u32>,
    last_synced: Option<String>,
    status: String,
    loading: bool,
    syncing: bool,
    comments: Vec<IssueComment>,
    comments_for: Option<(String, String, u64)>,
    comments_issue_updated_at: Option<String>,
    comments_truncated: bool,
    comments_loading: bool,
    comments_inflight: Option<(String, String, u64)>,
    comments_error: String,
    layout: UiLayout,
    md_cache: RefCell<egui_commonmark::CommonMarkCache>,
    tx: Sender<UiMsg>,
    rx: Receiver<UiMsg>,
}

impl IssueViewerApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        crate::fonts::install(&cc.egui_ctx);
        egui_extras::install_image_loaders(&cc.egui_ctx);
        let (tx, rx) = mpsc::channel();
        let tokens = KeyringTokenStore;
        let stored = tokens.load().map(|raw| decode_stored_auth(&raw));
        let token_value = stored.as_ref().map(|s| s.access_token.clone());
        let refresh_token = stored.as_ref().and_then(|s| s.refresh_token.clone());
        let access_expires_at = stored.as_ref().and_then(|s| s.access_expires_at);
        let mut app = Self {
            lang: Lang::detect(),
            tokens,
            token_value: token_value.clone(),
            refresh_token,
            access_expires_at,
            refreshing: false,
            user_code: None,
            verification_uri: None,
            repos: Vec::new(),
            selected: None,
            selected_issue: None,
            selection_pane: Pane::Left,
            tree: Vec::new(),
            all_rows: Vec::new(),
            query: String::new(),
            show_open: true,
            show_closed: false,
            sort_key: SortKey::Updated,
            sort_dir: SortDir::Desc,
            rate_remaining: None,
            last_synced: None,
            status: String::new(),
            loading: false,
            syncing: false,
            comments: Vec::new(),
            comments_for: None,
            comments_issue_updated_at: None,
            comments_truncated: false,
            comments_loading: false,
            comments_inflight: None,
            comments_error: String::new(),
            layout: Cache::open(&Self::cache_path())
                .ok()
                .and_then(|c| c.ui_layout().ok())
                .unwrap_or_default(),
            md_cache: RefCell::new(egui_commonmark::CommonMarkCache::default()),
            tx,
            rx,
        };
        if let Some(token) = token_value {
            let expired = stored
                .as_ref()
                .is_some_and(|auth| should_refresh(auth, chrono::Utc::now().timestamp()));
            if expired {
                app.spawn_refresh();
            } else {
                if let Ok(cache) = Cache::open(&Self::cache_path())
                    && let Ok(Some((owner, repo, issue))) = cache.last_repo()
                {
                    app.selected = Some((owner.clone(), repo.clone()));
                    app.selected_issue = issue;
                    app.reload_tree();
                    app.ensure_comments();
                    app.spawn_sync(token.clone(), owner, repo);
                }
                app.spawn_list_repos(token);
            }
        }
        app
    }

    pub fn cache_path() -> PathBuf {
        let mut dir = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
        dir.push("issue-viewer");
        dir.push("cache.sqlite");
        dir
    }

    fn persist_session(&self) {
        let Some((owner, repo)) = &self.selected else {
            return;
        };
        if let Ok(cache) = Cache::open(&Self::cache_path()) {
            let _ = cache.set_last_repo(owner, repo, self.selected_issue);
        }
    }

    fn persist_layout(&self) {
        if let Ok(cache) = Cache::open(&Self::cache_path()) {
            let _ = cache.set_ui_layout(&self.layout);
        }
    }

    fn spawn_login(&self) {
        let tx = self.tx.clone();
        thread::spawn(move || {
            if is_placeholder_client_id(GITHUB_CLIENT_ID) {
                if let Some(token) = try_gh_cli_token() {
                    let _ = tx.send(UiMsg::LoggedIn {
                        access_token: token,
                        refresh_token: None,
                        expires_in: None,
                    });
                    return;
                }
                let _ = tx.send(UiMsg::Error("need_oauth_app".into()));
                return;
            }
            let http = reqwest::blocking::Client::new();
            let codes = match request_device_code(&http, GITHUB_CLIENT_ID, GITHUB_SCOPE) {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send(UiMsg::Error(e.to_string()));
                    return;
                }
            };
            let _ = webbrowser::open(&codes.verification_uri);
            let _ = tx.send(UiMsg::DeviceCode {
                user_code: codes.user_code.clone(),
                verification_uri: codes.verification_uri.clone(),
            });
            let mut interval = Duration::from_secs(codes.interval.max(1));
            loop {
                thread::sleep(interval);
                let resp = match poll_token(&http, GITHUB_CLIENT_ID, &codes.device_code) {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = tx.send(UiMsg::Error(e.to_string()));
                        return;
                    }
                };
                if let Some(token) = resp.access_token {
                    let _ = tx.send(UiMsg::LoggedIn {
                        access_token: token,
                        refresh_token: resp.refresh_token,
                        expires_in: resp.expires_in,
                    });
                    return;
                }
                match resp.error.as_deref() {
                    Some("authorization_pending") => {}
                    Some("slow_down") => {
                        if let Some(extra) = resp.interval {
                            interval = Duration::from_secs(extra.max(1));
                        } else {
                            interval += Duration::from_secs(5);
                        }
                    }
                    Some(other) => {
                        let _ = tx.send(UiMsg::Error(other.to_string()));
                        return;
                    }
                    None => {}
                }
            }
        });
    }

    fn spawn_list_repos(&mut self, token: String) {
        self.loading = true;
        let tx = self.tx.clone();
        thread::spawn(move || {
            let client = LiveClient::new(token);
            match client.list_repos() {
                Ok(repos) => {
                    let _ = tx.send(UiMsg::Repos(repos));
                }
                Err(e) => {
                    let _ = tx.send(UiMsg::Error(e.to_string()));
                }
            }
        });
    }

    fn spawn_sync(&mut self, token: String, owner: String, repo: String) {
        if self.syncing {
            return;
        }
        self.syncing = true;
        self.loading = true;
        let tx = self.tx.clone();
        thread::spawn(move || {
            let path = IssueViewerApp::cache_path();
            let cache = match Cache::open(&path) {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send(UiMsg::Error(e.to_string()));
                    return;
                }
            };
            let client = LiveClient::new(token);
            match sync_repo(&cache, &client, &owner, &repo) {
                Ok(remaining) => {
                    let _ = tx.send(UiMsg::SyncDone { remaining });
                }
                Err(e) => {
                    let _ = tx.send(UiMsg::Error(e.to_string()));
                }
            }
        });
    }

    fn rebuild_visible(&mut self) {
        let filtered = filter::apply(
            &self.all_rows,
            &self.query,
            self.show_open,
            self.show_closed,
            self.sort_key,
            self.sort_dir,
        );
        self.tree = build_tree(&filtered);
    }

    fn reload_tree(&mut self) {
        let Some((owner, repo)) = self.selected.clone() else {
            return;
        };
        match Cache::open(&Self::cache_path()) {
            Ok(cache) => {
                match cache.list_issues(&owner, &repo) {
                    Ok(rows) => {
                        if let Some(n) = self.selected_issue
                            && !rows.iter().any(|r| r.number == n)
                        {
                            self.selected_issue = None;
                        }
                        self.all_rows = rows;
                        self.rebuild_visible();
                    }
                    Err(e) => self.status = e.to_string(),
                }
                self.last_synced = cache.last_synced(&owner, &repo).ok().flatten();
            }
            Err(e) => self.status = e.to_string(),
        }
    }

    fn clear_comments(&mut self) {
        self.comments.clear();
        self.comments_for = None;
        self.comments_issue_updated_at = None;
        self.comments_truncated = false;
        self.comments_loading = false;
        self.comments_error.clear();
    }

    fn ensure_comments(&mut self) {
        let Some((owner, repo)) = self.selected.clone() else {
            self.clear_comments();
            return;
        };
        let Some(number) = self.selected_issue else {
            self.clear_comments();
            return;
        };
        let key = (owner.clone(), repo.clone(), number);
        if self.comments_for.as_ref() != Some(&key) {
            self.comments.clear();
            self.comments_error.clear();
            self.comments_truncated = false;
            self.comments_issue_updated_at = None;
            self.comments_for = Some(key.clone());
        }
        match Cache::open(&Self::cache_path()) {
            Ok(cache) => match cache.list_comments(&owner, &repo, number) {
                Ok(Some(cached)) => {
                    self.comments = cached.comments;
                    self.comments_truncated = cached.truncated;
                    self.comments_issue_updated_at = Some(cached.issue_updated_at);
                }
                Ok(None) => {}
                Err(e) => self.comments_error = e.to_string(),
            },
            Err(e) => self.comments_error = e.to_string(),
        }
        let Some(updated_at) = self.selected_row().map(|row| row.updated_at.clone()) else {
            self.comments_loading = false;
            return;
        };
        if self.comments_issue_updated_at.as_deref() == Some(updated_at.as_str()) {
            self.comments_loading = false;
            return;
        }
        let Some(token) = self.token_value.clone() else {
            self.comments_loading = false;
            if self.comments.is_empty() && self.comments_error.is_empty() {
                self.comments_error = "offline".into();
            }
            return;
        };
        if self.comments_inflight.as_ref() == Some(&key) {
            self.comments_loading = true;
            return;
        }
        self.spawn_comment_fetch(token, owner, repo, number, updated_at);
    }

    fn spawn_comment_fetch(
        &mut self,
        token: String,
        owner: String,
        repo: String,
        number: u64,
        issue_updated_at: String,
    ) {
        self.comments_loading = true;
        self.comments_inflight = Some((owner.clone(), repo.clone(), number));
        let tx = self.tx.clone();
        thread::spawn(move || {
            let client = LiveClient::new(token);
            match client.fetch_issue_comments(&owner, &repo, number) {
                Ok(page) => match Cache::open(&IssueViewerApp::cache_path()) {
                    Ok(cache) => match cache.replace_comments(
                        &owner,
                        &repo,
                        number,
                        &issue_updated_at,
                        page.truncated,
                        &page.comments,
                    ) {
                        Ok(()) => {
                            let _ = tx.send(UiMsg::CommentsReady {
                                owner,
                                repo,
                                number,
                                rate_remaining: page.rate_remaining,
                            });
                        }
                        Err(e) => {
                            let _ = tx.send(UiMsg::CommentsFailed {
                                owner,
                                repo,
                                number,
                                error: e.to_string(),
                            });
                        }
                    },
                    Err(e) => {
                        let _ = tx.send(UiMsg::CommentsFailed {
                            owner,
                            repo,
                            number,
                            error: e.to_string(),
                        });
                    }
                },
                Err(crate::github::GitHubError::Auth) => {
                    let _ = tx.send(UiMsg::Error("auth required".into()));
                }
                Err(e) => {
                    let _ = tx.send(UiMsg::CommentsFailed {
                        owner,
                        repo,
                        number,
                        error: e.to_string(),
                    });
                }
            }
        });
    }

    fn pump_messages(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                UiMsg::DeviceCode {
                    user_code,
                    verification_uri,
                } => {
                    self.user_code = Some(user_code);
                    self.verification_uri = Some(verification_uri);
                    self.loading = false;
                }
                UiMsg::LoggedIn {
                    access_token,
                    refresh_token,
                    expires_in,
                } => {
                    self.store_session(access_token, refresh_token, expires_in);
                    self.user_code = None;
                    if self.status == "auth required" {
                        self.status.clear();
                    }
                    if let Some(token) = self.token_value.clone() {
                        self.spawn_list_repos(token);
                    }
                }
                UiMsg::Repos(repos) => {
                    self.repos = repos;
                    if !self.syncing {
                        self.loading = false;
                    }
                }
                UiMsg::SyncDone { remaining } => {
                    self.syncing = false;
                    self.loading = false;
                    self.rate_remaining = remaining;
                    self.status.clear();
                    self.reload_tree();
                    self.ensure_comments();
                }
                UiMsg::Refreshed {
                    access_token,
                    refresh_token,
                    expires_in,
                } => {
                    self.refreshing = false;
                    self.store_session(access_token, refresh_token, expires_in);
                    self.status.clear();
                    self.retry_after_refresh();
                }
                UiMsg::RefreshFailed => {
                    self.refreshing = false;
                    self.loading = false;
                    self.syncing = false;
                    self.status = "auth required".into();
                }
                UiMsg::Error(e) => {
                    self.syncing = false;
                    self.loading = false;
                    if e == "auth required" {
                        self.comments_loading = false;
                        self.comments_inflight = None;
                        match on_unauthorized(self.has_refresh_token(), self.refreshing) {
                            UnauthorizedAction::StartRefresh => self.spawn_refresh(),
                            UnauthorizedAction::IgnoreStale => {}
                            UnauthorizedAction::Relogin => {
                                self.refreshing = false;
                                self.status = e;
                            }
                        }
                    } else {
                        self.refreshing = false;
                        self.status = e;
                    }
                }
                UiMsg::CommentsReady {
                    owner,
                    repo,
                    number,
                    rate_remaining,
                } => {
                    if rate_remaining.is_some() {
                        self.rate_remaining = rate_remaining;
                    }
                    let key = (owner.clone(), repo.clone(), number);
                    if self.comments_inflight.as_ref() == Some(&key) {
                        self.comments_inflight = None;
                    }
                    if self
                        .selected
                        .as_ref()
                        .is_some_and(|(o, r)| o == &owner && r == &repo)
                        && self.selected_issue == Some(number)
                    {
                        match Cache::open(&Self::cache_path())
                            .and_then(|cache| cache.list_comments(&owner, &repo, number))
                        {
                            Ok(Some(cached)) => {
                                self.comments = cached.comments;
                                self.comments_truncated = cached.truncated;
                                self.comments_issue_updated_at = Some(cached.issue_updated_at);
                                self.comments_for = Some(key);
                                self.comments_loading = false;
                                self.comments_error.clear();
                            }
                            Ok(None) => {
                                self.comments.clear();
                                self.comments_truncated = false;
                                self.comments_issue_updated_at = None;
                                self.comments_for = Some(key);
                                self.comments_loading = false;
                                self.comments_error.clear();
                            }
                            Err(err) => {
                                self.comments_loading = false;
                                self.comments_error = err.to_string();
                            }
                        }
                    }
                }
                UiMsg::CommentsFailed {
                    owner,
                    repo,
                    number,
                    error,
                } => {
                    let key = (owner, repo, number);
                    if self.comments_inflight.as_ref() == Some(&key) {
                        self.comments_inflight = None;
                    }
                    if self
                        .selected
                        .as_ref()
                        .is_some_and(|(o, r)| o == &key.0 && r == &key.1)
                        && self.selected_issue == Some(key.2)
                    {
                        self.comments_loading = false;
                        self.comments_error = error;
                    }
                }
            }
        }
    }

    fn store_session(
        &mut self,
        access_token: String,
        refresh_token: Option<String>,
        expires_in: Option<u64>,
    ) {
        let access_expires_at = expires_at_from_ttl(chrono::Utc::now().timestamp(), expires_in);
        let auth = StoredAuth {
            access_token: access_token.clone(),
            refresh_token,
            access_expires_at,
        };
        self.tokens.save(&encode_stored_auth(&auth));
        self.token_value = Some(access_token);
        self.refresh_token = auth.refresh_token;
        self.access_expires_at = auth.access_expires_at;
    }

    fn has_refresh_token(&self) -> bool {
        self.refresh_token.as_deref().is_some_and(|s| !s.is_empty())
    }

    fn spawn_refresh(&mut self) {
        let Some(refresh_token) = self.refresh_token.clone().filter(|s| !s.is_empty()) else {
            self.status = "auth required".into();
            return;
        };
        if self.refreshing {
            return;
        }
        self.refreshing = true;
        self.loading = true;
        self.status.clear();
        let tx = self.tx.clone();
        thread::spawn(move || {
            let http = reqwest::blocking::Client::new();
            match refresh_access_token(&http, GITHUB_CLIENT_ID, &refresh_token) {
                Ok(resp) if resp.access_token.is_some() => {
                    let _ = tx.send(UiMsg::Refreshed {
                        access_token: resp.access_token.unwrap(),
                        refresh_token: resp.refresh_token,
                        expires_in: resp.expires_in,
                    });
                }
                Ok(_) => {
                    let _ = tx.send(UiMsg::RefreshFailed);
                }
                Err(e) => {
                    let _ = tx.send(UiMsg::Error(e.to_string()));
                }
            }
        });
    }

    fn retry_after_refresh(&mut self) {
        let Some(token) = self.token_value.clone() else {
            return;
        };
        if let Some((owner, repo)) = self.selected.clone() {
            self.spawn_sync(token.clone(), owner, repo);
        }
        self.spawn_list_repos(token);
    }

    fn show_tree(
        ui: &mut eframe::egui::Ui,
        lang: Lang,
        nodes: &[TreeNode],
        selected: Option<u64>,
        clicked: &mut Option<u64>,
        id_ns: u64,
        active: bool,
    ) {
        ui.style_mut().wrap_mode = Some(eframe::egui::TextWrapMode::Truncate);
        ui.style_mut().interaction.selectable_labels = false;
        ui.set_min_width(0.0);
        ui.with_layout(
            eframe::egui::Layout::top_down(eframe::egui::Align::LEFT).with_cross_justify(true),
            |ui| {
                let gutter = Self::state_gutter(ui);
                Self::show_tree_rows(ui, lang, nodes, selected, clicked, id_ns, active, 0, gutter);
            },
        );
    }

    fn show_tree_rows(
        ui: &mut eframe::egui::Ui,
        lang: Lang,
        nodes: &[TreeNode],
        selected: Option<u64>,
        clicked: &mut Option<u64>,
        id_ns: u64,
        active: bool,
        depth: usize,
        gutter: f32,
    ) {
        ui.spacing_mut().item_spacing.y = 2.0;
        let indent = ui.spacing().indent;
        for node in nodes {
            let is_sel = selected == Some(node.issue.number);
            let id = ui.make_persistent_id((id_ns, node.issue.number));
            let has_children = !node.children.is_empty();
            let mut open = ui.data_mut(|d| d.get_persisted(id).unwrap_or(true));
            let height = ui.spacing().interact_size.y.max(22.0);
            let (rect, row_resp) = ui.allocate_exact_size(
                eframe::egui::vec2(ui.available_width(), height),
                eframe::egui::Sense::click(),
            );
            if row_resp.hovered() {
                ui.ctx()
                    .set_cursor_icon(eframe::egui::CursorIcon::PointingHand);
            }
            if is_sel {
                let fill = if active {
                    ui.visuals().selection.bg_fill
                } else {
                    crate::theme::THEME.selection_inactive_bg
                };
                ui.painter().rect_filled(rect, 3.0, fill);
            }
            let state_rect = eframe::egui::Rect::from_min_size(
                rect.min,
                eframe::egui::vec2(gutter, rect.height()),
            );
            ui.allocate_new_ui(
                eframe::egui::UiBuilder::new().max_rect(state_rect).layout(
                    eframe::egui::Layout::left_to_right(eframe::egui::Align::Center),
                ),
                |ui| {
                    Self::state_pill(ui, node.issue.state);
                },
            );
            let body_rect = eframe::egui::Rect::from_min_max(
                eframe::egui::pos2(rect.min.x + gutter, rect.min.y),
                rect.max,
            );
            ui.allocate_new_ui(
                eframe::egui::UiBuilder::new().max_rect(body_rect).layout(
                    eframe::egui::Layout::left_to_right(eframe::egui::Align::Center),
                ),
                |ui| {
                    ui.add_space(depth as f32 * indent);
                    if has_children {
                        if Self::disclosure(ui, open) {
                            open = !open;
                        }
                    } else {
                        ui.add_space(14.0);
                    }
                    ui.add(
                        eframe::egui::Label::new(format!("#{}", node.issue.number))
                            .selectable(false),
                    );
                    Self::tree_meta(ui, &node.issue);
                    ui.add(
                        eframe::egui::Label::new(&node.issue.title)
                            .truncate()
                            .halign(eframe::egui::Align::LEFT)
                            .selectable(false),
                    );
                },
            );
            let tip = Self::meta_tooltip(lang, &node.issue);
            if row_resp.clicked() {
                *clicked = Some(node.issue.number);
            }
            if !tip.is_empty() {
                row_resp.on_hover_text(tip);
            }
            ui.data_mut(|d| d.insert_persisted(id, open));
            if has_children && open {
                let top = ui.cursor().min.y;
                let line_x = rect.min.x + gutter + (depth as f32 + 0.5) * indent;
                Self::show_tree_rows(
                    ui,
                    lang,
                    &node.children,
                    selected,
                    clicked,
                    id_ns,
                    active,
                    depth + 1,
                    gutter,
                );
                if ui.visuals().indent_has_left_vline {
                    let bottom = ui.cursor().min.y - ui.spacing().item_spacing.y - 2.0;
                    if bottom > top {
                        let stroke = ui.visuals().widgets.noninteractive.bg_stroke;
                        ui.painter().line_segment(
                            [
                                eframe::egui::pos2(line_x, top),
                                eframe::egui::pos2(line_x, bottom),
                            ],
                            stroke,
                        );
                    }
                }
            }
        }
    }

    fn disclosure(ui: &mut eframe::egui::Ui, open: bool) -> bool {
        let size = eframe::egui::vec2(14.0, 14.0);
        let (rect, resp) = ui.allocate_exact_size(size, eframe::egui::Sense::click());
        let c = rect.center();
        let color = ui.visuals().text_color();
        let pts = if open {
            [
                eframe::egui::pos2(c.x - 4.0, c.y - 2.0),
                eframe::egui::pos2(c.x + 4.0, c.y - 2.0),
                eframe::egui::pos2(c.x, c.y + 3.5),
            ]
        } else {
            [
                eframe::egui::pos2(c.x - 2.0, c.y - 4.0),
                eframe::egui::pos2(c.x + 3.5, c.y),
                eframe::egui::pos2(c.x - 2.0, c.y + 4.0),
            ]
        };
        ui.painter().add(eframe::egui::Shape::convex_polygon(
            pts.to_vec(),
            color,
            eframe::egui::Stroke::NONE,
        ));
        resp.clicked()
    }

    fn state_gutter(ui: &eframe::egui::Ui) -> f32 {
        let font = eframe::egui::TextStyle::Small.resolve(ui.style());
        let text_w = ui.fonts(|fonts| {
            fonts
                .layout_no_wrap(
                    "Closed".to_owned(),
                    font,
                    eframe::egui::Color32::PLACEHOLDER,
                )
                .size()
                .x
        });
        // Pill padding plus a gap so the indent rule starts clear of the chip.
        text_w + 20.0
    }

    fn state_pill(ui: &mut eframe::egui::Ui, state: crate::model::IssueState) {
        let theme = crate::theme::THEME;
        match state {
            crate::model::IssueState::Open => {
                Self::pill(ui, "Open", theme.state_open_fg, theme.state_open_bg);
            }
            crate::model::IssueState::Closed => {
                Self::pill(ui, "Closed", theme.state_closed_fg, theme.state_closed_bg);
            }
        }
    }

    fn pill(
        ui: &mut eframe::egui::Ui,
        text: &str,
        fg: eframe::egui::Color32,
        bg: eframe::egui::Color32,
    ) {
        eframe::egui::Frame::new()
            .fill(bg)
            .corner_radius(10.0)
            .inner_margin(eframe::egui::Margin::symmetric(6, 1))
            .show(ui, |ui| {
                ui.add(
                    eframe::egui::Label::new(eframe::egui::RichText::new(text).color(fg).small())
                        .selectable(false),
                );
            });
    }

    fn colored_pill(ui: &mut eframe::egui::Ui, text: &str, color: &str) {
        let (fg, bg) = crate::theme::chip_colors(color);
        Self::pill(ui, text, fg, bg);
    }

    fn tree_meta(ui: &mut eframe::egui::Ui, row: &IssueRow) {
        if let Some(kind) = &row.issue_type {
            Self::colored_pill(ui, &kind.name, &kind.color);
        }
        let shown_labels = row.labels.len().min(2);
        for label in row.labels.iter().take(shown_labels) {
            Self::colored_pill(ui, &label.name, &label.color);
        }
        if let Some(field) = row.fields.first() {
            let text = format!("{}: {}", field.name, Self::clip(&field.value, 18));
            if let Some(color) = field.color.as_deref().filter(|c| !c.is_empty()) {
                Self::colored_pill(ui, &text, color);
            } else {
                let theme = crate::theme::THEME;
                Self::pill(ui, &text, theme.chip_fg, theme.chip_bg);
            }
        }
        let hidden = row.labels.len().saturating_sub(shown_labels)
            + row
                .fields
                .len()
                .saturating_sub(usize::from(!row.fields.is_empty()));
        if hidden > 0 {
            ui.weak(format!("+{hidden}"));
        }
    }

    fn meta_tooltip(lang: Lang, row: &IssueRow) -> String {
        let mut lines = Vec::new();
        if let Some(kind) = &row.issue_type {
            lines.push(format!("{}: {}", lang.t("type"), kind.name));
        }
        if !row.labels.is_empty() {
            let names = row
                .labels
                .iter()
                .map(|l| l.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!("{}: {}", lang.t("labels"), names));
        }
        for field in &row.fields {
            lines.push(format!("{}: {}", field.name, field.value));
        }
        lines.join("\n")
    }

    fn show_type_and_labels(ui: &mut eframe::egui::Ui, lang: Lang, row: &IssueRow) {
        if row.issue_type.is_none() && row.labels.is_empty() {
            return;
        }
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if let Some(kind) = &row.issue_type {
                ui.weak(lang.t("type"));
                Self::colored_pill(ui, &kind.name, &kind.color);
            }
            if !row.labels.is_empty() {
                ui.weak(lang.t("labels"));
                for label in &row.labels {
                    Self::colored_pill(ui, &label.name, &label.color);
                }
            }
        });
    }

    fn show_fields(ui: &mut eframe::egui::Ui, lang: Lang, row: &IssueRow) {
        if row.fields.is_empty() {
            return;
        }
        ui.weak(lang.t("fields"));
        eframe::egui::Grid::new(("issue_fields", row.number))
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                for field in &row.fields {
                    ui.weak(&field.name);
                    if let Some(color) = field.color.as_deref().filter(|c| !c.is_empty()) {
                        Self::colored_pill(ui, &field.value, color);
                    } else {
                        ui.add(
                            eframe::egui::Label::new(&field.value)
                                .wrap_mode(eframe::egui::TextWrapMode::Wrap),
                        );
                    }
                    ui.end_row();
                }
            });
        ui.add_space(6.0);
    }

    fn clip(s: &str, n: usize) -> String {
        let mut chars = s.chars();
        let head: String = chars.by_ref().take(n).collect();
        if chars.next().is_some() {
            format!("{head}…")
        } else {
            head
        }
    }

    fn comment_error_text(&self) -> String {
        let error = &self.comments_error;
        if error.contains("rate_limited") {
            self.lang.t("rate_limited").to_string()
        } else if error == "auth required" {
            self.lang.t("relogin_required").to_string()
        } else if error == "offline" {
            self.lang.t("offline").to_string()
        } else {
            error.clone()
        }
    }

    fn show_comments_section(
        &self,
        ui: &mut eframe::egui::Ui,
        owner: &str,
        repo: &str,
        number: u64,
    ) {
        let shown = self
            .comments_for
            .as_ref()
            .is_some_and(|(o, r, n)| o == owner && r == repo && *n == number);
        if !shown {
            return;
        }
        ui.add_space(16.0);
        let count = self.comments.len();
        let loading_empty = self.comments_loading && self.comments.is_empty();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.label(eframe::egui::RichText::new(self.lang.t("comments")).strong());
            if !loading_empty {
                let label = if self.comments_truncated {
                    format!("{count}+")
                } else {
                    count.to_string()
                };
                ui.weak(label);
            }
        });
        if loading_empty {
            ui.weak(self.lang.t("loading_comments"));
            return;
        }
        if self.comments_loading {
            ui.weak(self.lang.t("loading_comments"));
        }
        if self.comments.is_empty() && !self.comments_error.is_empty() {
            ui.weak(self.comment_error_text());
            return;
        }
        if self.comments.is_empty() && self.comments_error.is_empty() && !self.comments_loading {
            ui.weak(self.lang.t("no_comments"));
            return;
        }
        let mut cache = self.md_cache.borrow_mut();
        for comment in &self.comments {
            Self::show_comment_card(ui, self.lang, comment, &mut cache);
        }
        if self.comments_truncated {
            ui.weak(self.lang.t("comments_truncated"));
        }
    }

    fn show_comment_card(
        ui: &mut eframe::egui::Ui,
        lang: Lang,
        comment: &IssueComment,
        cache: &mut egui_commonmark::CommonMarkCache,
    ) {
        let rail_x = ui.cursor().min.x + 7.0;
        let top = ui.cursor().min.y;
        let stroke = ui.visuals().widgets.noninteractive.bg_stroke;
        ui.painter().line_segment(
            [
                eframe::egui::pos2(rail_x, top),
                eframe::egui::pos2(rail_x, top + 10.0),
            ],
            stroke,
        );
        ui.add_space(10.0);
        let width = (ui.available_width() - 18.0).max(120.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.add_space(18.0);
            eframe::egui::Frame::new()
                .fill(crate::theme::THEME.chip_bg)
                .stroke(stroke)
                .corner_radius(8.0)
                .inner_margin(eframe::egui::Margin::symmetric(10, 10))
                .show(ui, |ui| {
                    ui.set_min_width(width);
                    ui.set_max_width(width);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        if comment.author.is_empty() {
                            ui.strong(lang.t("deleted_user"));
                        } else {
                            ui.strong(&comment.author);
                        }
                        ui.weak(crate::timefmt::format_rfc3339_local(&comment.created_at));
                        if !comment.updated_at.is_empty()
                            && comment.updated_at != comment.created_at
                        {
                            Self::pill(
                                ui,
                                lang.t("edited"),
                                crate::theme::THEME.chip_fg,
                                crate::theme::THEME.chip_bg,
                            );
                        }
                        if comment.minimized {
                            Self::pill(
                                ui,
                                lang.t("minimized"),
                                crate::theme::THEME.state_closed_fg,
                                crate::theme::THEME.state_closed_bg,
                            );
                        }
                    });
                    if !comment.body.is_empty() {
                        ui.add_space(4.0);
                        crate::md::show(ui, cache, &comment.body);
                    }
                });
        });
        ui.add_space(4.0);
    }

    fn selected_row(&self) -> Option<&IssueRow> {
        let n = self.selected_issue?;
        self.all_rows.iter().find(|r| r.number == n)
    }

    fn show_status_bar(&mut self, ctx: &eframe::egui::Context) {
        let model = status_bar(self.syncing, &self.status);
        let lang = self.lang;
        let signed_in = self.token_value.is_some();
        eframe::egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.set_min_height(ui.spacing().interact_size.y);
                if model.reloading {
                    ui.label(lang.t("reloading"));
                } else if self.loading {
                    ui.label(lang.t("loading"));
                }
                if model.relogin {
                    ui.colored_label(ui.visuals().error_fg_color, lang.t("relogin_required"));
                    if signed_in
                        && self.user_code.is_none()
                        && ui
                            .add_enabled(
                                !self.loading,
                                eframe::egui::Button::new(lang.t("sign_in")),
                            )
                            .clicked()
                    {
                        self.loading = true;
                        self.spawn_login();
                    }
                }
                if signed_in {
                    if let Some(code) = self.user_code.clone() {
                        ui.separator();
                        ui.label(lang.t("user_code"));
                        ui.strong(&code);
                        if let Some(uri) = self.verification_uri.clone() {
                            ui.hyperlink(uri);
                        }
                    }
                }
                ui.with_layout(
                    eframe::egui::Layout::right_to_left(eframe::egui::Align::Center),
                    |ui| {
                        if let Some(n) = self.rate_remaining {
                            ui.label(format!("{}: {n}", lang.t("rate_remaining")));
                        }
                        if let Some(ts) = &self.last_synced {
                            ui.label(format!(
                                "{}: {}",
                                lang.t("last_synced"),
                                crate::timefmt::format_unix_local(ts)
                            ));
                        }
                    },
                );
            });
            if let Some(key) = model.notice_key {
                ui.colored_label(ui.visuals().error_fg_color, lang.t(key));
            } else if model.notice_raw {
                ui.colored_label(ui.visuals().error_fg_color, &self.status);
            }
        });
    }
}

impl eframe::App for IssueViewerApp {
    fn update(&mut self, ctx: &eframe::egui::Context, _frame: &mut eframe::Frame) {
        self.pump_messages();
        eframe::egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(self.lang.t("language"));
                if ui
                    .selectable_label(self.lang == Lang::En, "English")
                    .clicked()
                {
                    self.lang = Lang::En;
                }
                if ui
                    .selectable_label(self.lang == Lang::Ja, "日本語")
                    .clicked()
                {
                    self.lang = Lang::Ja;
                }
                if self.token_value.is_some() && ui.button(self.lang.t("sign_out")).clicked() {
                    self.tokens.clear();
                    if let Ok(cache) = Cache::open(&Self::cache_path()) {
                        let _ = cache.clear_last_repo();
                    }
                    self.token_value = None;
                    self.refresh_token = None;
                    self.access_expires_at = None;
                    self.refreshing = false;
                    self.repos.clear();
                    self.tree.clear();
                    self.all_rows.clear();
                    self.selected = None;
                    self.selected_issue = None;
                    self.clear_comments();
                    self.status.clear();
                }
            });
            if self.token_value.is_some() {
                ui.horizontal(|ui| {
                    ui.label("Repo");
                    let current = self
                        .selected
                        .as_ref()
                        .map(|(o, r)| format!("{o}/{r}"))
                        .unwrap_or_default();
                    let mut picked: Option<(String, String)> = None;
                    eframe::egui::ComboBox::from_id_salt("repo")
                        .selected_text(current)
                        .show_ui(ui, |ui| {
                            for repo in &self.repos {
                                let label = format!("{}/{}", repo.owner, repo.name);
                                if ui.selectable_label(false, &label).clicked() {
                                    picked = Some((repo.owner.clone(), repo.name.clone()));
                                }
                            }
                        });
                    if let Some((owner, name)) = picked {
                        self.selected = Some((owner.clone(), name.clone()));
                        self.selected_issue = None;
                        self.clear_comments();
                        self.persist_session();
                        self.reload_tree();
                        if let Some(token) = self.token_value.clone() {
                            self.spawn_sync(token, owner, name);
                        }
                    }
                    if ui
                        .add_enabled(
                            !self.syncing,
                            eframe::egui::Button::new(self.lang.t("refresh")),
                        )
                        .clicked()
                        && let (Some(token), Some((o, r))) =
                            (self.token_value.clone(), self.selected.clone())
                    {
                        self.spawn_sync(token, o, r);
                    }
                });
                ui.horizontal(|ui| {
                    let hint = self.lang.t("filter_hint");
                    ui.add(
                        eframe::egui::TextEdit::singleline(&mut self.query)
                            .hint_text(hint)
                            .desired_width(180.0),
                    );
                    if ui
                        .selectable_label(self.show_open, self.lang.t("opened"))
                        .clicked()
                    {
                        self.show_open = !self.show_open;
                    }
                    if ui
                        .selectable_label(self.show_closed, self.lang.t("closed"))
                        .clicked()
                    {
                        self.show_closed = !self.show_closed;
                    }
                    if ui
                        .selectable_label(self.sort_key == SortKey::Created, self.lang.t("created"))
                        .clicked()
                    {
                        self.sort_key = SortKey::Created;
                    }
                    if ui
                        .selectable_label(self.sort_key == SortKey::Updated, self.lang.t("updated"))
                        .clicked()
                    {
                        self.sort_key = SortKey::Updated;
                    }
                    if ui
                        .selectable_label(self.sort_dir == SortDir::Asc, self.lang.t("asc"))
                        .clicked()
                    {
                        self.sort_dir = SortDir::Asc;
                    }
                    if ui
                        .selectable_label(self.sort_dir == SortDir::Desc, self.lang.t("desc"))
                        .clicked()
                    {
                        self.sort_dir = SortDir::Desc;
                    }
                    self.rebuild_visible();
                });
            }
        });

        self.show_status_bar(ctx);

        if self.token_value.is_none() {
            eframe::egui::CentralPanel::default().show(ctx, |ui| {
                if ui.button(self.lang.t("sign_in")).clicked() {
                    self.loading = true;
                    self.spawn_login();
                }
                if let Some(code) = &self.user_code {
                    ui.label(self.lang.t("user_code"));
                    ui.heading(code);
                    if let Some(uri) = &self.verification_uri {
                        ui.hyperlink(uri);
                    }
                }
            });
            open_clicked_urls(ctx);
            ctx.request_repaint_after(Duration::from_millis(200));
            return;
        }

        let mut left_click = None;
        let mut right_click = None;
        const PANE_MIN: f32 = 100.0;
        const PANE_MAX: f32 = 640.0;
        let left = eframe::egui::SidePanel::left("issues")
            .resizable(true)
            .min_width(PANE_MIN)
            .max_width(PANE_MAX)
            .default_width(self.layout.left_w)
            .show(ctx, |ui| {
                ui.set_min_width(0.0);
                ui.set_width(ui.available_width());
                eframe::egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        Self::show_tree(
                            ui,
                            self.lang,
                            &self.tree,
                            self.selected_issue,
                            &mut left_click,
                            1,
                            self.selection_pane == Pane::Left,
                        );
                    });
            });
        let related = self
            .selected_issue
            .map(|n| focus_tree(&self.all_rows, n))
            .unwrap_or_default();
        let right = eframe::egui::SidePanel::right("related")
            .resizable(true)
            .min_width(PANE_MIN)
            .max_width(PANE_MAX)
            .default_width(self.layout.right_w)
            .show(ctx, |ui| {
                ui.set_min_width(0.0);
                ui.set_width(ui.available_width());
                ui.label(self.lang.t("related"));
                eframe::egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        Self::show_tree(
                            ui,
                            self.lang,
                            &related,
                            self.selected_issue,
                            &mut right_click,
                            2,
                            self.selection_pane == Pane::Right,
                        );
                    });
            });
        if let Some(n) = right_click {
            self.selected_issue = Some(n);
            self.selection_pane = Pane::Right;
            self.persist_session();
            self.ensure_comments();
        } else if let Some(n) = left_click {
            self.selected_issue = Some(n);
            self.selection_pane = Pane::Left;
            self.persist_session();
            self.ensure_comments();
        }

        let left_w = left.response.rect.width();
        let right_w = right.response.rect.width();
        let win = ctx.input(|i| i.viewport().inner_rect.map(|r| r.size()));
        let mut next = self.layout;
        next.left_w = left_w;
        next.right_w = right_w;
        if let Some(sz) = win {
            next.window_w = sz.x.max(400.0);
            next.window_h = sz.y.max(300.0);
        }
        if (next.left_w - self.layout.left_w).abs() > 0.5
            || (next.right_w - self.layout.right_w).abs() > 0.5
            || (next.window_w - self.layout.window_w).abs() > 0.5
            || (next.window_h - self.layout.window_h).abs() > 0.5
        {
            self.layout = next;
            self.persist_layout();
        }

        eframe::egui::CentralPanel::default().show(ctx, |ui| {
            if let (Some((owner, repo)), Some(row)) =
                (self.selected.clone(), self.selected_row().cloned())
            {
                ui.heading(format!("#{} {}", row.number, row.title));
                let theme = crate::theme::THEME;
                ui.horizontal(|ui| {
                    match row.state {
                        crate::model::IssueState::Open => {
                            Self::pill(
                                ui,
                                self.lang.t("opened"),
                                theme.state_open_fg,
                                theme.state_open_bg,
                            );
                        }
                        crate::model::IssueState::Closed => {
                            Self::pill(
                                ui,
                                self.lang.t("closed"),
                                theme.state_closed_fg,
                                theme.state_closed_bg,
                            );
                        }
                    }
                    Self::pill(
                        ui,
                        &format!(
                            "{} {}",
                            self.lang.t("created"),
                            crate::timefmt::format_rfc3339_local(&row.created_at)
                        ),
                        theme.chip_fg,
                        theme.chip_bg,
                    );
                    Self::pill(
                        ui,
                        &format!(
                            "{} {}",
                            self.lang.t("updated"),
                            crate::timefmt::format_rfc3339_local(&row.updated_at)
                        ),
                        theme.chip_fg,
                        theme.chip_bg,
                    );
                });
                Self::show_type_and_labels(ui, self.lang, &row);
                Self::show_fields(ui, self.lang, &row);
                if ui.button(self.lang.t("open_issue")).clicked() {
                    let url = format!(
                        "https://github.com/{}/{}/issues/{}",
                        owner, repo, row.number
                    );
                    let _ = webbrowser::open(&url);
                }
                ui.separator();
                eframe::egui::ScrollArea::vertical().show(ui, |ui| {
                    {
                        let mut cache = self.md_cache.borrow_mut();
                        crate::md::show(ui, &mut cache, &row.body);
                    }
                    self.show_comments_section(ui, &owner, &repo, row.number);
                });
            } else {
                ui.label(self.lang.t("select_issue"));
            }
        });
        open_clicked_urls(ctx);
        ctx.request_repaint_after(Duration::from_millis(200));
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.persist_layout();
    }
}

#[allow(deprecated)]
fn open_clicked_urls(ctx: &eframe::egui::Context) {
    let mut urls = Vec::new();
    ctx.output_mut(|o| {
        if let Some(open) = o.open_url.take() {
            urls.push(open.url);
        }
        o.commands.retain(|cmd| match cmd {
            eframe::egui::OutputCommand::OpenUrl(open) => {
                urls.push(open.url.clone());
                false
            }
            _ => true,
        });
    });
    for url in urls {
        if url.starts_with("https://") || url.starts_with("http://") {
            let _ = webbrowser::open(&url);
        }
    }
}
