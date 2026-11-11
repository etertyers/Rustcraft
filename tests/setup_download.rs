//! First-launch downloads (M74) against a tiny local HTTP server that plays
//! Mojang's launcher endpoints and Modrinth: a fake version manifest,
//! version JSON, client.jar, asset index, objects and Modrinth answers.

use rustcraft::setup::download::{self, Algo, Control, Event, Policy, Request, Stamp};
use rustcraft::setup::layers;
use rustcraft::setup::manifest::{Endpoints, Extra, Kind, StyleTag};
use rustcraft::setup::{SetupError, vanilla};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// --- The server -------------------------------------------------------------

#[derive(Default)]
struct State {
    routes: HashMap<String, Vec<u8>>,
    /// Requests seen, in order.
    log: Vec<String>,
    /// Paths that are served damaged this many more times.
    corrupt: HashMap<String, usize>,
    /// After this many successful object responses, objects answer 500.
    objects_before_failing: Option<usize>,
    objects_served: usize,
}

struct Server {
    base: String,
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
}

impl Server {
    fn start(routes: HashMap<String, Vec<u8>>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let state = Arc::new(Mutex::new(State {
            routes,
            ..Default::default()
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let (s, st) = (state.clone(), stop.clone());
        std::thread::spawn(move || {
            while !st.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let s = s.clone();
                        std::thread::spawn(move || serve(stream, &s));
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(2)),
                }
            }
        });
        Self { base, state, stop }
    }

    fn endpoints(&self) -> Endpoints {
        Endpoints::local(&self.base)
    }

    fn requests(&self) -> Vec<String> {
        self.state.lock().unwrap().log.clone()
    }

    fn clear_log(&self) {
        self.state.lock().unwrap().log.clear();
    }

    fn count(&self, prefix: &str) -> usize {
        self.requests()
            .iter()
            .filter(|r| r.starts_with(prefix))
            .count()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn serve(mut stream: TcpStream, state: &Mutex<State>) {
    let _ = stream.set_nonblocking(false);
    let mut buf = [0u8; 4096];
    let mut request = Vec::new();
    while !request.windows(4).any(|w| w == b"\r\n\r\n") {
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => return,
            Ok(n) => request.extend_from_slice(&buf[..n]),
        }
    }
    let text = String::from_utf8_lossy(&request);
    let target = text.split_whitespace().nth(1).unwrap_or("/").to_owned();
    let path = target.split('?').next().unwrap().to_owned();
    let (status, body) = {
        let mut st = state.lock().unwrap();
        st.log.push(path.clone());
        let is_object = path.starts_with("/objects/");
        if is_object && let Some(limit) = st.objects_before_failing {
            if st.objects_served >= limit {
                (500, b"fail".to_vec())
            } else {
                st.objects_served += 1;
                (200, st.routes.get(&path).cloned().unwrap_or_default())
            }
        } else if let Some(body) = st.routes.get(&path).cloned() {
            match st.corrupt.get_mut(&path) {
                Some(left) if *left > 0 => {
                    *left -= 1;
                    let mut bad = body;
                    if let Some(first) = bad.first_mut() {
                        *first ^= 0xff;
                    }
                    (200, bad)
                }
                _ => (200, body),
            }
        } else {
            (404, b"nope".to_vec())
        }
    };
    let head = format!(
        "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&body);
}

// --- Fake Mojang and Modrinth data --------------------------------------------

fn sha1(data: &[u8]) -> String {
    download::hash_bytes(Algo::Sha1, data)
}

fn sha512(data: &[u8]) -> String {
    download::hash_bytes(Algo::Sha512, data)
}

struct Fake {
    routes: HashMap<String, Vec<u8>>,
    sound_paths: Vec<String>,
    extras: Vec<Extra>,
}

fn jar() -> Vec<u8> {
    use zip::write::SimpleFileOptions;
    let mut out = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut out));
        let opts =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let files: [(&str, &[u8]); 10] = [
            ("assets/minecraft/textures/block/stone.png", b"stone"),
            ("assets/minecraft/textures/font/ascii.png", b"ascii page"),
            (
                "assets/minecraft/textures/gui/title/background/panorama_0.png",
                b"STUB",
            ),
            ("assets/minecraft/models/block/stone.json", b"{}"),
            ("assets/minecraft/blockstates/stone.json", b"{}"),
            ("assets/minecraft/items/stone.json", b"{}"),
            ("assets/minecraft/lang/en_us.json", b"{}"),
            (
                "assets/minecraft/font/default.json",
                br#"{"providers":[{"type":"bitmap","file":"minecraft:font/ascii.png"}]}"#,
            ),
            ("assets/minecraft/font/include/unifont.json", b"{}"),
            ("assets/minecraft/shaders/core/panorama.fsh", b"not kept"),
        ];
        for (name, data) in files {
            zip.start_file(name, opts).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap();
    }
    out
}

fn fake(extra_count: usize) -> Fake {
    let mut routes: HashMap<String, Vec<u8>> = HashMap::new();
    let sound_paths: Vec<String> = (0..12)
        .map(|i| format!("sounds/ambient/s{i}.ogg"))
        .collect();
    let sounds_json = {
        let names: Vec<String> = (0..12).map(|i| format!("\"ambient/s{i}\"")).collect();
        format!("{{\"ambient.test\":{{\"sounds\":[{}]}}}}", names.join(",")).into_bytes()
    };
    let mut objects: Vec<(String, Vec<u8>)> = sound_paths
        .iter()
        .enumerate()
        .map(|(i, p)| {
            (
                format!("minecraft/{p}"),
                format!("sound number {i}").into_bytes(),
            )
        })
        .collect();
    objects.push(("minecraft/sounds.json".into(), sounds_json));
    objects.push((
        "minecraft/textures/gui/title/background/panorama_0.png".into(),
        b"REAL PANORAMA".to_vec(),
    ));
    objects.push((
        "minecraft/font/include/unifont.json".into(),
        br#"{"providers":[]}"#.to_vec(),
    ));
    objects.push(("minecraft/lang/de_de.json".into(), b"{}".to_vec()));
    objects.push(("icons/icon_16x16.png".into(), b"icon".to_vec()));
    let mut index = String::from("{\"objects\":{");
    for (i, (name, data)) in objects.iter().enumerate() {
        let hash = sha1(data);
        if i > 0 {
            index.push(',');
        }
        index += &format!("\"{name}\":{{\"hash\":\"{hash}\",\"size\":{}}}", data.len());
        routes.insert(format!("/objects/{}/{}", &hash[..2], hash), data.clone());
    }
    index += "}}";
    let index = index.into_bytes();
    routes.insert("/index/34.json".into(), index.clone());
    let jar = jar();
    routes.insert("/client.jar".into(), jar.clone());
    let base = "BASE"; // replaced when the server's address is known
    let version = format!(
        "{{\"downloads\":{{\"client\":{{\"sha1\":\"{}\",\"size\":{},\"url\":\"{base}/client.jar\"}}}},\"assetIndex\":{{\"id\":\"34\",\"sha1\":\"{}\",\"size\":{},\"url\":\"{base}/index/34.json\"}}}}",
        sha1(&jar),
        jar.len(),
        sha1(&index),
        index.len()
    );
    routes.insert("/v/26.3.json".into(), version.into_bytes());
    let mut extras = Vec::new();
    let mut versions = Vec::new();
    for i in 0..extra_count {
        let data: Vec<u8> = format!("extra file {i}").repeat(40).into_bytes();
        let kind = if i % 2 == 0 { Kind::Pack } else { Kind::Mod };
        let file = format!("extra-{i}.{}", if i % 2 == 0 { "zip" } else { "jar" });
        extras.push(Extra {
            name: format!("Extra {i}"),
            kind,
            project: format!("proj{i}"),
            version: format!("ver{i}"),
            file: file.clone(),
            size: data.len() as u64,
            sha512: sha512(&data),
            license: "MIT".into(),
            author: "Someone".into(),
            page: format!("https://modrinth.com/mod/extra-{i}"),
            styles: vec![StyleTag::Barebones],
        });
        routes.insert(format!("/cdn/{file}"), data.clone());
        versions.push(format!(
            "{{\"id\":\"ver{i}\",\"files\":[{{\"url\":\"{base}/cdn/{file}\",\"hashes\":{{\"sha512\":\"{}\"}}}}]}}",
            sha512(&data)
        ));
    }
    routes.insert(
        "/modrinth/v2/versions".into(),
        format!("[{}]", versions.join(",")).into_bytes(),
    );
    Fake {
        routes,
        sound_paths,
        extras,
    }
}

/// Starts a server for `fake`, with the `BASE` placeholder filled in and
/// the version manifest added.
fn serve_fake(mut fake: Fake) -> (Server, Vec<Extra>, Vec<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    // The server picks its own port, so start it first with the routes
    // that don't mention the address, then add the ones that do.
    let server = Server::start(HashMap::new());
    let base = server.base.clone().replace("BASE", &base);
    for (path, body) in fake.routes.iter_mut() {
        if path.ends_with(".json") || path.contains("modrinth") {
            *body = String::from_utf8(body.clone())
                .map(|t| t.replace("BASE", &base).into_bytes())
                .unwrap_or_else(|_| body.clone());
        }
    }
    let version = fake.routes["/v/26.3.json"].clone();
    // A newer snapshot first and an older release after, so a lookup by
    // exact id has to skip them.
    let manifest = format!(
        "{{\"versions\":[{{\"id\":\"26.4-snapshot-1\",\"url\":\"{base}/v/none.json\",\"sha1\":\"00\"}},{{\"id\":\"26.3\",\"url\":\"{base}/v/26.3.json\",\"sha1\":\"{}\"}}]}}",
        sha1(&version)
    );
    fake.routes.insert(
        "/mc/game/version_manifest_v2.json".into(),
        manifest.into_bytes(),
    );
    server.state.lock().unwrap().routes = fake.routes;
    (server, fake.extras, fake.sound_paths)
}

fn quick() -> Policy {
    Policy {
        attempts: 3,
        backoff: Duration::from_millis(5),
        connect: Duration::from_millis(500),
        response: Duration::from_millis(400),
        read: Duration::from_millis(400),
        connections: 4,
    }
}

fn temp(name: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "rc-setup-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn files_under(dir: &Path) -> Vec<String> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, root, out);
            } else {
                out.push(p.strip_prefix(root).unwrap().to_string_lossy().into_owned());
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

fn install(
    server: &Server,
    target: &Path,
    extras: Vec<Extra>,
    with_vanilla: bool,
    policy: Policy,
) -> (Result<(), SetupError>, Vec<Event>) {
    let ep = server.endpoints();
    let plan = with_vanilla.then(|| {
        Arc::new(vanilla::resolve(&download::Http::new(&ep, policy), &ep).expect("measure"))
    });
    let (tx, rx) = crossbeam_channel::unbounded();
    let ctl = Control {
        tx,
        cancel: Arc::new(AtomicBool::new(false)),
    };
    download::run(
        &Request {
            vanilla: plan,
            extras,
            target: target.to_owned(),
            endpoints: ep,
            policy,
        },
        &ctl,
    );
    drop(ctl);
    let events: Vec<Event> = rx.try_iter().collect();
    let result = match events.last() {
        Some(Event::Finished) => Ok(()),
        Some(Event::Failed(e)) => Err(e.clone()),
        Some(Event::Cancelled) => Err(SetupError::Cancelled),
        other => panic!("no final event: {other:?}"),
    };
    (result, events)
}

// --- Tests -------------------------------------------------------------------

#[test]
fn a_fresh_download_makes_the_script_layout_and_the_stamp() {
    let (server, extras, sounds) = serve_fake(fake(3));
    let target = temp("fresh");
    let (result, events) = install(&server, &target, extras.clone(), true, quick());
    result.unwrap();

    let vanilla_dir = target.join("vanilla");
    let counts = layers::check_vanilla(&vanilla_dir).expect("a complete layer");
    assert_eq!(counts.sounds, sounds.len());
    let mc = vanilla_dir.join("assets/minecraft");
    // The jar's kept files, the index's files, the index winning over the
    // jar's stubs, and nothing outside the kept paths.
    assert_eq!(
        std::fs::read(mc.join("textures/block/stone.png")).unwrap(),
        b"stone"
    );
    assert_eq!(
        std::fs::read(mc.join("textures/gui/title/background/panorama_0.png")).unwrap(),
        b"REAL PANORAMA"
    );
    assert_eq!(
        std::fs::read_to_string(mc.join("font/include/unifont.json")).unwrap(),
        r#"{"providers":[]}"#
    );
    let listed = files_under(&vanilla_dir);
    assert!(
        !listed
            .iter()
            .any(|f| f.contains("shaders") || f.contains("de_de") || f.contains("icon_")),
        "{listed:?}"
    );
    assert!(listed.contains(&download::SUMS_FILE.to_owned()));
    // Packs and jars landed in downloads/ under their folders.
    assert_eq!(
        files_under(&target.join("downloads")),
        [
            "mods/extra-1.jar",
            "resourcepacks/extra-0.zip",
            "resourcepacks/extra-2.zip"
        ]
    );
    // No .partial folder is left, and the stamp says what is installed.
    let top = files_under(&target);
    assert!(
        !top.iter()
            .any(|f| f.contains("partial") || f.ends_with(".tmp")),
        "{top:?}"
    );
    let stamp = Stamp::read(&target);
    assert_eq!(stamp.vanilla.as_ref().unwrap().version, "26.3");
    assert_eq!(stamp.extras.len(), 3);
    assert_eq!(stamp.extras[0].version, "ver0");
    // Progress adds up to the total it announced.
    let total = events.iter().find_map(|e| match e {
        Event::Started { total, .. } => Some(*total),
        _ => None,
    });
    let done: i64 = events
        .iter()
        .filter_map(|e| {
            if let Event::Bytes(n) = e {
                Some(*n)
            } else {
                None
            }
        })
        .sum();
    assert_eq!(done as u64, total.unwrap());
    // Every check passes on the finished layer.
    assert_eq!(download::verify_vanilla(&vanilla_dir), Some(vec![]));
    assert!(download::damaged_extras(&target.join("downloads")).is_empty());
}

#[test]
fn a_damaged_file_is_retried_then_fails_with_a_clear_error() {
    let (server, _, sounds) = serve_fake(fake(0));
    // The first sound arrives damaged once: the retry makes it right.
    let first_hash = {
        let st = server.state.lock().unwrap();
        st.routes
            .keys()
            .find(|k| k.starts_with("/objects/"))
            .cloned()
            .unwrap()
    };
    server
        .state
        .lock()
        .unwrap()
        .corrupt
        .insert(first_hash.clone(), 1);
    let target = temp("retry");
    let (result, _) = install(&server, &target, vec![], true, quick());
    result.unwrap();
    assert!(server.count(&first_hash) >= 2, "fetched again");

    // Always damaged: fails after the policy's tries, naming the file.
    let target = temp("mismatch");
    server
        .state
        .lock()
        .unwrap()
        .corrupt
        .insert(first_hash.clone(), 100);
    server.clear_log();
    let (result, _) = install(&server, &target, vec![], true, quick());
    match result {
        Err(SetupError::Checksum {
            attempts,
            expected,
            got,
            ..
        }) => {
            assert_eq!(attempts, 3);
            assert_ne!(expected, got);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(server.count(&first_hash), 3, "tried {} times", 3);
    assert!(
        !target.join("vanilla").exists(),
        "nothing is moved into place"
    );
    let _ = sounds;
    let message = SetupError::Checksum {
        file: "x".into(),
        expected: "a".into(),
        got: "b".into(),
        attempts: 3,
    }
    .to_string();
    assert!(message.contains("damaged") && message.contains("3 tries"));
}

#[test]
fn an_interrupted_download_resumes_and_fetches_only_what_is_missing() {
    let (server, extras, _) = serve_fake(fake(2));
    let target = temp("resume");
    // The server starts failing after 6 objects.
    server.state.lock().unwrap().objects_before_failing = Some(6);
    let (result, _) = install(&server, &target, extras.clone(), true, quick());
    assert!(
        matches!(result, Err(SetupError::Status { status: 500, .. })),
        "{result:?}"
    );
    assert!(!target.join("vanilla").exists(), "not installed yet");
    let have: HashSet<String> = files_under(&target.join("vanilla.partial/assets/minecraft"))
        .into_iter()
        .collect();
    assert!(!have.is_empty(), "some objects arrived");

    // Healed: only what is missing is asked for, and the extras that
    // arrived are not fetched twice either.
    server.state.lock().unwrap().objects_before_failing = None;
    server.clear_log();
    let (result, _) = install(&server, &target, extras, true, quick());
    result.unwrap();
    let asked = server.requests();
    let objects_asked = asked.iter().filter(|r| r.starts_with("/objects/")).count();
    let objects_total = 12 + 3; // sounds + sounds.json, panorama, unifont.json
    assert_eq!(objects_asked, objects_total - have.len(), "{asked:?}");
    layers::check_vanilla(&target.join("vanilla")).unwrap();
}

#[test]
fn offline_gives_the_offline_error_and_does_not_hang() {
    // Nothing listens on this port.
    let port = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let ep = Endpoints::local(&format!("http://127.0.0.1:{port}"));
    let started = Instant::now();
    let err = vanilla::resolve(&download::Http::new(&ep, quick()), &ep).unwrap_err();
    assert!(matches!(err, SetupError::Offline { .. }), "{err:?}");
    assert!(err.to_string().contains("Is this Mac online?"));
    assert!(started.elapsed() < Duration::from_secs(5));

    // A server that accepts and never answers: the timeout ends it.
    let silent = TcpListener::bind("127.0.0.1:0").unwrap();
    let ep = Endpoints::local(&format!("http://{}", silent.local_addr().unwrap()));
    let started = Instant::now();
    let err = vanilla::resolve(&download::Http::new(&ep, quick()), &ep).unwrap_err();
    assert!(matches!(err, SetupError::Offline { .. }), "{err:?}");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    drop(silent);
}

#[test]
fn a_manifest_without_the_version_stops_with_a_clear_error() {
    let (server, _, _) = serve_fake(fake(0));
    server.state.lock().unwrap().routes.insert(
        "/mc/game/version_manifest_v2.json".into(),
        br#"{"versions":[{"id":"26.4-snapshot-1","url":"x","sha1":"y"}]}"#.to_vec(),
    );
    let ep = server.endpoints();
    let err = vanilla::resolve(&download::Http::new(&ep, quick()), &ep).unwrap_err();
    assert_eq!(err, SetupError::NoVersion("26.3".into()));
}

#[test]
fn packs_already_installed_are_not_fetched_again() {
    let (server, extras, _) = serve_fake(fake(2));
    let target = temp("skip");
    install(&server, &target, extras.clone(), false, quick())
        .0
        .unwrap();
    server.clear_log();
    install(&server, &target, extras.clone(), false, quick())
        .0
        .unwrap();
    assert_eq!(server.count("/cdn/"), 0, "{:?}", server.requests());
    // A damaged file is found, and fetched again.
    std::fs::write(target.join("downloads/mods/extra-1.jar"), b"broken").unwrap();
    assert_eq!(
        download::damaged_extras(&target.join("downloads")).len(),
        0,
        "only the pinned list is checked"
    );
    install(&server, &target, extras, false, quick()).0.unwrap();
    assert_eq!(server.count("/cdn/"), 1);
}

#[test]
fn the_user_layer_is_used_when_the_root_has_no_vanilla_and_ignored_when_complete() {
    let (server, _, _) = serve_fake(fake(0));
    let user = temp("user-layer");
    install(&server, &user, vec![], true, quick()).0.unwrap();

    // A root with no vanilla: the user layer's copy is used.
    let root = temp("root-empty");
    let chosen = layers::resolve(&root, Some(&user));
    assert_eq!(chosen.vanilla, user.join("vanilla"));
    assert!(chosen.vanilla_in_user_layer);
    assert!(!layers::vanilla_missing_in(&root, Some(&user)));

    // A root with its own complete copy: the user layer is ignored.
    let full = temp("root-full");
    copy_dir(&user.join("vanilla"), &full.join("vanilla"));
    let chosen = layers::resolve(&full, Some(&user));
    assert_eq!(chosen.vanilla, full.join("vanilla"));
    assert!(!chosen.vanilla_in_user_layer);

    // An incomplete root copy (the sounds are gone) yields to the user's.
    std::fs::remove_dir_all(full.join("vanilla/assets/minecraft/sounds")).unwrap();
    assert_eq!(
        layers::resolve(&full, Some(&user)).vanilla,
        user.join("vanilla")
    );

    // Nothing anywhere: setup is needed, and the root is reported.
    let nowhere = temp("nowhere");
    assert!(layers::vanilla_missing_in(&root, Some(&nowhere)));
    assert_eq!(
        layers::resolve(&root, Some(&nowhere)).vanilla,
        root.join("vanilla")
    );
}

#[test]
fn a_complete_install_shows_no_setup_screen() {
    let (server, _, _) = serve_fake(fake(0));
    let root = temp("complete");
    install(&server, &root, vec![], true, quick()).0.unwrap();
    assert!(!layers::vanilla_missing_in(&root, None));
}

#[test]
fn a_folder_the_player_has_is_checked_then_copied() {
    let (server, _, _) = serve_fake(fake(0));
    let source = temp("have-source");
    install(&server, &source, vec![], true, quick()).0.unwrap();
    let target = temp("have-target");
    let (tx, rx) = crossbeam_channel::unbounded();
    let ctl = Control {
        tx,
        cancel: Arc::new(AtomicBool::new(false)),
    };
    // Picking the `assets` folder or the vanilla folder both work.
    download::import_folder(&source.join("vanilla"), &target, &ctl).unwrap();
    layers::check_vanilla(&target.join("vanilla")).unwrap();
    assert!(rx.try_iter().any(|e| matches!(e, Event::Started { .. })));

    // A folder without the files is refused with the reason.
    let empty = temp("have-empty");
    let err = download::import_folder(&empty, &temp("have-t2"), &ctl).unwrap_err();
    assert!(err.to_string().contains("no assets/minecraft"), "{err}");
    // An incomplete one says what is missing.
    std::fs::remove_dir_all(source.join("vanilla/assets/minecraft/sounds")).unwrap();
    let err = download::import_folder(&source.join("vanilla"), &temp("have-t3"), &ctl).unwrap_err();
    assert!(err.to_string().contains("missing"), "{err}");
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let (s, d) = (e.path(), to.join(e.file_name()));
        if s.is_dir() {
            copy_dir(&s, &d);
        } else {
            std::fs::copy(&s, &d).unwrap();
        }
    }
}
