//! Script editors and the trust banner.

use crate::theme::{self, Palette};
use crate::widgets;
use egui::{RichText, Ui};
use pakpos_core::model::Scripts;

const PRE_SNIPPETS: &[(&str, &str)] = &[
    ("Set an environment variable", "pm.environment.set(\"key\", \"value\");\n"),
    ("Set a local variable", "pm.variables.set(\"timestamp\", String(Date.now()));\n"),
    (
        "Add / replace a header",
        "pm.request.headers.upsert({ key: \"X-Request-Id\", value: pm.variables.replaceIn(\"{{$guid}}\") });\n",
    ),
    (
        "Build the body from variables",
        "pm.request.body.raw = JSON.stringify({ user: pm.environment.get(\"user\"), at: new Date().toISOString() });\n",
    ),
    ("Log the request", "console.log(pm.request.method, pm.request.url);\n"),
];

const TEST_SNIPPETS: &[(&str, &str)] = &[
    ("Status code is 200", "pm.test(\"Status code is 200\", () => {\n  pm.response.to.have.status(200);\n});\n"),
    ("Response time below 500 ms", "pm.test(\"Response time is below 500 ms\", () => {\n  pm.expect(pm.response.responseTime).to.be.below(500);\n});\n"),
    ("Check a JSON value", "pm.test(\"Has the expected name\", () => {\n  const json = pm.response.json();\n  pm.expect(json.name).to.eql(\"Budi\");\n});\n"),
    ("Save a token to the environment", "const json = pm.response.json();\npm.environment.set(\"token\", json.token);\n"),
    ("Header is present", "pm.test(\"Content-Type is JSON\", () => {\n  pm.expect(pm.response.headers.get(\"Content-Type\")).to.include(\"json\");\n});\n"),
    ("Array is not empty", "pm.test(\"Returns items\", () => {\n  pm.expect(pm.response.json().items).to.be.an(\"array\").that.is.not.empty;\n});\n"),
    ("Log the response", "console.log(pm.response.code, pm.response.text());\n"),
];

pub enum ScriptAction {
    Trust,
}

fn editor(
    ui: &mut Ui,
    title: &str,
    hint: &str,
    code: &mut String,
    id: (&str, u64),
    snippets: &[(&str, &str)],
    p: &Palette,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        widgets::section_title(ui, title);
        ui.menu_button(RichText::new("Snippets ▾").small(), |ui| {
            for (label, snippet) in snippets {
                if ui.button(*label).clicked() {
                    if !code.is_empty() && !code.ends_with('\n') {
                        code.push('\n');
                    }
                    code.push_str(snippet);
                    changed = true;
                    ui.close();
                }
            }
        });
        if !code.trim().is_empty() {
            ui.label(RichText::new(format!("{} lines", code.lines().count())).small().color(p.text_weak));
        }
    });
    let theme_ = egui_extras::syntax_highlighting::CodeTheme::from_style(ui.style());
    let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
        let mut job = egui_extras::syntax_highlighting::highlight(ui.ctx(), ui.style(), &theme_, buf.as_str(), "js");
        job.wrap.max_width = wrap_width;
        ui.ctx().fonts_mut(|f| f.layout_job(job))
    };
    changed |= ui
        .add(
            egui::TextEdit::multiline(code)
                .id_salt(id)
                .code_editor()
                .desired_rows(5)
                .desired_width(f32::INFINITY)
                .hint_text(hint)
                .layouter(&mut layouter),
        )
        .changed();
    changed
}

/// Pre-request + post-response editors. `trusted` is `None` for unsaved requests.
pub fn scripts_editor(
    ui: &mut Ui,
    scripts: &mut Scripts,
    uid: u64,
    trusted: Option<bool>,
    inherited: usize,
    p: &Palette,
    action: &mut Option<ScriptAction>,
) -> bool {
    let mut changed = false;
    if trusted == Some(false) {
        egui::Frame::new()
            .fill(theme::YELLOW.gamma_multiply(0.12))
            .stroke(egui::Stroke::new(1.0, theme::YELLOW.gamma_multiply(0.6)))
            .corner_radius(8)
            .inner_margin(10)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new("⚠").color(theme::YELLOW).size(16.0));
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Scripts in this collection are not run yet").strong());
                        ui.label(
                            RichText::new(
                                "It came from outside PakPos (import or shared folder). Scripts run sandboxed, without file or network access, but read them before trusting.",
                            )
                            .small()
                            .color(p.text_weak),
                        );
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::primary_button(ui, "Trust collection").clicked() {
                            *action = Some(ScriptAction::Trust);
                        }
                    });
                });
            });
        ui.add_space(8.0);
    }
    if inherited > 0 {
        ui.label(
            RichText::new(format!(
                "{inherited} collection/folder script(s) also run around this request (edit them via the collection's “Edit collection…”)."
            ))
            .small()
            .color(p.text_weak),
        );
        ui.add_space(4.0);
    }
    changed |= editor(
        ui,
        "Pre-request script",
        "// Runs before the request is sent.\npm.variables.set(\"ts\", String(Date.now()));",
        &mut scripts.pre_request,
        ("pre_script", uid),
        PRE_SNIPPETS,
        p,
    );
    ui.add_space(8.0);
    changed |= editor(
        ui,
        "Post-response script (tests)",
        "pm.test(\"Status code is 200\", () => pm.response.to.have.status(200));",
        &mut scripts.test,
        ("test_script", uid),
        TEST_SNIPPETS,
        p,
    );
    ui.add_space(4.0);
    ui.label(
        RichText::new("pm.* API: pm.test, pm.expect, pm.response, pm.request, pm.environment, pm.collectionVariables, pm.globals, pm.variables, console.log. No pm.sendRequest, require or file access.")
            .small()
            .color(p.text_weak),
    );
    changed
}

/// Script console output in the response panel.
pub fn console(ui: &mut Ui, logs: &[String], uid: u64, p: &Palette) {
    if logs.is_empty() {
        ui.label(RichText::new("No console output. Use console.log() in a script.").color(p.text_weak));
        return;
    }
    egui::ScrollArea::vertical().id_salt(("console", uid)).auto_shrink([false, false]).show(ui, |ui| {
        for line in logs {
            let color = if line.starts_with("[error]") {
                theme::RED
            } else if line.starts_with("[warn]") {
                theme::YELLOW
            } else {
                p.text
            };
            ui.add(egui::Label::new(RichText::new(line).monospace().color(color)).wrap());
        }
    });
}
