//! Update check against GitHub releases, and the About dialog's update and
//! open-source credits sections.

use crate::app::PakPosApp;
use crate::theme;
use egui::{RichText, Ui};
use pakpos_core::update::Release;
use std::sync::mpsc::{self, Receiver};

#[derive(Default)]
pub enum UpdateStatus {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    Failed(String),
}

#[derive(Default)]
pub struct UpdateState {
    pub status: UpdateStatus,
    rx: Option<Receiver<Result<Option<Release>, String>>>,
    /// Started from the About dialog: report every outcome, not only news.
    manual: bool,
}

impl UpdateState {
    pub fn available(&self) -> Option<&Release> {
        match &self.status {
            UpdateStatus::Available(r) => Some(r),
            _ => None,
        }
    }
}

/// Open-source projects PakPos is built on: name, what it does here, licence, home page.
const CREDITS: &[(&str, &str, &str, &str)] = &[
    ("egui / eframe", "User interface", "MIT OR Apache-2.0", "https://github.com/emilk/egui"),
    ("egui_extras + syntect", "Syntax highlighting, images", "MIT OR Apache-2.0", "https://github.com/emilk/egui"),
    ("reqwest", "HTTP client", "MIT OR Apache-2.0", "https://github.com/seanmonstar/reqwest"),
    ("rustls", "TLS", "Apache-2.0 OR ISC OR MIT", "https://github.com/rustls/rustls"),
    ("tokio", "Async runtime", "MIT", "https://github.com/tokio-rs/tokio"),
    ("tonic", "gRPC", "MIT", "https://github.com/hyperium/tonic"),
    ("prost", "Protocol Buffers", "Apache-2.0", "https://github.com/tokio-rs/prost"),
    (
        "prost-reflect / protox",
        "Protobuf reflection, .proto parsing",
        "MIT OR Apache-2.0",
        "https://github.com/andrewhickman/protox",
    ),
    ("tungstenite", "WebSocket", "MIT OR Apache-2.0", "https://github.com/snapview/tungstenite-rs"),
    ("rquickjs (QuickJS)", "JavaScript scripts", "MIT", "https://github.com/DelSkayn/rquickjs"),
    ("serde / serde_json", "JSON", "MIT OR Apache-2.0", "https://github.com/serde-rs/serde"),
    ("yaml-rust2", "YAML (OpenAPI import)", "MIT OR Apache-2.0", "https://github.com/Ethiraric/yaml-rust2"),
    ("RustCrypto hashes", "Hashes and signatures", "MIT OR Apache-2.0", "https://github.com/RustCrypto"),
    ("cookie", "Cookie jar", "MIT OR Apache-2.0", "https://github.com/SergioBenitez/cookie-rs"),
    ("chrono", "Dates and times", "MIT OR Apache-2.0", "https://github.com/chronotope/chrono"),
    ("url", "URL parsing", "MIT OR Apache-2.0", "https://github.com/servo/rust-url"),
    ("uuid", "IDs", "Apache-2.0 OR MIT", "https://github.com/uuid-rs/uuid"),
    ("image", "Image previews", "MIT OR Apache-2.0", "https://github.com/image-rs/image"),
    ("rfd", "File dialogs", "MIT", "https://github.com/PolyMeilex/rfd"),
    ("directories", "Data folders", "MIT OR Apache-2.0", "https://github.com/soc/directories-rs"),
    ("webbrowser", "Opening links", "MIT OR Apache-2.0", "https://github.com/amodm/webbrowser-rs"),
    ("clap", "Command line (pakpos-cli)", "MIT OR Apache-2.0", "https://github.com/clap-rs/clap"),
    ("Inter", "Font", "OFL-1.1", "https://github.com/rsms/inter"),
    ("JetBrains Mono", "Font", "OFL-1.1", "https://github.com/JetBrains/JetBrainsMono"),
    ("Noto fonts", "Emoji, symbols, arrows", "OFL-1.1", "https://github.com/google/fonts"),
];

impl PakPosApp {
    /// Starts a check in the background. `manual` checks report "up to date"
    /// and errors too; the one at startup stays quiet unless there is news.
    pub fn check_for_updates(&mut self, ctx: &egui::Context, manual: bool) {
        if self.update.rx.is_some() {
            self.update.manual |= manual;
            return;
        }
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(pakpos_core::update::check(pakpos_core::VERSION));
            ctx.request_repaint();
        });
        self.update = UpdateState { status: UpdateStatus::Checking, rx: Some(rx), manual };
    }

    pub fn poll_updates(&mut self) {
        let Some(rx) = &self.update.rx else { return };
        let Ok(result) = rx.try_recv() else { return };
        self.update.rx = None;
        let manual = self.update.manual;
        self.update.status = match result {
            Ok(Some(r)) => {
                if !manual {
                    // Longer than a normal toast: it is easy to miss while the app opens.
                    self.toasts.push(crate::app::Toast {
                        text: crate::i18n::trf("PakPos {v} is available", &[("v", &r.version)]),
                        error: false,
                        until: std::time::Instant::now() + std::time::Duration::from_secs(12),
                    });
                }
                UpdateStatus::Available(r)
            }
            Ok(None) => UpdateStatus::UpToDate,
            // Offline at startup is normal: only a manual check shows the error.
            Err(e) if manual => UpdateStatus::Failed(e),
            Err(_) => UpdateStatus::Idle,
        };
    }

    /// Update status line and button, for the About dialog.
    pub fn update_section(&mut self, ui: &mut Ui) {
        let p = self.palette;
        // One widget per line, so each line centres in the About dialog.
        match &self.update.status {
            UpdateStatus::Idle => {}
            UpdateStatus::Checking => {
                ui.label(RichText::new(crate::i18n::tr("Checking for updates…")).color(p.text_weak));
            }
            UpdateStatus::UpToDate => {
                ui.label(RichText::new(crate::i18n::tr("✔ You have the latest version")).color(theme::GREEN));
            }
            UpdateStatus::Available(r) => {
                ui.label(
                    RichText::new(crate::i18n::trf("PakPos {v} is available", &[("v", &r.version)]))
                        .strong()
                        .color(theme::ORANGE),
                );
                ui.add_space(4.0);
                if crate::widgets::primary_button(ui, crate::i18n::tr("Download")).clicked() {
                    let _ = webbrowser::open(&r.url);
                }
                return;
            }
            UpdateStatus::Failed(e) => {
                ui.label(RichText::new(crate::i18n::tr("Could not check for updates")).color(theme::RED))
                    .on_hover_text(e.as_str());
            }
        }
        ui.add_space(4.0);
        let busy = matches!(self.update.status, UpdateStatus::Checking);
        if ui.add_enabled(!busy, egui::Button::new(crate::i18n::tr("Check for updates"))).clicked() {
            self.check_for_updates(ui.ctx(), true);
        }
    }
}

/// Collapsible list of the open-source projects PakPos uses.
pub fn credits_section(ui: &mut Ui, p: &theme::Palette) {
    egui::CollapsingHeader::new(crate::i18n::tr("Open-source libraries")).id_salt("credits").show(ui, |ui| {
        egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
            egui::Grid::new("credits_grid").num_columns(3).spacing([12.0, 4.0]).striped(true).show(ui, |ui| {
                for (name, what, license, url) in CREDITS {
                    ui.hyperlink_to(*name, *url);
                    ui.label(RichText::new(crate::i18n::tr(what)).small().color(p.text_weak));
                    ui.label(RichText::new(*license).small().color(p.text_weak));
                    ui.end_row();
                }
            });
        });
        ui.label(
            RichText::new(crate::i18n::tr("Plus their own dependencies; see Cargo.lock for the full list."))
                .small()
                .color(p.text_weak),
        );
    });
}
