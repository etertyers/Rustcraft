//! Game Files (Options) and the "Download the packs for this style?"
//! prompt: what is installed, Verify, Re-download, and fetching the style
//! packs later. One background job at a time; when it finishes the app
//! rebuilds its look (`take_reload`).

use super::SetupError;
use super::download::{self, Event, Handle, Policy};
use super::layers::{self, Presence};
use super::manifest::{self, Endpoints, Extra, Kind, StyleTag};
use super::screen::{self, Progress, megabytes};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

#[derive(Clone, PartialEq, Eq)]
enum Health {
    Installed,
    Missing,
    Damaged(String),
}

#[derive(Default)]
struct State {
    job: Option<Running>,
    prompt: Option<Prompt>,
    reload: bool,
    /// What the last job or check said.
    message: Option<String>,
    report: Option<Report>,
    check: Option<std::sync::mpsc::Receiver<Checked>>,
}

struct Running {
    handle: Handle,
    progress: Progress,
    what: String,
}

struct Prompt {
    style: StyleTag,
    missing: Vec<Extra>,
}

/// What Game Files shows, computed when the page opens and after a job.
struct Report {
    vanilla: Health,
    vanilla_where: PathBuf,
    vanilla_in_user_layer: bool,
    vanilla_bytes: u64,
    vanilla_version: String,
    extras: Vec<(Extra, Health)>,
    downloads_where: PathBuf,
}

struct Checked {
    vanilla: Option<Vec<String>>,
    damaged_extras: Vec<String>,
}

static STATE: Mutex<State> = Mutex::new(State {
    job: None,
    prompt: None,
    reload: false,
    message: None,
    report: None,
    check: None,
});

fn lock() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

fn target() -> Option<PathBuf> {
    layers::user_assets()
}

fn tag_of(style: crate::render::visuals::Style) -> Option<StyleTag> {
    use crate::render::visuals::Style;
    match style {
        Style::Vanilla => None,
        Style::Barebones => Some(StyleTag::Barebones),
        Style::Realistic => Some(StyleTag::Realistic),
    }
}

fn downloads_dir() -> PathBuf {
    let assets = crate::assets::dir();
    crate::assets::downloads_for(&assets)
}

/// The packs and jars `style` uses that this Mac doesn't have.
pub fn missing_for(style: StyleTag) -> Vec<Extra> {
    let dir = downloads_dir();
    manifest::extras()
        .iter()
        .filter(|e| e.styles.contains(&style) && layers::presence(&dir, e) != Presence::Present)
        .cloned()
        .collect()
}

/// Called when the player chooses a style: if its packs are missing, the
/// prompt opens (once per choice) instead of silently falling back.
pub fn style_chosen(style: crate::render::visuals::Style) {
    let Some(tag) = tag_of(style) else {
        return;
    };
    let missing = missing_for(tag);
    if missing.is_empty() || target().is_none() {
        return;
    }
    let mut state = lock();
    if state.job.is_none() {
        state.prompt = Some(Prompt {
            style: tag,
            missing,
        });
    }
}

/// True once after a job finished: the app should rebuild its look.
pub fn take_reload() -> bool {
    std::mem::take(&mut lock().reload)
}

fn start_job(state: &mut State, what: &str, vanilla: bool, extras: Vec<Extra>) {
    let Some(target) = target() else {
        state.message = Some("There's no Application Support folder to put the files in.".into());
        return;
    };
    state.message = None;
    state.job = Some(Running {
        handle: download::start_full(
            vanilla,
            extras,
            target,
            Endpoints::official(),
            Policy::default(),
        ),
        progress: Progress::new(),
        what: what.to_owned(),
    });
}

/// Takes the running job's events and the check's answer.
fn poll(state: &mut State) {
    let mut finished = None;
    if let Some(job) = state.job.as_mut() {
        for event in job.handle.rx.try_iter() {
            match event {
                Event::Started { total, .. } => job.progress.total = total,
                Event::File(name) => {
                    job.progress.file = name;
                    job.progress.stage = None;
                }
                Event::Bytes(n) => job.progress.done += n,
                Event::Stage(text) => job.progress.stage = Some(text),
                Event::Finished => finished = Some(Ok(())),
                Event::Failed(err) => finished = Some(Err(err)),
                Event::Cancelled => finished = Some(Err(SetupError::Cancelled)),
            }
        }
        job.progress.note(Instant::now());
    }
    if let Some(result) = finished {
        let what = state.job.take().map(|j| j.what).unwrap_or_default();
        match result {
            Ok(()) => {
                state.message = Some(format!("{what}: done."));
                state.reload = true;
            }
            Err(SetupError::Cancelled) => state.message = Some(format!("{what}: stopped.")),
            Err(err) => state.message = Some(format!("{what} failed: {err}")),
        }
        state.report = None;
    }
    if let Some(rx) = &state.check
        && let Ok(checked) = rx.try_recv()
    {
        state.check = None;
        let mut bad = checked.damaged_extras;
        match &checked.vanilla {
            Some(files) if !files.is_empty() => {
                bad.push(format!("{} of Minecraft's files", files.len()))
            }
            Some(_) => {}
            None => {}
        }
        state.message = Some(if bad.is_empty() {
            "Verified: every checksum matches.".to_owned()
        } else {
            format!("Damaged: {}. Re-download to fix.", bad.join(", "))
        });
        state.report = None;
    }
}

fn build_report() -> Report {
    let assets = crate::assets::dir();
    let layers = layers::current(&assets);
    let vanilla = match layers::check_vanilla(&layers.vanilla) {
        Ok(_) => Health::Installed,
        Err(problems) if layers.vanilla.join("assets").exists() => {
            Health::Damaged(problems.join("; "))
        }
        Err(_) => Health::Missing,
    };
    let stamp = layers::user_assets()
        .map(|t| download::Stamp::read(&t))
        .unwrap_or_default();
    let version = stamp
        .vanilla
        .map(|v| v.version)
        .unwrap_or_else(|| "from the fetch script".to_owned());
    let extras = manifest::extras()
        .iter()
        .map(|e| {
            let health = match layers::presence(&layers.downloads, e) {
                Presence::Present => Health::Installed,
                Presence::Missing => Health::Missing,
                Presence::WrongSize => Health::Damaged("wrong size".into()),
            };
            (e.clone(), health)
        })
        .collect();
    Report {
        vanilla_bytes: if vanilla == Health::Missing {
            0
        } else {
            download::dir_size(&layers.vanilla.join("assets"))
        },
        vanilla,
        vanilla_in_user_layer: layers.vanilla_in_user_layer,
        vanilla_where: layers.vanilla,
        vanilla_version: version,
        extras,
        downloads_where: layers.downloads,
    }
}

fn start_check(state: &mut State) {
    let Some(report) = state.report.as_ref() else {
        return;
    };
    let (vanilla, downloads) = (report.vanilla_where.clone(), report.downloads_where.clone());
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let damaged_extras = download::damaged_extras(&downloads)
            .iter()
            .map(|e| e.name.clone())
            .collect();
        let _ = tx.send(Checked {
            vanilla: download::verify_vanilla(&vanilla),
            damaged_extras,
        });
    });
    state.message = Some("Checking every file…".into());
    state.check = Some(rx);
}

fn health_text(ui: &mut egui::Ui, health: &Health) {
    match health {
        Health::Installed => {
            ui.label("Installed");
        }
        Health::Missing => {
            ui.colored_label(ui.visuals().warn_fg_color, "Missing");
        }
        Health::Damaged(why) => {
            ui.colored_label(ui.visuals().error_fg_color, format!("Damaged ({why})"));
        }
    }
}

fn progress_ui(ui: &mut egui::Ui, job: &Running) -> bool {
    let p = &job.progress;
    ui.label(format!("{}…", job.what));
    ui.add(egui::ProgressBar::new(p.fraction()).show_percentage());
    let rate = p.rate();
    let left = p.total.saturating_sub(p.done.max(0) as u64);
    let mut line = format!(
        "{} of {}",
        megabytes(p.done.max(0) as u64),
        megabytes(p.total)
    );
    if rate > 1000.0 {
        line += &format!(
            ", {:.1} MB/s, about {} left",
            rate / 1e6,
            screen::duration_text(left as f64 / rate)
        );
    }
    ui.label(line);
    if let Some(stage) = &p.stage {
        ui.label(stage.as_str());
    } else if !p.file.is_empty() {
        ui.label(format!("Now: {}", p.file));
    }
    ui.button("Cancel").clicked()
}

/// Draws the prompt and the progress of a background job over whatever
/// screen is open. Called from the UI frames.
pub fn show(ctx: &egui::Context) {
    let mut state = lock();
    poll(&mut state);
    if state.job.is_some() || state.prompt.is_some() {
        ctx.request_repaint();
    }
    let mut cancel = false;
    if let Some(job) = &state.job {
        egui::Window::new("Downloading")
            .anchor(egui::Align2::RIGHT_BOTTOM, [-12.0, -12.0])
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| cancel = progress_ui(ui, job));
    }
    if cancel && let Some(job) = &state.job {
        job.handle.cancel();
    }
    let mut answer = None;
    if let Some(prompt) = &state.prompt {
        let bytes: u64 = prompt.missing.iter().map(|e| e.size).sum();
        let packs = prompt
            .missing
            .iter()
            .filter(|e| e.kind == Kind::Pack)
            .count();
        let mods = prompt.missing.len() - packs;
        egui::Window::new("Download the packs for this style?")
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(format!(
                    "The {} style uses {packs} packs and {mods} mods that aren't on this Mac yet ({}), \
                     from Modrinth. Without them it falls back to what's installed.",
                    match prompt.style {
                        StyleTag::Barebones => "Barebones",
                        StyleTag::Realistic => "Realistic",
                    },
                    megabytes(bytes)
                ));
                ui.horizontal(|ui| {
                    if ui.button(format!("Download ({})", megabytes(bytes))).clicked() {
                        answer = Some(true);
                    }
                    if ui.button("Not now").clicked() {
                        answer = Some(false);
                    }
                });
            });
    }
    match answer {
        Some(true) => {
            if let Some(prompt) = state.prompt.take() {
                start_job(
                    &mut state,
                    "Downloading the style packs",
                    false,
                    prompt.missing,
                );
            }
        }
        Some(false) => state.prompt = None,
        None => {}
    }
}

/// The body of Options → Game Files.
pub fn page(ui: &mut egui::Ui) {
    let mut state = lock();
    poll(&mut state);
    if state.report.is_none() {
        state.report = Some(build_report());
    }
    ui.ctx().request_repaint();
    let busy = state.job.is_some() || state.check.is_some();
    let mut verify = false;
    let mut redownload = false;
    let mut get_packs = false;
    if let Some(report) = &state.report {
        ui.strong("Minecraft's textures, sounds and font");
        ui.horizontal(|ui| {
            health_text(ui, &report.vanilla);
            if report.vanilla != Health::Missing {
                ui.label(format!(
                    "— {}, {}",
                    report.vanilla_version,
                    megabytes(report.vanilla_bytes)
                ));
            }
        });
        ui.label(format!(
            "In {}{}",
            report.vanilla_where.display(),
            if report.vanilla_in_user_layer {
                ""
            } else {
                " (the app's own copy)"
            }
        ));
        ui.add_space(6.0);
        ui.strong("Style packs and mods");
        let missing = report
            .extras
            .iter()
            .filter(|(_, h)| *h != Health::Installed)
            .count();
        for (extra, health) in &report.extras {
            ui.horizontal(|ui| {
                ui.label(format!("{} ({})", extra.name, megabytes(extra.size)));
                health_text(ui, health);
            });
        }
        ui.label(format!("In {}", report.downloads_where.display()));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            verify = ui.add_enabled(!busy, egui::Button::new("Verify")).clicked();
            redownload = ui
                .add_enabled(!busy, egui::Button::new("Re-download Minecraft's files"))
                .clicked();
            get_packs = ui
                .add_enabled(!busy && missing > 0, egui::Button::new("Get style packs"))
                .clicked();
        });
    }
    if let Some(message) = &state.message {
        ui.add_space(6.0);
        ui.label(message.as_str());
    }
    let mut cancel = false;
    if let Some(job) = &state.job {
        ui.add_space(6.0);
        cancel = progress_ui(ui, job);
    }
    if cancel && let Some(job) = &state.job {
        job.handle.cancel();
    }
    if verify {
        start_check(&mut state);
    }
    if redownload {
        start_job(
            &mut state,
            "Re-downloading Minecraft's files",
            true,
            Vec::new(),
        );
    }
    if get_packs {
        let missing: Vec<Extra> = state
            .report
            .as_ref()
            .map(|r| {
                r.extras
                    .iter()
                    .filter(|(_, h)| *h != Health::Installed)
                    .map(|(e, _)| e.clone())
                    .collect()
            })
            .unwrap_or_default();
        start_job(&mut state, "Downloading the style packs", false, missing);
    }
}

/// Forgets the cached report so the page counts again (when it opens).
pub fn forget_report() {
    let mut state = lock();
    state.report = None;
    state.message = None;
}
