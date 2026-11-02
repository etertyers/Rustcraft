//! `--perf-test`'s results as JSON (`docs/perf/<label>.json`), the median of
//! several flights (`--runs <n>`), and the checks `scripts/perf_gate.py`
//! makes on them. Reporting only: nothing here touches the game.

use std::{collections::BTreeMap, path::Path, process::Command};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::bench::{self, Bench};

/// One `--perf-test` result: a single flight, or the median of several.
/// `scripts/add_bench.py` adds `benchmarks` (the median of each criterion
/// benchmark, in ms); `scripts/perf_gate.py` compares the whole file with
/// `docs/perf/baseline.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Results {
    pub label: String,
    /// Which baseline row these are for ("vanilla", or a trailer row).
    pub row: String,
    pub commit: String,
    /// `hw.model`, e.g. `Macmini9,1`: the gate refuses to compare machines.
    pub machine: String,
    pub machine_detail: String,
    pub dimension: String,
    pub seed: u64,
    pub render_distance: i32,
    pub seconds: f32,
    pub runs: u32,
    /// Median of each measure over the runs.
    pub measures: BTreeMap<String, f64>,
    /// Max − min as a percentage of the median, per measure.
    pub spread_pct: BTreeMap<String, f64>,
    #[serde(default)]
    pub benchmarks: BTreeMap<String, f64>,
    /// Each flight's measures, for combining launches.
    #[serde(default)]
    pub per_run: Vec<BTreeMap<String, f64>>,
}

impl Results {
    pub fn new(bench: &Bench, render_distance: i32, measures: BTreeMap<String, f64>) -> Self {
        let detail = bench::machine();
        Self {
            label: bench.perf_label(),
            row: bench.row.clone(),
            commit: commit(),
            machine: detail.split(" (").next().unwrap_or_default().to_owned(),
            machine_detail: detail,
            dimension: format!("{:?}{}", bench.dimension, bench.dimension_suffix()).to_lowercase(),
            seed: bench::SEED,
            render_distance,
            seconds: bench.seconds,
            runs: 1,
            spread_pct: measures.keys().map(|k| (k.clone(), 0.0)).collect(),
            measures,
            benchmarks: BTreeMap::new(),
            per_run: Vec::new(),
        }
    }

    pub fn write(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self)? + "\n")
            .with_context(|| format!("writing {}", path.display()))
    }

    /// The write-up for several runs, in Markdown.
    pub fn markdown(&self) -> String {
        let mut text = format!(
            "# {} performance\n\n{} flights of {:.0} s (seed {}, render distance {}), median and spread \
             (max − min as % of the median). Measured on {}, commit {}, row `{}`.\n\n\
             | Measure | Median | Spread |\n|---|---|---|\n",
            self.label,
            self.runs,
            self.seconds,
            self.seed,
            self.render_distance,
            self.machine_detail,
            self.commit,
            self.row
        );
        for (key, value) in &self.measures {
            text += &format!("| {key} | {value:.2} | {:.1}% |\n", self.spread_pct[key]);
        }
        text
    }
}

/// The median of `values` (the mean of the middle two when even).
pub fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let n = sorted.len();
    if n == 0 {
        0.0
    } else if n % 2 == 1 {
        sorted[n / 2]
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    }
}

/// Max − min as a percentage of the median.
pub fn spread_pct(values: &[f64]) -> f64 {
    let mid = median(values);
    if mid == 0.0 {
        return 0.0;
    }
    let max = values.iter().copied().fold(f64::MIN, f64::max);
    let min = values.iter().copied().fold(f64::MAX, f64::min);
    (max - min) / mid.abs() * 100.0
}

/// The median and spread of each measure the runs share.
pub fn combine(runs: &[Results]) -> Results {
    let mut out = runs[0].clone();
    out.runs = runs.len() as u32;
    out.per_run = runs.iter().map(|r| r.measures.clone()).collect();
    for key in runs[0].measures.keys() {
        let values: Vec<f64> = runs
            .iter()
            .filter_map(|r| r.measures.get(key))
            .copied()
            .collect();
        out.measures.insert(key.clone(), median(&values));
        out.spread_pct.insert(key.clone(), spread_pct(&values));
    }
    out
}

fn commit() -> String {
    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_owned())
            .unwrap_or_default()
    };
    let head = git(&["rev-parse", "HEAD"]);
    if git(&["status", "--porcelain", "--untracked-files=no"]).is_empty() {
        head
    } else {
        format!("{head}-dirty")
    }
}

/// `--perf-test --runs <n>`: flies n times, each in a fresh process with
/// an empty world and its own 5 s warm-up, and writes the medians.
pub fn run_many(bench: &Bench, args: &[String]) -> Result<()> {
    let exe = std::env::current_exe()?;
    // This command line minus --runs and --perf-out.
    let mut rest = Vec::new();
    let mut words = args.iter().skip(1);
    while let Some(word) = words.next() {
        if word == "--runs" || word == "--perf-out" {
            words.next();
        } else {
            rest.push(word.clone());
        }
    }
    let mut flights = Vec::new();
    for i in 1..=bench.runs {
        let out =
            std::env::temp_dir().join(format!("rustcraft-perf-{}-{i}.json", std::process::id()));
        println!("perf run {i} of {}", bench.runs);
        let status = Command::new(&exe)
            .args(&rest)
            .args(["--runs", "1", "--perf-out"])
            .arg(&out)
            .status()?;
        if !status.success() {
            bail!("run {i} failed: {status}");
        }
        let text = std::fs::read_to_string(&out)
            .with_context(|| format!("run {i} wrote no results to {}", out.display()))?;
        flights.push(serde_json::from_str::<Results>(&text)?);
        let _ = std::fs::remove_file(&out);
    }
    let results = combine(&flights);
    let json = format!("docs/perf/{}.json", results.label);
    results.write(Path::new(&json))?;
    let md = format!("docs/perf/{}.md", results.label);
    std::fs::write(&md, results.markdown())?;
    println!("{}\nwrote {json} and {md}", results.markdown());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn median_and_spread_of_three_runs() {
        assert_eq!(median(&[120.0, 100.0, 110.0]), 110.0);
        assert_eq!(median(&[1.0, 3.0]), 2.0);
        // (120 − 100) / 110
        assert!((spread_pct(&[120.0, 100.0, 110.0]) - 18.18).abs() < 0.01);
        assert_eq!(spread_pct(&[0.0, 0.0]), 0.0);
    }
}
