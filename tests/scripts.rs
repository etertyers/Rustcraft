//! The shell scripts that guard the build (M59): `check-vanilla.sh`,
//! `check-downloads.sh` and `check-bundle.sh`, run on fake layouts.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn temp(name: &str) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("rc-scripts-{name}-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(path: impl AsRef<Path>, text: &str) {
    let path = path.as_ref();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn run(script: &str, args: &[&Path]) -> Output {
    Command::new("sh")
        .arg(repo().join("scripts").join(script))
        .args(args)
        .output()
        .unwrap()
}

/// Runs `check-bundle.sh` from a copy in a fake repo (its own `scripts/` and
/// `assets/`), because the script renames the repo's `assets/` while it runs
/// and must not do that to the real one under other tests. Returns the fake
/// repo too, with `FAKE_REPO` set to it for the app's binary.
fn check_bundle(app: &Path, src: &Path) -> (Output, PathBuf) {
    let fake = temp("repo");
    fs::create_dir_all(fake.join("assets/data")).unwrap();
    write(fake.join("assets/data/blocks.ron"), "x");
    let script = fake.join("scripts/check-bundle.sh");
    write(
        &script,
        &fs::read_to_string(repo().join("scripts/check-bundle.sh")).unwrap(),
    );
    let out = Command::new("sh")
        .arg(&script)
        .arg(app)
        .arg(src)
        .env("FAKE_REPO", fs::canonicalize(&fake).unwrap())
        .output()
        .unwrap();
    (out, fake)
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// A small but complete vanilla layer: one of each folder, two sounds, a
/// font with one bitmap page and a reference to an include.
fn fake_vanilla(assets: &Path, with_sounds: bool, with_font: bool) {
    let mc = assets.join("vanilla/assets/minecraft");
    for file in [
        "textures/block/stone.png",
        "models/block/stone.json",
        "blockstates/stone.json",
        "items/stick.json",
        "lang/en_us.json",
    ] {
        write(mc.join(file), "x");
    }
    write(
        mc.join("sounds.json"),
        r#"{"block.stone.step": {"sounds": ["step/stone1", {"name": "step/stone2"}, {"name": "block.stone.hit", "type": "event"}]}}"#,
    );
    if with_sounds {
        write(mc.join("sounds/step/stone1.ogg"), "x");
        write(mc.join("sounds/step/stone2.ogg"), "x");
    }
    if with_font {
        write(
            mc.join("font/default.json"),
            r#"{"providers": [{"type": "reference", "id": "minecraft:include/default"}]}"#,
        );
        write(
            mc.join("font/include/default.json"),
            r#"{"providers": [{"type": "bitmap", "file": "minecraft:font/ascii.png", "ascent": 7, "chars": ["a"]}]}"#,
        );
        write(mc.join("textures/font/ascii.png"), "x");
    }
}

#[test]
fn a_complete_vanilla_layer_passes() {
    let assets = temp("vanilla-ok");
    fake_vanilla(&assets, true, true);
    let out = run("check-vanilla.sh", &[&assets]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(String::from_utf8_lossy(&out.stdout).contains("2 sound files"));
}

#[test]
fn a_vanilla_layer_with_no_sounds_fails_and_says_how_many() {
    let assets = temp("vanilla-no-sounds");
    fake_vanilla(&assets, false, true);
    let out = run("check-vanilla.sh", &[&assets]);
    assert!(!out.status.success());
    let message = stderr(&out);
    assert!(
        message.contains("missing 2 sound files") && message.contains("fetch-vanilla-assets.sh"),
        "{message}"
    );
}

#[test]
fn a_vanilla_layer_with_no_font_fails_and_names_it() {
    let assets = temp("vanilla-no-font");
    fake_vanilla(&assets, true, false);
    let out = run("check-vanilla.sh", &[&assets]);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("font/default.json"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn a_missing_font_page_is_named() {
    let assets = temp("vanilla-no-page");
    fake_vanilla(&assets, true, true);
    fs::remove_file(assets.join("vanilla/assets/minecraft/textures/font/ascii.png")).unwrap();
    let out = run("check-vanilla.sh", &[&assets]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("font pages"), "{}", stderr(&out));
}

#[test]
fn downloads_are_matched_by_name_ignoring_case_spaces_dashes_and_versions() {
    let dir = temp("downloads");
    for pack in [
        "Bare Bones 1.21.11.zip",
        "Bare Bones x Fresh Animations 1.10.4.zip",
        "Barebones+Leaves+Add-on+main.zip",
        "FreshAnimations_v1.11.2_26.1+.zip",
        "Patrix_26.3_32x_basic.zip",
    ] {
        write(dir.join("resourcepacks").join(pack), "x");
    }
    for jar in [
        "AmbientSounds_FABRIC_v6.3.11_mc26.3.jar",
        "PresenceFootsteps-1.14.0+26.3.jar",
        "cave_dust-3.1.0.jar",
        "make_bubbles_pop-0.3.4-fabric-mc26.2.x.jar",
        "notenoughanimations-fabric-1.12.7-mc26.3.jar",
        "particlerain-4.0.3+26.3-fabric.jar",
        "sound-physics-remastered-fabric-1.5.2+26.3.jar",
        "visuality-0.7.15+26.3.jar",
        "wakes-0.7.3+26.3.jar",
    ] {
        write(dir.join("mods").join(jar), "x");
    }
    let out = run("check-downloads.sh", &[&dir]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );

    // Take two away: it exits non-zero and gives a link for each.
    fs::remove_file(dir.join("resourcepacks/Patrix_26.3_32x_basic.zip")).unwrap();
    fs::remove_file(dir.join("mods/wakes-0.7.3+26.3.jar")).unwrap();
    let out = run("check-downloads.sh", &[&dir]);
    assert!(!out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("missing  pack     Patrix 32x"), "{text}");
    assert!(
        text.contains("https://modrinth.com/resourcepack/patrix-32x"),
        "{text}"
    );
    assert!(text.contains("missing  mod      Wakes"), "{text}");
    // Fresh Animations alone must not satisfy the two Bare Bones packs.
    let only = temp("downloads-fa");
    write(
        only.join("resourcepacks/FreshAnimations_v1.11.2_26.1+.zip"),
        "x",
    );
    let text = String::from_utf8_lossy(&run("check-downloads.sh", &[&only]).stdout).into_owned();
    assert!(text.contains("missing  pack     Bare Bones  "), "{text}");
    assert!(
        text.contains("found    pack     Fresh Animations"),
        "{text}"
    );
}

/// A fake app: copies of the source folders and a binary that prints and
/// exits as told.
fn fake_app(src: &Path, status: i32) -> PathBuf {
    let app = temp("app").join("Rustcraft.app");
    let res = app.join("Contents/Resources/assets");
    fs::create_dir_all(&res).unwrap();
    for folder in [
        "vanilla",
        "downloads",
        "textures",
        "data",
        "structures",
        "skins",
    ] {
        fs::create_dir_all(src.join(folder)).unwrap();
        assert!(
            Command::new("cp")
                .arg("-R")
                .arg(src.join(folder))
                .arg(&res)
                .status()
                .unwrap()
                .success()
        );
    }
    let bin = app.join("Contents/MacOS/rustcraft");
    write(
        &bin,
        &format!("#!/bin/sh\necho 'sound events: 1 in the table'\nexit {status}\n"),
    );
    Command::new("chmod").arg("+x").arg(&bin).status().unwrap();
    app
}

#[test]
fn the_bundle_check_passes_on_a_complete_layer_and_fails_naming_what_differs() {
    let src = temp("bundle-src");
    fake_vanilla(&src, true, true);
    write(src.join("textures/block/a.png"), "x");
    write(src.join("data/blocks.ron"), "x");
    let app = fake_app(&src, 0);
    let (out, _) = check_bundle(&app, &src);
    assert!(out.status.success(), "{}", stderr(&out));

    // A sound file left out of the app.
    fs::remove_file(
        app.join("Contents/Resources/assets/vanilla/assets/minecraft/sounds/step/stone1.ogg"),
    )
    .unwrap();
    let (out, _) = check_bundle(&app, &src);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("vanilla: "), "{}", stderr(&out));
}

#[test]
fn the_bundle_check_fails_when_the_apps_own_report_does() {
    let src = temp("bundle-src-bad");
    fake_vanilla(&src, true, true);
    let app = fake_app(&src, 1);
    let (out, _) = check_bundle(&app, &src);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("asset report found problems"),
        "{}",
        stderr(&out)
    );
}

fn complete_source(name: &str) -> PathBuf {
    let src = temp(name);
    fake_vanilla(&src, true, true);
    write(src.join("textures/block/a.png"), "x");
    write(src.join("data/blocks.ron"), "x");
    src
}

#[test]
fn the_bundle_check_fails_when_the_binary_holds_the_repos_path() {
    let src = complete_source("bundle-src-path");
    let app = fake_app(&src, 0);
    // The fake repo's path is only known once the check has made it, so the
    // binary reads it from the environment at run time and also contains the
    // path of the temp folder the repo lives in (its parent) plus the repo's
    // name: both are given away by `strings`.
    let fake = temp("repo-for-path");
    let canon = fs::canonicalize(&fake).unwrap();
    // A script that carries the path is enough for `strings`.
    write(
        app.join("Contents/MacOS/rustcraft"),
        &format!("#!/bin/sh\n# {}\nexit 0\n", canon.display()),
    );
    let script = canon.join("scripts/check-bundle.sh");
    write(
        &script,
        &fs::read_to_string(repo().join("scripts/check-bundle.sh")).unwrap(),
    );
    fs::create_dir_all(canon.join("assets")).unwrap();
    let out = Command::new("sh")
        .arg(&script)
        .arg(&app)
        .arg(&src)
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("contains this repo's path"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn the_bundle_check_runs_a_copy_with_the_repos_assets_out_of_reach() {
    let src = complete_source("bundle-src-hidden");
    let app = fake_app(&src, 0);
    // Fails if the repo's assets/ is visible while the app runs.
    write(
        app.join("Contents/MacOS/rustcraft"),
        "#!/bin/sh\n[ -e \"$FAKE_REPO/assets\" ] && { echo 'repo assets reachable' >&2; exit 1; }\necho 'sound events: 1 in the table'\nexit 0\n",
    );
    let (out, fake) = check_bundle(&app, &src);
    assert!(out.status.success(), "{}", stderr(&out));
    // And it is back afterwards.
    assert!(fake.join("assets/data/blocks.ron").is_file());
    assert!(!fake.join("assets.hidden-by-check-bundle").exists());
}

/// `check-bundle.sh --lite` from a fake repo that also holds the download
/// list and its script (the lite check reads them).
fn check_lite(app: &Path, src: &Path) -> Output {
    let fake = temp("repo-lite");
    fs::create_dir_all(fake.join("assets/data")).unwrap();
    write(fake.join("assets/data/blocks.ron"), "x");
    for script in ["check-bundle.sh", "check-downloads.sh"] {
        write(
            fake.join("scripts").join(script),
            &fs::read_to_string(repo().join("scripts").join(script)).unwrap(),
        );
    }
    write(
        fake.join("assets/data/downloads.ron"),
        &fs::read_to_string(repo().join("assets/data/downloads.ron")).unwrap(),
    );
    Command::new("sh")
        .arg(fake.join("scripts/check-bundle.sh"))
        .arg("--lite")
        .arg(app)
        .arg(src)
        .output()
        .unwrap()
}

/// A lite app: the small asset folders only, and a binary that logs what the
/// real one logs when it opens the first-launch screen.
fn fake_lite_app(src: &Path, logs_the_screen: bool) -> PathBuf {
    let app = temp("lite-app").join("Rustcraft.app");
    let res = app.join("Contents/Resources/assets");
    fs::create_dir_all(&res).unwrap();
    for folder in ["textures", "data", "structures", "skins"] {
        fs::create_dir_all(src.join(folder)).unwrap();
        assert!(
            Command::new("cp")
                .arg("-R")
                .arg(src.join(folder))
                .arg(&res)
                .status()
                .unwrap()
                .success()
        );
    }
    let log = if logs_the_screen {
        "echo 'first-launch screen: window opened' >&2\n"
    } else {
        "echo 'title screen' >&2\n"
    };
    let bin = app.join("Contents/MacOS/rustcraft");
    write(&bin, &format!("#!/bin/sh\n{log}sleep 3\n"));
    Command::new("chmod").arg("+x").arg(&bin).status().unwrap();
    app
}

#[test]
fn the_lite_check_passes_a_lite_app_that_shows_the_first_launch_screen() {
    let src = complete_source("lite-src");
    let app = fake_lite_app(&src, true);
    let out = check_lite(&app, &src);
    assert!(out.status.success(), "{}", stderr(&out));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("no Mojang file, pack or mod jar inside"),
        "{text}"
    );
    assert!(text.contains("shows the first-launch screen"), "{text}");
}

#[test]
fn the_lite_check_fails_on_vanilla_packs_jars_or_a_missing_screen() {
    let src = complete_source("lite-src-bad");
    // A sound file inside.
    let app = fake_lite_app(&src, true);
    write(
        app.join("Contents/Resources/assets/textures/sound/stone1.ogg"),
        "x",
    );
    let out = check_lite(&app, &src);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("must be downloaded instead"),
        "{}",
        stderr(&out)
    );

    // A vanilla folder, however small.
    let app = fake_lite_app(&src, true);
    write(
        app.join("Contents/Resources/assets/vanilla/assets/minecraft/lang/en_us.json"),
        "{}",
    );
    let out = check_lite(&app, &src);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("has an assets/vanilla folder"),
        "{}",
        stderr(&out)
    );

    // A pack named in the download list.
    let app = fake_lite_app(&src, true);
    write(
        app.join("Contents/Resources/assets/textures/Patrix_26.3_32x_basic.zip"),
        "x",
    );
    let out = check_lite(&app, &src);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("Patrix_26.3_32x_basic.zip"),
        "{}",
        stderr(&out)
    );

    // An app that doesn't show the screen.
    let app = fake_lite_app(&src, false);
    let out = check_lite(&app, &src);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("did not show the first-launch screen"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn check_downloads_reads_the_games_own_list() {
    let out = run("check-downloads.sh", &[Path::new("--list")]);
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert_eq!(text.lines().count(), 14, "{text}");
    for e in rustcraft::setup::manifest::extras() {
        assert!(
            text.contains(&e.file),
            "{} missing from the script's list",
            e.file
        );
    }
}
