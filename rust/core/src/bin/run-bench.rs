//! Runs the C++ harness (bench/cpp) and the Rust prototype's `tc-bench` with identical arguments
//! on the generated corpus, checks output parity byte for byte and writes a Markdown report
//! (handoff §7.5).
//!
//! Run from the repo root:
//!   cargo run --release --manifest-path rust/Cargo.toml -p tc-core --bin run-bench -- [options]
//! Options:
//!   --data DIR     test data directory (default `testdata`)
//!   --cpp PATH     C++ harness (default `bench/cpp/build/[Release/]tc_bench_cpp`)
//!   --rust PATH    Rust `tc-bench` (default `rust/target/release/tc-bench`); skipped if missing
//!   --runs N       measured runs per timed case (default 3, after one load-only warm-up)
//!   --only TEXT    run only cases whose name contains TEXT
//!   --report PATH  Markdown report (default `rust/bench-results.md`)

use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const TOOLS: [&str; 2] = ["C++", "Rust"];

struct Case {
    name: String,
    file: &'static str,
    /// delimiter, quote, escape, encoding, BOM bytes
    dialect: [&'static str; 5],
    steps: &'static [&'static str],
    /// true: warm-up + N measured runs; false: one run, only for parity
    timed: bool,
    /// output suffixes (`saved`, `after`) compared byte for byte between the tools
    parity: &'static [&'static str],
}

const UTF8: [&str; 5] = [",", "\"", "\"", "utf8", "0"];

fn cases() -> Vec<Case> {
    fn utf8(
        name: &str,
        file: &'static str,
        steps: &'static [&'static str],
        parity: &'static [&'static str],
    ) -> Case {
        Case {
            name: name.into(),
            file,
            dialect: UTF8,
            steps,
            timed: true,
            parity,
        }
    }
    let mut cases = vec![
        utf8(
            "big-core",
            "big.csv",
            &[
                "--save",
                "--find",
                "NEEDLE_7f3a",
                "--find-ci",
                "needle_7F3A",
            ],
            &["saved"],
        ),
        utf8(
            "big-sort-num",
            "big.csv",
            &["--sort-col", "5", "--sort-type", "num"],
            &[],
        ),
        utf8(
            "big-sort-str",
            "big.csv",
            &["--sort-col", "2", "--sort-type", "str"],
            &[],
        ),
        utf8(
            "big-sort-stri",
            "big.csv",
            &["--sort-col", "4", "--sort-type", "stri"],
            &[],
        ),
        utf8("wide-core", "wide.csv", &["--save"], &["saved"]),
        utf8(
            "quoted-regex",
            "quoted.csv",
            &["--save", "--find-re", "NEEDLE_[0-9a-f]{4}"],
            &["saved"],
        ),
        utf8(
            "quoted-macro",
            "quoted.csv",
            &[
                "--macro",
                "bench/macros/scale.js",
                "--macro-sel",
                "1,2,-1,-1",
            ],
            &[],
        ),
        // unique keys, so the sorted output must be identical despite unstable vs. stable sort
        Case {
            timed: false,
            ..utf8(
                "big-sort-id",
                "big.csv",
                &["--sort-col", "0", "--sort-type", "num"],
                &["sorted"],
            )
        },
    ];
    let parity_files: &[(&'static str, [&'static str; 5])] = &[
        ("ragged.csv", UTF8),
        ("semicolon.csv", [";", "\"", "\"", "utf8", "0"]),
        ("tab.tsv", ["tab", "\"", "\"", "utf8", "0"]),
        ("pipe.csv", ["|", "\"", "\"", "utf8", "0"]),
        ("backslash.csv", [",", "\"", "\\", "utf8", "0"]),
        ("utf8bom.csv", [",", "\"", "\"", "utf8", "3"]),
        ("latin1.csv", [";", "\"", "\"", "latin1", "0"]),
        ("win1252.csv", [";", "\"", "\"", "win1252", "0"]),
        ("utf16le.csv", ["tab", "\"", "\"", "utf16le", "2"]),
        ("utf16be.csv", [",", "\"", "\"", "utf16be", "2"]),
        ("edge/empty.csv", UTF8),
        ("edge/header_only.csv", UTF8),
        ("edge/single_cell.csv", UTF8),
        ("edge/no_trailing_newline.csv", UTF8),
        ("edge/blank_lines.csv", UTF8),
        ("edge/nul_bytes.csv", UTF8),
        ("edge/cr_only.csv", UTF8),
        ("edge/unterminated_quote.csv", UTF8),
        ("edge/invalid_utf8.csv", UTF8),
        ("edge/latin1_c1.csv", [",", "\"", "\"", "latin1", "0"]),
        ("edge/ctrl_z.csv", UTF8),
    ];
    for &(file, dialect) in parity_files {
        let stem = file
            .trim_end_matches(".csv")
            .trim_end_matches(".tsv")
            .replace('/', "-");
        cases.push(Case {
            name: format!("parity-{stem}"),
            file,
            dialect,
            steps: &["--save"],
            timed: false,
            parity: &["saved"],
        });
    }
    cases
}

/// Differences that are intended (divergence ids from docs/dev/RUST_PROTOTYPE_HANDOFF.md §5.9).
fn expected_difference(file: &str) -> Option<&'static str> {
    match file {
        "edge/unterminated_quote.csv" => {
            Some("D1: C++ drops the unterminated record and deletes the row before it")
        }
        "edge/invalid_utf8.csv" => Some("D6: different U+FFFD replacement rules"),
        "edge/latin1_c1.csv" => Some("D5: C++ drops Latin-1 bytes 0x80–0x9F"),
        "edge/ctrl_z.csv" => Some("D16: on Windows C++ reads in text mode, byte 0x1A ends the file"),
        _ => None,
    }
}

/// Aim (time ratio Rust/C++) per step, from the handoff §6. The hard limit is always ≤ 1.0.
fn aim(step: &str) -> Option<f64> {
    match step {
        "load" | "save" => Some(0.5),
        "sort" => Some(0.3),
        "find_cs" | "find_ci" => Some(0.2),
        _ => None,
    }
}

#[derive(Clone)]
struct Step {
    name: String,
    ms: f64,
    peak_mb: f64,
    shape: String,
    found: Option<String>,
}

fn parse_step(line: &str) -> Option<Step> {
    fn field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
        let start = line.find(&format!("\"{key}\":"))? + key.len() + 3;
        let value = line[start..].trim_start();
        let value = match value.as_bytes().first()? {
            b'"' => &value[1..value[1..].find('"')? + 1],
            b'[' => &value[..value.find(']')? + 1],
            _ => value.split([',', '}']).next()?.trim_end(),
        };
        Some(value)
    }
    if !line.trim_start().starts_with('{') {
        return None;
    }
    Some(Step {
        name: field(line, "step")?.to_string(),
        ms: field(line, "ms")?.parse().ok()?,
        peak_mb: field(line, "peak_rss_mb")?.parse().ok()?,
        shape: format!("{}×{}", field(line, "rows")?, field(line, "cols")?),
        found: field(line, "found").map(|s| s.to_string()),
    })
}

fn run(
    tool: &Path,
    case: &Case,
    data: &Path,
    out: &Path,
    load_only: bool,
) -> Result<Vec<Step>, String> {
    let [delim, quote, escape, enc, bom] = case.dialect;
    let mut cmd = Command::new(tool);
    cmd.arg("--file").arg(data.join(case.file));
    cmd.args([
        "--delim", delim, "--quote", quote, "--escape", escape, "--enc", enc, "--bom", bom,
    ]);
    cmd.arg("--out").arg(out);
    if !load_only {
        cmd.args(case.steps);
    }
    let output = cmd
        .output()
        .map_err(|e| format!("cannot start {}: {e}", tool.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("exit {}: {}", output.status, stderr.trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(parse_step)
        .collect())
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

/// Offset of the first differing byte, `None` if the files are identical.
fn first_difference(a: &Path, b: &Path) -> io::Result<Option<u64>> {
    let mut fa = BufReader::with_capacity(1 << 20, File::open(a)?);
    let mut fb = BufReader::with_capacity(1 << 20, File::open(b)?);
    let mut offset = 0u64;
    loop {
        let (ba, bb) = (fa.fill_buf()?, fb.fill_buf()?);
        let n = ba.len().min(bb.len());
        if n == 0 {
            return Ok((ba.len() != bb.len()).then_some(offset));
        }
        if let Some(i) = ba[..n].iter().zip(&bb[..n]).position(|(x, y)| x != y) {
            return Ok(Some(offset + i as u64));
        }
        fa.consume(n);
        fb.consume(n);
        offset += n as u64;
    }
}

fn parity(case: &Case, suffix: &str, outs: &[PathBuf; 2]) -> String {
    let files = outs
        .clone()
        .map(|o| PathBuf::from(format!("{}.{suffix}.csv", o.display())));
    let diff = match first_difference(&files[0], &files[1]) {
        Ok(d) => d,
        Err(e) => return format!("⚠️ cannot compare: {e}"),
    };
    let size = |p: &Path| fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    match (diff, expected_difference(case.file)) {
        (None, None) => "✅ identical".into(),
        (None, Some(why)) => format!("✅ identical (expected difference did not occur: {why})"),
        (Some(at), Some(why)) => format!("➖ expected difference at byte {at} — {why}"),
        (Some(at), None) => format!(
            "❌ differs at byte {at} (C++ {} B, Rust {} B)",
            size(&files[0]),
            size(&files[1])
        ),
    }
}

fn default_cpp() -> PathBuf {
    let exe = format!("tc_bench_cpp{}", std::env::consts::EXE_SUFFIX);
    let build = Path::new("bench/cpp/build");
    [build.join(&exe), build.join("Release").join(&exe)]
        .into_iter()
        .find(|p| p.exists())
        .unwrap_or_else(|| build.join(&exe))
}

/// UTC date from the system clock (civil-from-days, Howard Hinnant).
fn today() -> String {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() / 86_400)
        .unwrap_or(0) as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

fn main() {
    let mut data = PathBuf::from("testdata");
    let mut tools: [Option<PathBuf>; 2] = [
        Some(default_cpp()),
        Some(PathBuf::from(format!(
            "rust/target/release/tc-bench{}",
            std::env::consts::EXE_SUFFIX
        ))),
    ];
    let mut runs = 3usize;
    let mut only = String::new();
    let mut report_path = PathBuf::from("rust/bench-results.md");
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .unwrap_or_else(|| panic!("missing value for {flag}"));
        match flag.as_str() {
            "--data" => data = value.into(),
            "--cpp" => tools[0] = Some(value.into()),
            "--rust" => tools[1] = Some(value.into()),
            "--runs" => runs = value.parse().expect("--runs needs a number"),
            "--only" => only = value,
            "--report" => report_path = value.into(),
            _ => panic!("unknown option {flag}"),
        }
    }
    if !Path::new("bench/macros/scale.js").exists() {
        panic!("run from the repository root (bench/macros/scale.js not found)");
    }
    for (slot, tool) in tools.iter_mut().enumerate() {
        if tool.as_ref().is_some_and(|p| !p.exists()) {
            eprintln!(
                "{}: {} not found, skipping",
                TOOLS[slot],
                tool.as_ref().unwrap().display()
            );
            *tool = None;
        }
    }
    assert!(tools.iter().any(Option::is_some), "neither tool found");

    let mut timing = String::new();
    let mut parity_rows = String::new();
    let mut failures = String::new();
    let started = Instant::now();

    for case in cases().iter().filter(|c| c.name.contains(&only)) {
        if !data.join(case.file).exists() {
            let _ = writeln!(
                failures,
                "| {} | – | missing {} (run gen-testdata) |",
                case.name, case.file
            );
            continue;
        }
        // per tool: all measured runs of this case
        let mut results: [Vec<Vec<Step>>; 2] = [Vec::new(), Vec::new()];
        let mut outs: [PathBuf; 2] = [PathBuf::new(), PathBuf::new()];
        for (slot, tool) in tools.iter().enumerate() {
            let Some(tool) = tool else { continue };
            let out_dir = data
                .join("out")
                .join(if slot == 0 { "cpp" } else { "rust" });
            fs::create_dir_all(&out_dir).expect("cannot create output directory");
            outs[slot] = out_dir.join(&case.name);
            let n = if case.timed { runs } else { 1 };
            if case.timed {
                eprintln!(
                    "[{:>6.0}s] {} {}: warm-up",
                    started.elapsed().as_secs_f64(),
                    case.name,
                    TOOLS[slot]
                );
                let _ = run(tool, case, &data, &outs[slot], true);
            }
            for i in 1..=n {
                eprintln!(
                    "[{:>6.0}s] {} {}: run {i}/{n}",
                    started.elapsed().as_secs_f64(),
                    case.name,
                    TOOLS[slot]
                );
                match run(tool, case, &data, &outs[slot], false) {
                    Ok(steps) => results[slot].push(steps),
                    Err(e) => {
                        let _ = writeln!(
                            failures,
                            "| {} | {} | {} |",
                            case.name,
                            TOOLS[slot],
                            e.replace('\n', " ").replace('|', "\\|")
                        );
                        break;
                    }
                }
            }
        }

        if case.timed {
            let reference = results
                .iter()
                .find_map(|r| r.first())
                .cloned()
                .unwrap_or_default();
            for step in &reference {
                let pick = |slot: usize| -> Option<(f64, f64, Step)> {
                    let steps: Vec<&Step> = results[slot]
                        .iter()
                        .filter_map(|run| run.iter().find(|s| s.name == step.name))
                        .collect();
                    let first = (*steps.first()?).clone();
                    let ms = median(&mut steps.iter().map(|s| s.ms).collect::<Vec<_>>());
                    let peak = median(&mut steps.iter().map(|s| s.peak_mb).collect::<Vec<_>>());
                    Some((ms, peak, first))
                };
                let (cpp, rust) = (pick(0), pick(1));
                let fmt = |v: Option<f64>| v.map_or("–".into(), |v| format!("{v:.0}"));
                let (mut ratio, mut hard, mut aim_met, mut check) = (
                    "–".to_string(),
                    "–".to_string(),
                    "–".to_string(),
                    "–".to_string(),
                );
                if let (Some((c_ms, c_peak, c)), Some((r_ms, r_peak, r))) = (&cpp, &rust) {
                    let time_ratio = r_ms / c_ms.max(1.0);
                    ratio = format!("{time_ratio:.2}");
                    let peak_ok =
                        !matches!(step.name.as_str(), "load" | "sort") || r_peak <= c_peak;
                    hard = if time_ratio <= 1.0 && peak_ok {
                        "✅"
                    } else {
                        "❌"
                    }
                    .into();
                    aim_met = aim(&step.name).map_or("–".into(), |a| {
                        if time_ratio <= a { "✅" } else { "❌" }.into()
                    });
                    check = if c.shape != r.shape {
                        format!("❌ shape {} vs {}", c.shape, r.shape)
                    } else if c.found != r.found {
                        format!("❌ found {:?} vs {:?}", c.found, r.found)
                    } else {
                        "✅".into()
                    };
                }
                let _ = writeln!(
                    timing,
                    "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
                    case.name,
                    step.name,
                    fmt(cpp.as_ref().map(|c| c.0)),
                    fmt(rust.as_ref().map(|r| r.0)),
                    ratio,
                    fmt(cpp.as_ref().map(|c| c.1)),
                    fmt(rust.as_ref().map(|r| r.1)),
                    hard,
                    aim_met,
                    check,
                );
            }
        }

        if tools.iter().all(Option::is_some) && results.iter().all(|r| !r.is_empty()) {
            for suffix in case.parity {
                let _ = writeln!(
                    parity_rows,
                    "| {} | {suffix} | {} |",
                    case.name,
                    parity(case, suffix, &outs)
                );
            }
        }
    }

    let mut report = String::new();
    let _ = writeln!(report, "# Benchmark results: C++ core vs. Rust prototype\n");
    let _ = writeln!(report, "- Date: {} (UTC)", today());
    let _ = writeln!(
        report,
        "- Machine: {} / {}, {} logical CPUs (add CPU, RAM, disk by hand)",
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::thread::available_parallelism().map_or(0, |n| n.get())
    );
    for (slot, tool) in tools.iter().enumerate() {
        let _ = writeln!(
            report,
            "- {}: {}",
            TOOLS[slot],
            tool.as_ref()
                .map_or("not run".into(), |p| p.display().to_string())
        );
    }
    let _ = writeln!(
        report,
        "- Timed cases: median of {runs} runs after one load-only warm-up; total runtime {:.0} s\n",
        started.elapsed().as_secs_f64()
    );
    let _ = writeln!(report, "## Timing\n");
    let _ = writeln!(report, "Times in ms, peak = process peak RSS in MB after the step. Hard limit: Rust/C++ ≤ 1.0 (and peak ≤ C++ for load and sort). Aim: see handoff §6.\n");
    let _ = writeln!(report, "| Case | Step | C++ ms | Rust ms | Rust/C++ | C++ peak MB | Rust peak MB | Hard limit | Aim | Same result |");
    let _ = writeln!(report, "|---|---|---:|---:|---:|---:|---:|---|---|---|");
    report.push_str(&timing);
    if !parity_rows.is_empty() {
        let _ = writeln!(report, "\n## Parity (saved output, byte for byte)\n");
        let _ = writeln!(report, "| Case | Output | Result |\n|---|---|---|");
        report.push_str(&parity_rows);
    }
    if !failures.is_empty() {
        let _ = writeln!(report, "\n## Failures\n");
        let _ = writeln!(report, "| Case | Tool | Error |\n|---|---|---|");
        report.push_str(&failures);
    }
    fs::write(&report_path, &report).expect("cannot write report");
    println!("{report}");
    eprintln!("report written to {}", report_path.display());
}
