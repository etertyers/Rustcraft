//! The real thing: downloads Minecraft's files from Mojang into a temp
//! folder and compares them with `assets/vanilla` (made by
//! `scripts/fetch-vanilla-assets.sh`). Needs the network and about 420 MB;
//! run it with `cargo test --test setup_real -- --ignored --nocapture`.

use rustcraft::setup::download::{self, Control, Event, Policy, Request};
use rustcraft::setup::manifest::Endpoints;
use rustcraft::setup::{layers, vanilla};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

fn hashes(root: &Path) -> BTreeMap<String, String> {
    fn walk(dir: &Path, root: &Path, out: &mut BTreeMap<String, String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, root, out);
            } else {
                let name = p.file_name().unwrap().to_string_lossy();
                // The mirror's own index files, and the checksum list.
                if matches!(name.as_ref(), "_list.json" | "_all.json" | ".DS_Store") {
                    continue;
                }
                out.insert(
                    p.strip_prefix(root).unwrap().to_string_lossy().into_owned(),
                    download::hash_file(download::Algo::Sha1, &p).unwrap(),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

#[test]
#[ignore = "downloads about 420 MB from Mojang"]
fn the_real_download_matches_the_fetch_scripts_layout() {
    let reference = rustcraft::assets::repo_root().join("assets/vanilla/assets");
    if !reference.join("minecraft/font/default.json").is_file() {
        eprintln!("skipping: run scripts/fetch-vanilla-assets.sh first (assets/vanilla/assets)");
        return;
    }
    let target = std::env::temp_dir().join(format!("rc-setup-real-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&target);
    let ep = Endpoints::official();
    let policy = Policy::default();
    let plan = vanilla::resolve(&download::Http::new(&ep, policy), &ep).expect("Mojang's lists");
    println!(
        "version {}, client.jar {} B, {} objects, download {} B",
        plan.version,
        plan.client.size,
        plan.objects.len(),
        plan.download_bytes()
    );
    let (tx, rx) = crossbeam_channel::unbounded();
    let ctl = Control {
        tx,
        cancel: Arc::new(AtomicBool::new(false)),
    };
    let started = std::time::Instant::now();
    download::run(
        &Request {
            vanilla: Some(Arc::new(plan)),
            extras: vec![],
            target: target.clone(),
            endpoints: ep,
            policy,
        },
        &ctl,
    );
    drop(ctl);
    let events: Vec<Event> = rx.try_iter().collect();
    assert!(
        matches!(events.last(), Some(Event::Finished)),
        "{:?}",
        events.last()
    );
    println!("downloaded in {:.1?}", started.elapsed());

    layers::check_vanilla(&target.join("vanilla")).expect("complete");
    let ours = hashes(&target.join("vanilla/assets"));
    let theirs = hashes(&reference);
    let only_ours: Vec<_> = ours.keys().filter(|k| !theirs.contains_key(*k)).collect();
    let only_theirs: Vec<_> = theirs.keys().filter(|k| !ours.contains_key(*k)).collect();
    let different: Vec<_> = ours
        .iter()
        .filter(|(k, v)| theirs.get(*k).is_some_and(|t| t != *v))
        .map(|(k, _)| k)
        .collect();
    println!(
        "{} files here, {} in assets/vanilla; only here {}, only there {}, different {}",
        ours.len(),
        theirs.len(),
        only_ours.len(),
        only_theirs.len(),
        different.len()
    );
    assert!(only_ours.is_empty(), "{only_ours:?}");
    assert!(only_theirs.is_empty(), "{only_theirs:?}");
    assert!(different.is_empty(), "{different:?}");
    let _ = std::fs::remove_dir_all(&target);
}
