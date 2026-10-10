//! Runs the bundled scenario catalogue: one test per scenario file, each
//! iterating the scenario's OS matrix.

use flow_harness::matrix::run_matrix;
use flow_harness::report::failure_text;
use flow_harness::scenario::{catalogue_dir, load_dir};

async fn run_scenario(name: &str) {
    let scenarios = load_dir(&catalogue_dir(), Some(name)).expect("catalogue loads");
    assert_eq!(scenarios.len(), 1, "exactly one scenario is named {name}");
    let report = run_matrix(scenarios, None, 9, |_| {}).await.expect("runs");
    let failures: Vec<String> = report
        .cases()
        .filter(|c| !c.passed)
        .map(|c| format!("[{}] {}", c.combo, failure_text(c)))
        .collect();
    assert!(
        failures.is_empty(),
        "{name}: {} of {} cases failed\n{}",
        failures.len(),
        report.total(),
        failures.join("\n")
    );
}

include!(concat!(env!("OUT_DIR"), "/scenarios_gen.rs"));
