//! `flow-harness run [--scenario GLOB] [--pairs all|win-mac,...] [--report-dir DIR]`
//! `flow-harness up --devices "Mac:macos:controller,PC:windows:controlled" [--paired] ...`
//! In `up` mode, stdin takes `disconnect NAME` / `reconnect NAME` lines;
//! `--stop-on-eof` also stops when stdin closes (its parent died).
//! Bare flags run the matrix (CI calls `flow-harness --report-dir DIR`).

use std::path::PathBuf;
use std::process::ExitCode;

use flow_daemon::devmode::SecurityMode;
use flow_harness::matrix::run_matrix;
use flow_harness::report;
use flow_harness::scenario::{catalogue_dir, load_dir, parse_pairs_filter};
use flow_harness::up::{self, UpOptions};

const USAGE: &str = "usage:
  flow-harness [run] [--scenario GLOB] [--pairs all|win-mac,...] [--report-dir DIR] [--jobs N] [--scenarios-dir DIR]
  flow-harness up --devices Name:os:role,... [--paired] [--base-port N] [--manifest FILE] [--security secure|insecure] [--stop-on-eof]";

#[tokio::main(flavor = "multi_thread")]
async fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let command = match args.first().map(String::as_str) {
        Some("run" | "up") => args.remove(0),
        Some("-h" | "--help") => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        _ => "run".to_string(),
    };
    let result = match command.as_str() {
        "up" => run_up(args).await,
        _ => run(args).await,
    };
    match result {
        Ok(code) => code,
        Err(message) => {
            eprintln!("flow-harness: {message}\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// `--flag value` pairs and bare switches, rejecting anything unknown.
struct Flags(Vec<String>);

impl Flags {
    fn value(&mut self, name: &str) -> Result<Option<String>, String> {
        match self.0.iter().position(|a| a == name) {
            None => Ok(None),
            Some(i) => {
                if i + 1 >= self.0.len() {
                    return Err(format!("{name} needs a value"));
                }
                self.0.remove(i);
                Ok(Some(self.0.remove(i)))
            }
        }
    }

    fn switch(&mut self, name: &str) -> bool {
        self.0
            .iter()
            .position(|a| a == name)
            .map(|i| self.0.remove(i))
            .is_some()
    }

    fn done(self) -> Result<(), String> {
        match self.0.first() {
            None => Ok(()),
            Some(extra) => Err(format!("unexpected argument {extra:?}")),
        }
    }
}

async fn run(args: Vec<String>) -> Result<ExitCode, String> {
    let mut flags = Flags(args);
    let glob = flags.value("--scenario")?;
    let pairs = flags.value("--pairs")?;
    let report_dir = flags.value("--report-dir")?.map(PathBuf::from);
    let dir = flags
        .value("--scenarios-dir")?
        .map_or_else(catalogue_dir, PathBuf::from);
    let jobs = match flags.value("--jobs")? {
        Some(n) => n.parse().map_err(|_| "--jobs needs a number".to_string())?,
        None => 12,
    };
    flags.done()?;

    let only = pairs.as_deref().and_then(parse_pairs_filter);
    let scenarios = load_dir(&dir, glob.as_deref())?;
    if scenarios.is_empty() {
        return Err("no scenario matches".to_string());
    }
    eprintln!("running {} scenarios", scenarios.len());
    let report = run_matrix(scenarios, only.as_deref(), jobs, |case| {
        let mark = if case.passed { "ok    " } else { "FAILED" };
        eprintln!(
            "{mark} {} [{}] {} ms",
            case.scenario, case.combo, case.duration_ms
        );
    })
    .await?;

    for case in report.cases().filter(|c| !c.passed) {
        eprintln!(
            "\n--- {} [{}]\n{}",
            case.scenario,
            case.combo,
            report::failure_text(case)
        );
    }
    eprintln!(
        "\n{} / {} cases passed in {:.1}s",
        report.passed(),
        report.total(),
        report.duration_ms as f64 / 1000.0
    );
    if let Some(dir) = report_dir {
        report::write_all(&report, &dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        eprintln!("reports written to {}", dir.display());
    }
    // Exit directly: engine tasks still winding down must not delay or panic the exit.
    std::process::exit(if report.all_passed() { 0 } else { 1 });
}

async fn run_up(args: Vec<String>) -> Result<ExitCode, String> {
    let mut flags = Flags(args);
    let devices = flags.value("--devices")?.ok_or("--devices is required")?;
    let paired = flags.switch("--paired");
    let stop_on_eof = flags.switch("--stop-on-eof");
    let base_port = flags
        .value("--base-port")?
        .map(|p| {
            p.parse()
                .map_err(|_| "--base-port needs a port".to_string())
        })
        .transpose()?;
    let manifest = flags.value("--manifest")?.map(PathBuf::from);
    let security = match flags.value("--security")?.as_deref() {
        None | Some("insecure") => SecurityMode::Insecure,
        Some("secure") => SecurityMode::Secure,
        Some(other) => return Err(format!("--security {other:?}: use secure or insecure")),
    };
    flags.done()?;

    let options = UpOptions {
        devices: up::parse_devices(&devices)?,
        paired,
        base_port,
        manifest: manifest.clone(),
        security,
    };
    let (mut world, entries) = up::start(&options).await?;
    up::publish(&entries, manifest.as_ref())?;
    eprintln!("devices are up; Ctrl-C to stop");
    let mut commands = up::stdin_commands();
    let mut stdin_open = true;
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            line = commands.recv(), if stdin_open => match line {
                Some(line) => up::control(&mut world, &line).await,
                // The parent (a test runner) went away: do not outlive it.
                None if stop_on_eof => break,
                None => stdin_open = false,
            },
        }
    }
    world.shutdown();
    std::process::exit(0);
}
