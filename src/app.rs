use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use crate::auth::{poll_token, request_device_code};
use crate::config::{GITHUB_CLIENT_ID, GITHUB_SCOPE};
use crate::db::Cache;
use crate::github::{GitHubClient, LiveClient, RepoRef};
use crate::i18n::Lang;
use crate::sync::sync_repo;
use crate::token::{KeyringTokenStore, TokenStore};
use crate::tree::{build_tree, TreeNode};

enum UiMsg {
    DeviceCode {
        user_code: String,
        verification_uri: String,
    },
    LoggedIn(String),
    Repos(Vec<RepoRef>),
    SyncDone,
    Error(String),
}

pub struct IssueViewerApp {
    lang: Lang,
    tokens: KeyringTokenStore,
    token_value: Option<String>,
    user_code: Option<String>,
    verification_uri: Option<String>,
    repos: Vec<RepoRef>,
    selected: Option<(String, String)>,
    tree: Vec<TreeNode>,
    last_synced: Option<String>,
    status: String,
    loading: bool,
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
            tree: Vec::new(),
            last_synced: None,
            status: String::new(),
            loading: false,
            tx,
            rx,
        };
        if let Some(token) = token_value {
            app.spawn_list_repos(token);
        }
        app
    }

    fn cache_path() -> PathBuf {
        let mut dir = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
        dir.push("issue-viewer");
        dir.push("cache.sqlite");
        dir
    }

    fn spawn_login(&self) {
        let tx = self.tx.clone();
        thread::spawn(move || {
            if GITHUB_CLIENT_ID == "REPLACE_ME" {
                let _ = tx.send(UiMsg::Error(
                    "Set GITHUB_CLIENT_ID in src/config.rs (OAuth App with Device Flow enabled)"
                        .into(),
                ));
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
                Ok(()) => {
                    let _ = tx.send(UiMsg::SyncDone);
                }
                Err(e) => {
                    let _ = tx.send(UiMsg::Error(e.to_string()));
                }
            }
        });
    }

    fn reload_tree(&mut self) {
        let Some((owner, repo)) = self.selected.clone() else {
            return;
        };
        match Cache::open(&Self::cache_path()) {
            Ok(cache) => {
                match cache.list_issues(&owner, &repo) {
                    Ok(rows) => self.tree = build_tree(&rows),
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
                UiMsg::SyncDone => {
                    self.loading = false;
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

    fn show_tree(ui: &mut eframe::egui::Ui, nodes: &[TreeNode], owner: &str, repo: &str) {
        for node in nodes {
            let state = match node.issue.state {
                crate::model::IssueState::Open => "open",
                crate::model::IssueState::Closed => "closed",
            };
            let label = format!("#{} [{}] {}", node.issue.number, state, node.issue.title);
            if node.children.is_empty() {
                if ui.selectable_label(false, label).clicked() {
                    let url = format!(
                        "https://github.com/{}/{}/issues/{}",
                        owner, repo, node.issue.number
                    );
                    let _ = webbrowser::open(&url);
                }
            } else {
                eframe::egui::CollapsingHeader::new(label)
                    .id_salt(node.issue.number)
                    .default_open(true)
                    .show(ui, |ui| {
                        Self::show_tree(ui, &node.children, owner, repo);
                    });
            }
        }
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
                    self.token_value = None;
                    self.repos.clear();
                    self.tree.clear();
                    self.selected = None;
                }
            });
        });
        eframe::egui::CentralPanel::default().show(ctx, |ui| {
            if self.token_value.is_none() {
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
            } else {
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
                        self.reload_tree();
                        if let Some(token) = self.token_value.clone() {
                            self.spawn_sync(token, owner, name);
                        }
                    }
                    if ui.button(self.lang.t("refresh")).clicked() {
                        if let (Some(token), Some((o, r))) =
                            (self.token_value.clone(), self.selected.clone())
                        {
                            self.spawn_sync(token, o, r);
                        }
                    }
                });
                if self.loading {
                    ui.label(self.lang.t("loading"));
                }
                if let Some(ts) = &self.last_synced {
                    ui.label(format!("{}: {ts}", self.lang.t("last_synced")));
                }
                if !self.status.is_empty() {
                    ui.colored_label(eframe::egui::Color32::RED, &self.status);
                }
                if let Some((owner, repo)) = &self.selected {
                    eframe::egui::ScrollArea::vertical().show(ui, |ui| {
                        Self::show_tree(ui, &self.tree, owner, repo);
                    });
                }
            }
        });
        ctx.request_repaint_after(Duration::from_millis(200));
    }
}
