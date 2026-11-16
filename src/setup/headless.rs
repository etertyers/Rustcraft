//! `--download-only` (M74): the first-launch screen without a window, for
//! scripts and CI. Fills this clone's own `assets/vanilla` and
//! `assets/downloads` (honoring `.skip-packs`), copying from this Mac when a
//! complete copy exists and downloading otherwise.

use super::download::{self, Event, Policy, Request};
use super::layers::{self, CopySource, Needs};
use super::manifest::Endpoints;
use super::screen::megabytes;
use super::{Result, SetupError};
use std::io::Write;
use std::sync::Arc;

/// Does the work for `needs`, writing progress lines to `out`. `Ok` once
/// everything is in place (also when `needs` is `None`).
pub fn run_with(
    needs: Option<Needs>,
    copy: Option<CopySource>,
    endpoints: Endpoints,
    policy: Policy,
    out: &mut dyn Write,
) -> Result<()> {
    let Some(needs) = needs else {
        let _ = writeln!(out, "download-only: nothing is missing");
        return Ok(());
    };
    let _ = writeln!(
        out,
        "download-only: filling {} (Minecraft's files {}, {} packs and mods)",
        needs.target.display(),
        if needs.vanilla { "missing" } else { "present" },
        needs.extras.len()
    );
    let handle = if let Some(mut source) = copy {
        source.vanilla = needs.vanilla;
        source.extras = needs.extras.clone();
        let _ = writeln!(
            out,
            "download-only: copying {} from {}",
            megabytes(source.bytes),
            source.assets.display()
        );
        download::start_copy(source, needs.target.clone())
    } else {
        let plan = if needs.vanilla {
            let asked = download::measure(endpoints.clone(), policy)
                .recv()
                .map_err(|_| SetupError::Io("the size check stopped".into()))?;
            Some(Arc::new(asked?))
        } else {
            None
        };
        let _ = writeln!(out, "download-only: downloading");
        download::start(Request {
            vanilla: plan,
            extras: needs.extras.clone(),
            target: needs.target.clone(),
            endpoints,
            policy,
        })
    };
    let (mut total, mut done, mut shown) = (0u64, 0i64, 0u64);
    loop {
        match handle.rx.recv() {
            Ok(Event::Started { total: t, files }) => {
                total = t;
                let _ = writeln!(out, "download-only: {} in {files} files", megabytes(t));
            }
            Ok(Event::Bytes(n)) => {
                done += n;
                // A line every 10 per cent.
                let tenth = (done.max(0) as u64 * 10)
                    .checked_div(total)
                    .map_or(0, |t| t.min(10));
                if tenth > shown {
                    shown = tenth;
                    let _ = writeln!(
                        out,
                        "download-only: {}% ({} of {})",
                        tenth * 10,
                        megabytes(done.max(0) as u64),
                        megabytes(total)
                    );
                }
            }
            Ok(Event::Stage(text)) => {
                let _ = writeln!(out, "download-only: {text}");
            }
            Ok(Event::File(_)) => {}
            Ok(Event::Finished) => {
                let _ = writeln!(out, "download-only: done");
                return Ok(());
            }
            Ok(Event::Failed(err)) => return Err(err),
            Ok(Event::Cancelled) | Err(_) => return Err(SetupError::Cancelled),
        }
    }
}

/// `--download-only` for this clone: the exit status (0 when everything is
/// complete). `$RUSTCRAFT_SETUP_BASE` points it at a local server.
pub fn cli() -> i32 {
    let assets = crate::assets::dir();
    let source = crate::assets::source();
    let user = layers::user_assets();
    let needs = layers::needs(&assets, source, user.as_deref());
    let copy = needs.as_ref().and_then(|n| {
        layers::find_copy_source(&assets, n.vanilla, &n.extras, &layers::copy_candidates())
    });
    let endpoints = match std::env::var("RUSTCRAFT_SETUP_BASE") {
        Ok(base) => Endpoints::local(&base),
        Err(_) => Endpoints::official(),
    };
    let result = run_with(
        needs,
        copy,
        endpoints,
        Policy::default(),
        &mut std::io::stdout(),
    );
    match result {
        Ok(()) => {
            layers::remember_clone(&assets, source);
            0
        }
        Err(err) => {
            eprintln!("download-only failed: {err}");
            1
        }
    }
}
