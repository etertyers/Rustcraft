//! The download worker: HTTP with timeouts and retries, checksums, a
//! thread pool, progress over a channel, resume, and the atomic move into
//! place.
//!
//! Layout: everything is written under `<target>/vanilla.partial/` and
//! `<target>/downloads.partial/` and renamed into place only when the whole
//! set is verified, so a run that stops halfway leaves nothing that looks
//! installed. A file already in a `.partial` folder with the right checksum
//! is kept, so a second run fetches only what is missing.

use super::layers::{self, STAMP_FILE};
use super::manifest::{self, Endpoints, Extra};
use super::vanilla::{self, Plan};
use super::{Result, SetupError};
use crossbeam_channel::{Receiver, Sender};
use serde::{Deserialize, Serialize};
use sha1::Digest;
use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Parallel connections for the asset objects (Kenzo's design).
pub const CONNECTIONS: usize = 8;
/// Tries per file, the first included.
pub const ATTEMPTS: u32 = 4;
/// The wait before the second try; it doubles each time.
pub const BACKOFF: Duration = Duration::from_secs(1);
/// Connecting and resolving give up after this.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// The server must start answering within this.
pub const RESPONSE_TIMEOUT: Duration = Duration::from_secs(20);
/// A body that sends nothing for this long is dropped.
pub const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Timeouts and retries; tests shorten them.
#[derive(Debug, Clone, Copy)]
pub struct Policy {
    pub attempts: u32,
    pub backoff: Duration,
    pub connect: Duration,
    pub response: Duration,
    pub read: Duration,
    pub connections: usize,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            attempts: ATTEMPTS,
            backoff: BACKOFF,
            connect: CONNECT_TIMEOUT,
            response: RESPONSE_TIMEOUT,
            read: READ_TIMEOUT,
            connections: CONNECTIONS,
        }
    }
}

// --- Checksums -------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algo {
    Sha1,
    Sha512,
}

/// A file's expected hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub algo: Algo,
    pub hex: String,
}

impl Check {
    pub fn sha1(hex: &str) -> Self {
        Self {
            algo: Algo::Sha1,
            hex: hex.to_ascii_lowercase(),
        }
    }

    pub fn sha512(hex: &str) -> Self {
        Self {
            algo: Algo::Sha512,
            hex: hex.to_ascii_lowercase(),
        }
    }
}

enum Hasher {
    Sha1(sha1::Sha1),
    Sha512(sha2::Sha512),
}

impl Hasher {
    fn new(algo: Algo) -> Self {
        match algo {
            Algo::Sha1 => Self::Sha1(sha1::Sha1::new()),
            Algo::Sha512 => Self::Sha512(sha2::Sha512::new()),
        }
    }

    fn update(&mut self, data: &[u8]) {
        match self {
            Self::Sha1(h) => h.update(data),
            Self::Sha512(h) => h.update(data),
        }
    }

    fn finish(self) -> String {
        match self {
            Self::Sha1(h) => hex::encode(h.finalize()),
            Self::Sha512(h) => hex::encode(h.finalize()),
        }
    }
}

/// The hash of `data`.
pub fn hash_bytes(algo: Algo, data: &[u8]) -> String {
    let mut h = Hasher::new(algo);
    h.update(data);
    h.finish()
}

/// The hash of the file at `path`.
pub fn hash_file(algo: Algo, path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut h = Hasher::new(algo);
    let mut buf = vec![0u8; 256 << 10];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finish())
}

/// Whether `path` is a file of `size` bytes with the hash in `check`.
pub fn file_matches(path: &Path, check: &Check, size: u64) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.len() == size)
        && hash_file(check.algo, path).is_ok_and(|h| h == check.hex)
}

// --- HTTP ------------------------------------------------------------------

/// What the worker tells the screen.
#[derive(Debug, Clone)]
pub enum Event {
    /// The work is known: total bytes to move and how many files.
    Started {
        total: u64,
        files: usize,
    },
    /// A file was started.
    File(String),
    /// Bytes done (negative: a try failed and its bytes are taken back).
    Bytes(i64),
    /// A step that is not a transfer ("Unpacking…").
    Stage(String),
    Finished,
    Failed(SetupError),
    Cancelled,
}

/// Cancel flag and event channel, shared by the worker's threads.
#[derive(Clone)]
pub struct Control {
    pub tx: Sender<Event>,
    pub cancel: Arc<AtomicBool>,
}

impl Control {
    fn send(&self, event: Event) {
        let _ = self.tx.send(event);
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

fn host_of(url: &str) -> String {
    url.split("://")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or(url)
        .to_owned()
}

fn map_error(url: &str, err: ureq::Error) -> SetupError {
    match err {
        ureq::Error::StatusCode(status) => SetupError::Status {
            url: url.to_owned(),
            status,
        },
        ureq::Error::HostNotFound
        | ureq::Error::ConnectionFailed
        | ureq::Error::Timeout(_)
        | ureq::Error::Io(_) => SetupError::Offline {
            host: host_of(url),
            detail: err.to_string(),
        },
        other => SetupError::Io(other.to_string()),
    }
}

/// A blocking HTTP client with this game's User-Agent, timeouts and retries.
#[derive(Clone)]
pub struct Http {
    agent: ureq::Agent,
    policy: Policy,
}

impl Http {
    pub fn new(ep: &Endpoints, policy: Policy) -> Self {
        let tls = ureq::tls::TlsConfig::builder()
            .root_certs(ureq::tls::RootCerts::PlatformVerifier)
            .build();
        let config = ureq::Agent::config_builder()
            .user_agent(ep.user_agent.clone())
            .tls_config(tls)
            .timeout_resolve(Some(policy.connect))
            .timeout_connect(Some(policy.connect))
            .timeout_recv_response(Some(policy.response))
            .timeout_per_read(Some(policy.read))
            .max_idle_connections(policy.connections * 2)
            .max_idle_connections_per_host(policy.connections * 2)
            .build();
        Self {
            agent: ureq::Agent::new_with_config(config),
            policy,
        }
    }

    /// Runs `attempt` up to the policy's tries, waiting longer each time
    /// after a failure that can pass.
    fn retrying<T>(
        &self,
        what: &str,
        cancel: Option<&AtomicBool>,
        mut attempt: impl FnMut(u32) -> Result<T>,
    ) -> Result<T> {
        let mut wait = self.policy.backoff;
        let mut tries = 0;
        loop {
            tries += 1;
            if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                return Err(SetupError::Cancelled);
            }
            match attempt(tries) {
                Ok(value) => return Ok(value),
                Err(err) if err.retryable() && tries < self.policy.attempts => {
                    log::warn!("{what}: {err}; trying again in {wait:?}");
                    std::thread::sleep(wait);
                    wait *= 2;
                }
                Err(SetupError::Checksum {
                    file,
                    expected,
                    got,
                    ..
                }) => {
                    return Err(SetupError::Checksum {
                        file,
                        expected,
                        got,
                        attempts: tries,
                    });
                }
                Err(err) => return Err(err),
            }
        }
    }

    /// A small file in memory (at most `limit` bytes), checked if `check`.
    pub fn bytes(&self, url: &str, check: Option<Check>, limit: u64) -> Result<Vec<u8>> {
        self.retrying(url, None, |tries| {
            let mut response = self.agent.get(url).call().map_err(|e| map_error(url, e))?;
            let mut data = Vec::new();
            response
                .body_mut()
                .with_config()
                .limit(limit)
                .reader()
                .read_to_end(&mut data)
                .map_err(|e| SetupError::Io(format!("reading {url}: {e}")))?;
            if let Some(check) = &check {
                let got = hash_bytes(check.algo, &data);
                if got != check.hex {
                    return Err(SetupError::Checksum {
                        file: url.to_owned(),
                        expected: check.hex.clone(),
                        got,
                        attempts: tries,
                    });
                }
            }
            Ok(data)
        })
    }

    /// Downloads `url` to `dest` (through `dest.tmp`), checking size and
    /// hash, reporting bytes as they arrive. A file already there with the
    /// right hash is kept (and counted as done at once).
    fn file(&self, ctl: &Control, task: &Task) -> Result<()> {
        if file_matches(&task.dest, &task.check, task.size) {
            ctl.send(Event::Bytes(task.size as i64));
            return Ok(());
        }
        if let Some(parent) = task.dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = PathBuf::from(format!("{}.tmp", task.dest.display()));
        let result = self.retrying(&task.label, Some(&ctl.cancel), |tries| {
            let mut sent: u64 = 0;
            let outcome = self.attempt(ctl, task, &tmp, tries, &mut sent);
            if outcome.is_err() {
                ctl.send(Event::Bytes(-(sent as i64)));
            }
            outcome
        });
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        result
    }

    fn attempt(
        &self,
        ctl: &Control,
        task: &Task,
        tmp: &Path,
        tries: u32,
        sent: &mut u64,
    ) -> Result<()> {
        let url = &task.url;
        let mut response = self.agent.get(url).call().map_err(|e| map_error(url, e))?;
        let mut reader = response.body_mut().as_reader();
        let mut out = std::fs::File::create(tmp)?;
        let mut hasher = Hasher::new(task.check.algo);
        let mut buf = vec![0u8; 64 << 10];
        let (mut total, mut pending) = (0u64, 0u64);
        loop {
            if ctl.cancelled() {
                return Err(SetupError::Cancelled);
            }
            let n = reader
                .read(&mut buf)
                .map_err(|e| SetupError::Io(format!("{} was cut off: {e}", task.label)))?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            out.write_all(&buf[..n])?;
            total += n as u64;
            pending += n as u64;
            if pending >= 128 << 10 {
                ctl.send(Event::Bytes(pending as i64));
                *sent += pending;
                pending = 0;
            }
        }
        if pending > 0 {
            ctl.send(Event::Bytes(pending as i64));
            *sent += pending;
        }
        out.flush()?;
        drop(out);
        let got = hasher.finish();
        if got != task.check.hex || total != task.size {
            return Err(SetupError::Checksum {
                file: task.label.clone(),
                expected: task.check.hex.clone(),
                got: if total != task.size {
                    format!("{total} bytes instead of {}", task.size)
                } else {
                    got
                },
                attempts: tries,
            });
        }
        std::fs::rename(tmp, &task.dest)?;
        Ok(())
    }
}

// --- The job ---------------------------------------------------------------

/// One file to fetch.
#[derive(Debug, Clone)]
struct Task {
    url: String,
    dest: PathBuf,
    check: Check,
    size: u64,
    label: String,
}

/// What to install and where.
#[derive(Debug, Clone)]
pub struct Request {
    /// The vanilla files to install (None: leave vanilla alone).
    pub vanilla: Option<Arc<Plan>>,
    /// The packs and jars to install (entries of [`manifest::extras`]; tests
    /// pass their own).
    pub extras: Vec<Extra>,
    /// The user asset layer (`…/Rustcraft/assets`).
    pub target: PathBuf,
    pub endpoints: Endpoints,
    pub policy: Policy,
}

/// What `installed.ron` records.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stamp {
    pub vanilla: Option<VanillaStamp>,
    pub extras: Vec<ExtraStamp>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VanillaStamp {
    pub version: String,
    pub files: usize,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtraStamp {
    pub name: String,
    /// The Modrinth version id.
    pub version: String,
    pub file: String,
}

impl Stamp {
    pub fn read(target: &Path) -> Self {
        std::fs::read_to_string(target.join(STAMP_FILE))
            .ok()
            .and_then(|text| ron::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn write(&self, target: &Path) -> Result<()> {
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(|e| SetupError::Format(e.to_string()))?;
        std::fs::create_dir_all(target)?;
        std::fs::write(target.join(STAMP_FILE), text)?;
        Ok(())
    }
}

/// The running job.
pub struct Handle {
    pub rx: Receiver<Event>,
    cancel: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Handle {
    /// Stops soon; the `.partial` folders stay for the next run.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn is_running(&self) -> bool {
        self.thread.as_ref().is_some_and(|t| !t.is_finished())
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.cancel();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Starts the job on its own thread.
pub fn start(req: Request) -> Handle {
    let (tx, rx) = crossbeam_channel::unbounded();
    let cancel = Arc::new(AtomicBool::new(false));
    let ctl = Control {
        tx,
        cancel: cancel.clone(),
    };
    let thread = std::thread::Builder::new()
        .name("setup-download".into())
        .spawn(move || run(&req, &ctl))
        .ok();
    Handle { rx, cancel, thread }
}

/// Like [`start`], but asks Mojang for the vanilla plan first when
/// `vanilla` is set (Game Files' Re-download and the style prompt).
pub fn start_full(
    vanilla: bool,
    extras: Vec<Extra>,
    target: PathBuf,
    endpoints: Endpoints,
    policy: Policy,
) -> Handle {
    let (tx, rx) = crossbeam_channel::unbounded();
    let cancel = Arc::new(AtomicBool::new(false));
    let ctl = Control {
        tx,
        cancel: cancel.clone(),
    };
    let thread = std::thread::Builder::new()
        .name("setup-download".into())
        .spawn(move || {
            let plan = if vanilla {
                match vanilla::resolve(&Http::new(&endpoints, policy), &endpoints) {
                    Ok(plan) => Some(Arc::new(plan)),
                    Err(err) => return ctl.send(Event::Failed(err)),
                }
            } else {
                None
            };
            run(
                &Request {
                    vanilla: plan,
                    extras,
                    target,
                    endpoints,
                    policy,
                },
                &ctl,
            );
        })
        .ok();
    Handle { rx, cancel, thread }
}

/// Asks Mojang what a vanilla download is made of, on its own thread.
pub fn measure(ep: Endpoints, policy: Policy) -> Receiver<Result<Plan>> {
    let (tx, rx) = crossbeam_channel::bounded(1);
    std::thread::spawn(move || {
        let _ = tx.send(vanilla::resolve(&Http::new(&ep, policy), &ep));
    });
    rx
}

/// Runs the job on this thread and sends the final event
/// (`Finished`, `Failed` or `Cancelled`).
pub fn run(req: &Request, ctl: &Control) {
    match install(req, ctl) {
        Ok(()) => ctl.send(Event::Finished),
        Err(SetupError::Cancelled) => ctl.send(Event::Cancelled),
        Err(err) => ctl.send(Event::Failed(err)),
    }
}

fn vanilla_partial(target: &Path) -> PathBuf {
    target.join("vanilla.partial")
}

fn downloads_partial(target: &Path) -> PathBuf {
    target.join("downloads.partial")
}

fn install(req: &Request, ctl: &Control) -> Result<()> {
    let http = Http::new(&req.endpoints, req.policy);
    let target = &req.target;
    std::fs::create_dir_all(target)?;
    let mut tasks: Vec<Task> = Vec::new();

    // The packs and jars not already installed with the right hash.
    let wanted: Vec<&Extra> = req.extras.iter().collect();
    let todo: Vec<&Extra> = wanted
        .iter()
        .copied()
        .filter(|e| {
            !file_matches(
                &target.join("downloads").join(e.rel_path()),
                &Check::sha512(&e.sha512),
                e.size,
            )
        })
        .collect();
    let urls = super::modrinth::resolve_urls(&http, &req.endpoints, &todo)?;
    for e in &todo {
        tasks.push(Task {
            url: urls[&e.file].clone(),
            dest: downloads_partial(target).join(e.rel_path()),
            check: Check::sha512(&e.sha512),
            size: e.size,
            label: e.name.clone(),
        });
    }

    let mc = vanilla_partial(target).join("assets/minecraft");
    if let Some(plan) = &req.vanilla {
        tasks.push(Task {
            url: plan.client.url.clone(),
            dest: vanilla_partial(target).join("client.jar.download"),
            check: Check::sha1(&plan.client.sha1),
            size: plan.client.size,
            label: "client.jar".into(),
        });
        for o in &plan.objects {
            tasks.push(Task {
                url: vanilla::object_url(&req.endpoints, &o.hash),
                dest: mc.join(&o.rel),
                check: Check::sha1(&o.hash),
                size: o.size,
                label: o.rel.clone(),
            });
        }
    }
    let total: u64 = tasks.iter().map(|t| t.size).sum();
    ctl.send(Event::Started {
        total,
        files: tasks.len(),
    });
    // Big files first, so the connections stay busy to the end.
    tasks.sort_by_key(|t| std::cmp::Reverse(t.size));
    pool(&http, ctl, &tasks, req.policy.connections)?;

    if let Some(plan) = &req.vanilla {
        finish_vanilla(ctl, target, plan)?;
    }
    if !todo.is_empty() {
        ctl.send(Event::Stage("Moving the packs and mods into place".into()));
        for e in &todo {
            let from = downloads_partial(target).join(e.rel_path());
            let to = target.join("downloads").join(e.rel_path());
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::rename(&from, &to)?;
        }
        let _ = std::fs::remove_dir_all(downloads_partial(target));
    }

    let mut stamp = Stamp::read(target);
    if let Some(plan) = &req.vanilla {
        stamp.vanilla = Some(VanillaStamp {
            version: plan.version.clone(),
            files: plan.objects.len(),
            bytes: plan.download_bytes(),
        });
    }
    for e in &wanted {
        stamp.extras.retain(|s| s.file != e.file);
        stamp.extras.push(ExtraStamp {
            name: e.name.clone(),
            version: e.version.clone(),
            file: e.file.clone(),
        });
    }
    stamp.write(target)?;
    layers::refresh();
    Ok(())
}

/// Runs `tasks` on `connections` threads. The first error stops the rest.
fn pool(http: &Http, ctl: &Control, tasks: &[Task], connections: usize) -> Result<()> {
    let next = AtomicUsize::new(0);
    let failure: Mutex<Option<SetupError>> = Mutex::new(None);
    std::thread::scope(|scope| {
        for _ in 0..connections.clamp(1, tasks.len().max(1)) {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(task) = tasks.get(i) else { return };
                    if failure.lock().is_ok_and(|f| f.is_some()) || ctl.cancelled() {
                        return;
                    }
                    ctl.send(Event::File(task.label.clone()));
                    if let Err(err) = http.file(ctl, task)
                        && let Ok(mut f) = failure.lock()
                    {
                        f.get_or_insert(err);
                        return;
                    }
                }
            });
        }
    });
    if let Ok(mut f) = failure.lock()
        && let Some(err) = f.take()
    {
        return Err(err);
    }
    if ctl.cancelled() {
        return Err(SetupError::Cancelled);
    }
    Ok(())
}

/// Unpacks the jar over the objects, checks the layout, writes the checksum
/// list and moves `vanilla.partial` to `vanilla`.
fn finish_vanilla(ctl: &Control, target: &Path, plan: &Plan) -> Result<()> {
    let partial = vanilla_partial(target);
    let mc = partial.join("assets/minecraft");
    ctl.send(Event::Stage(
        "Unpacking Minecraft's textures and models".into(),
    ));
    let skip: HashSet<&str> = plan.objects.iter().map(|o| o.rel.as_str()).collect();
    vanilla::extract_client_jar(&partial.join("client.jar.download"), &mc, &skip)?;
    std::fs::remove_file(partial.join("client.jar.download"))?;
    ctl.send(Event::Stage("Checking the files".into()));
    if let Err(problems) = layers::check_vanilla(&partial) {
        let _ = std::fs::remove_dir_all(&partial);
        return Err(SetupError::Format(format!(
            "the downloaded files are incomplete ({}); try again",
            problems.join("; ")
        )));
    }
    write_sums(&partial)?;
    swap_into_place(&partial, &target.join("vanilla"))
}

/// Replaces `to` with `from` (a rename, so a reader sees one or the other).
pub fn swap_into_place(from: &Path, to: &Path) -> Result<()> {
    let old = PathBuf::from(format!("{}.old", to.display()));
    let _ = std::fs::remove_dir_all(&old);
    if to.exists() {
        std::fs::rename(to, &old)?;
    }
    std::fs::rename(from, to)?;
    let _ = std::fs::remove_dir_all(&old);
    Ok(())
}

// --- The checksum list (Game Files → Verify) --------------------------------

/// The list `finish_vanilla` writes into `vanilla/`: `sha1  path` per file.
pub const SUMS_FILE: &str = "files.sha1";

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn write_sums(vanilla: &Path) -> Result<()> {
    let mut files = Vec::new();
    walk(&vanilla.join("assets"), &mut files);
    files.sort();
    let mut text = String::new();
    for file in files {
        let rel = file.strip_prefix(vanilla).unwrap_or(&file);
        text += &format!("{}  {}\n", hash_file(Algo::Sha1, &file)?, rel.display());
    }
    std::fs::write(vanilla.join(SUMS_FILE), text)?;
    Ok(())
}

/// Checks every file of `vanilla` against its checksum list. `None`: no list
/// (made by the script). `Some(bad)`: the files that are missing or changed.
pub fn verify_vanilla(vanilla: &Path) -> Option<Vec<String>> {
    let text = std::fs::read_to_string(vanilla.join(SUMS_FILE)).ok()?;
    let mut bad = Vec::new();
    for line in text.lines() {
        let Some((sum, rel)) = line.split_once("  ") else {
            continue;
        };
        if hash_file(Algo::Sha1, &vanilla.join(rel)).is_ok_and(|h| h == sum) {
            continue;
        }
        bad.push(rel.to_owned());
    }
    Some(bad)
}

/// The pinned files of `downloads` whose sha512 is wrong.
pub fn damaged_extras(downloads: &Path) -> Vec<&'static Extra> {
    manifest::extras()
        .iter()
        .filter(|e| {
            let path = downloads.join(e.rel_path());
            path.exists() && !file_matches(&path, &Check::sha512(&e.sha512), e.size)
        })
        .collect()
}

/// The size of everything under `dir`.
pub fn dir_size(dir: &Path) -> u64 {
    let mut files = Vec::new();
    walk(dir, &mut files);
    files
        .iter()
        .filter_map(|f| std::fs::metadata(f).ok())
        .map(|m| m.len())
        .sum()
}

// --- A folder the player already has ----------------------------------------

/// The folder holding `assets/minecraft` inside `picked`: the folder itself,
/// its `vanilla/`, or `picked` being the `assets` (or `assets/minecraft`)
/// folder.
pub fn find_vanilla_in(picked: &Path) -> Option<PathBuf> {
    let candidates = [
        picked.to_owned(),
        picked.join("vanilla"),
        picked.parent()?.to_owned(),
        picked.parent()?.parent()?.to_owned(),
    ];
    candidates
        .into_iter()
        .find(|c| c.join("assets/minecraft/textures").is_dir())
}

fn copy_tree(from: &Path, to: &Path, ctl: &Control) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)?.filter_map(Result::ok) {
        if ctl.cancelled() {
            return Err(SetupError::Cancelled);
        }
        let name = entry.file_name();
        if name == ".git" || name == ".DS_Store" {
            continue;
        }
        let (src, dst) = (entry.path(), to.join(&name));
        if src.is_dir() {
            copy_tree(&src, &dst, ctl)?;
        } else {
            let bytes = std::fs::copy(&src, &dst)?;
            ctl.send(Event::Bytes(bytes as i64));
        }
    }
    Ok(())
}

/// Copies a vanilla layer the player already has into the user layer, after
/// checking it like `scripts/check-vanilla.sh`.
pub fn import_folder(picked: &Path, target: &Path, ctl: &Control) -> Result<()> {
    let source = find_vanilla_in(picked).ok_or_else(|| {
        SetupError::Format(format!(
            "{} has no assets/minecraft folder with textures",
            picked.display()
        ))
    })?;
    if let Err(problems) = layers::check_vanilla(&source) {
        return Err(SetupError::Format(format!(
            "{} isn't complete: {}",
            source.display(),
            problems.join("; ")
        )));
    }
    let partial = vanilla_partial(target);
    let _ = std::fs::remove_dir_all(&partial);
    let total = dir_size(&source.join("assets"));
    ctl.send(Event::Started { total, files: 0 });
    ctl.send(Event::Stage("Copying your folder".into()));
    copy_tree(&source.join("assets"), &partial.join("assets"), ctl)?;
    layers::check_vanilla(&partial).map_err(|p| SetupError::Format(p.join("; ")))?;
    write_sums(&partial)?;
    swap_into_place(&partial, &target.join("vanilla"))?;
    let mut stamp = Stamp::read(target);
    stamp.vanilla = Some(VanillaStamp {
        version: format!("copied from {}", source.display()),
        files: 0,
        bytes: total,
    });
    stamp.write(target)?;
    layers::refresh();
    Ok(())
}

/// Runs [`import_folder`] on its own thread, like [`start`].
pub fn start_import(picked: PathBuf, target: PathBuf) -> Handle {
    let (tx, rx) = crossbeam_channel::unbounded();
    let cancel = Arc::new(AtomicBool::new(false));
    let ctl = Control {
        tx,
        cancel: cancel.clone(),
    };
    let thread = std::thread::spawn(move || match import_folder(&picked, &target, &ctl) {
        Ok(()) => ctl.send(Event::Finished),
        Err(SetupError::Cancelled) => ctl.send(Event::Cancelled),
        Err(err) => ctl.send(Event::Failed(err)),
    });
    Handle {
        rx,
        cancel,
        thread: Some(thread),
    }
}
