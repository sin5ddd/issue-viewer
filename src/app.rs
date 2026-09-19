use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use crate::auth::{is_placeholder_client_id, poll_token, request_device_code, try_gh_cli_token};
use crate::config::{GITHUB_CLIENT_ID, GITHUB_SCOPE};
use crate::db::{Cache, UiLayout};
use crate::filter::{self, SortDir, SortKey};
use crate::github::{GitHubClient, LiveClient, RepoRef};
use crate::i18n::Lang;
use crate::model::IssueRow;
use crate::sync::sync_repo;
use crate::token::{KeyringTokenStore, TokenStore};
use crate::tree::{TreeNode, build_tree, focus_tree};

enum UiMsg {
    DeviceCode {
        user_code: String,
        verification_uri: String,
    },
    LoggedIn(String),
    Repos(Vec<RepoRef>),
    SyncDone { remaining: Option<u32> },
    Error(String),
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
    layout: UiLayout,
    tx: Sender<UiMsg>,
    rx: Receiver<UiMsg>,
}

impl IssueViewerApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        crate::fonts::install(&cc.egui_ctx);
        let (tx, rx) = mpsc::channel();
        let tokens = KeyringTokenStore;
        let token_value = tokens.load();
        let mut app = Self {
            lang: Lang::detect(),
            tokens,
            token_value: token_value.clone(),
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
            layout: Cache::open(&Self::cache_path())
                .ok()
                .and_then(|c| c.ui_layout().ok())
                .unwrap_or_default(),
            tx,
            rx,
        };
        if let Some(token) = token_value {
            if let Ok(cache) = Cache::open(&Self::cache_path())
                && let Ok(Some((owner, repo, issue))) = cache.last_repo()
            {
                app.selected = Some((owner.clone(), repo.clone()));
                app.selected_issue = issue;
                app.reload_tree();
                app.spawn_sync(token.clone(), owner, repo);
            }
            app.spawn_list_repos(token);
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
                    let _ = tx.send(UiMsg::LoggedIn(token));
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
                    let _ = tx.send(UiMsg::LoggedIn(token));
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
                UiMsg::LoggedIn(token) => {
                    self.tokens.save(&token);
                    self.token_value = Some(token.clone());
                    self.user_code = None;
                    self.spawn_list_repos(token);
                }
                UiMsg::Repos(repos) => {
                    self.repos = repos;
                    self.loading = false;
                }
                UiMsg::SyncDone { remaining } => {
                    self.loading = false;
                    self.rate_remaining = remaining;
                    self.status.clear();
                    self.reload_tree();
                }
                UiMsg::Error(e) => {
                    self.loading = false;
                    self.status = e;
                }
            }
        }
    }

    fn show_tree(
        ui: &mut eframe::egui::Ui,
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
                Self::show_tree_rows(ui, nodes, selected, clicked, id_ns, active);
            },
        );
    }

    fn show_tree_rows(
        ui: &mut eframe::egui::Ui,
        nodes: &[TreeNode],
        selected: Option<u64>,
        clicked: &mut Option<u64>,
        id_ns: u64,
        active: bool,
    ) {
        ui.spacing_mut().item_spacing.y = 2.0;
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
            ui.allocate_new_ui(
                eframe::egui::UiBuilder::new()
                    .max_rect(rect)
                    .layout(eframe::egui::Layout::left_to_right(
                        eframe::egui::Align::Center,
                    )),
                |ui| {
                    if has_children {
                        if Self::disclosure(ui, open) {
                            open = !open;
                        }
                    } else {
                        ui.add_space(14.0);
                    }
                    ui.add(
                        eframe::egui::Label::new(format!("#{}", node.issue.number)).selectable(false),
                    );
                    Self::state_pill(ui, node.issue.state);
                    ui.add(
                        eframe::egui::Label::new(&node.issue.title)
                            .truncate()
                            .halign(eframe::egui::Align::LEFT)
                            .selectable(false),
                    );
                },
            );
            if row_resp.clicked() {
                *clicked = Some(node.issue.number);
            }
            ui.data_mut(|d| d.insert_persisted(id, open));
            if has_children && open {
                ui.indent(id, |ui| {
                    Self::show_tree_rows(ui, &node.children, selected, clicked, id_ns, active);
                });
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

    fn pill(ui: &mut eframe::egui::Ui, text: &str, fg: eframe::egui::Color32, bg: eframe::egui::Color32) {
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

    fn selected_row(&self) -> Option<&IssueRow> {
        let n = self.selected_issue?;
        self.all_rows.iter().find(|r| r.number == n)
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
                    self.repos.clear();
                    self.tree.clear();
                    self.all_rows.clear();
                    self.selected = None;
                    self.selected_issue = None;
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
                        self.persist_session();
                        self.reload_tree();
                        if let Some(token) = self.token_value.clone() {
                            self.spawn_sync(token, owner, name);
                        }
                    }
                    if ui.button(self.lang.t("refresh")).clicked()
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
                if self.loading {
                    ui.label(self.lang.t("loading"));
                }
                if let Some(ts) = &self.last_synced {
                    ui.label(format!(
                        "{}: {}",
                        self.lang.t("last_synced"),
                        crate::timefmt::format_unix_local(ts)
                    ));
                }
                if let Some(n) = self.rate_remaining {
                    ui.label(format!("{}: {n}", self.lang.t("rate_remaining")));
                }
                if !self.status.is_empty() {
                    let text = if self.status.contains("rate_limited") {
                        self.lang.t("rate_limited")
                    } else if self.status == "need_oauth_app" {
                        self.lang.t("need_oauth_app")
                    } else {
                        self.status.as_str()
                    };
                    ui.colored_label(eframe::egui::Color32::RED, text);
                }
            }
        });

        if self.token_value.is_none() {
            eframe::egui::CentralPanel::default().show(ctx, |ui| {
                if ui.button(self.lang.t("sign_in")).clicked() {
                    self.loading = true;
                    self.spawn_login();
                }
                if self.loading {
                    ui.label(self.lang.t("loading"));
                }
                if let Some(code) = &self.user_code {
                    ui.label(self.lang.t("user_code"));
                    ui.heading(code);
                    if let Some(uri) = &self.verification_uri {
                        ui.hyperlink(uri);
                    }
                }
                if !self.status.is_empty() {
                    let text = if self.status == "need_oauth_app" {
                        self.lang.t("need_oauth_app")
                    } else {
                        self.status.as_str()
                    };
                    ui.colored_label(eframe::egui::Color32::RED, text);
                }
            });
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
        } else if let Some(n) = left_click {
            self.selected_issue = Some(n);
            self.selection_pane = Pane::Left;
            self.persist_session();
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
                if ui.button(self.lang.t("open_issue")).clicked() {
                    let url = format!(
                        "https://github.com/{}/{}/issues/{}",
                        owner, repo, row.number
                    );
                    let _ = webbrowser::open(&url);
                }
                ui.separator();
                eframe::egui::ScrollArea::vertical().show(ui, |ui| {
                    crate::md::show(ui, &row.body);
                });
            } else {
                ui.label(self.lang.t("select_issue"));
            }
        });
        ctx.request_repaint_after(Duration::from_millis(200));
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.persist_layout();
    }
}
