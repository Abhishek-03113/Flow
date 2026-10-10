//! Runs a set of scenarios across their OS matrices, several cases at a time.

use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;
use tokio::sync::Semaphore;

use crate::runner::{run_case, CaseResult};
use crate::scenario::Scenario;

#[derive(Debug, Clone, Serialize)]
pub struct ScenarioReport {
    pub name: String,
    pub description: String,
    pub cases: Vec<CaseResult>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub scenarios: Vec<ScenarioReport>,
    pub duration_ms: u64,
}

impl Report {
    pub fn cases(&self) -> impl Iterator<Item = &CaseResult> {
        self.scenarios.iter().flat_map(|s| s.cases.iter())
    }

    pub fn passed(&self) -> usize {
        self.cases().filter(|c| c.passed).count()
    }

    pub fn total(&self) -> usize {
        self.cases().count()
    }

    pub fn all_passed(&self) -> bool {
        self.cases().all(|c| c.passed)
    }
}

/// Runs every scenario on every cell of its matrix, restricted to the
/// cells in `only` when given, at most `parallel` worlds at once.
pub async fn run_matrix(
    scenarios: Vec<Scenario>,
    only: Option<&[String]>,
    parallel: usize,
    mut progress: impl FnMut(&CaseResult),
) -> Result<Report, String> {
    let started = Instant::now();
    let limit = Arc::new(Semaphore::new(parallel.max(1)));
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut slots = Vec::new();
    for (si, scenario) in scenarios.iter().enumerate() {
        let scenario = Arc::new(scenario.clone());
        let combos = scenario.combos()?;
        let mut count = 0;
        for combo in combos {
            if only.is_some_and(|only| !only.contains(&combo.label)) {
                continue;
            }
            let (limit, tx, scenario) = (Arc::clone(&limit), tx.clone(), Arc::clone(&scenario));
            let slot = count;
            count += 1;
            tokio::spawn(async move {
                let _permit = limit.acquire_owned().await;
                let case = run_case(&scenario, &combo).await;
                let _ = tx.send((si, slot, case));
            });
        }
        slots.push(count);
    }
    drop(tx);

    let mut cases: Vec<Vec<Option<CaseResult>>> = slots.iter().map(|n| vec![None; *n]).collect();
    while let Some((si, slot, case)) = rx.recv().await {
        progress(&case);
        cases[si][slot] = Some(case);
    }
    let scenarios = scenarios
        .into_iter()
        .zip(cases)
        .map(|(scenario, cases)| ScenarioReport {
            name: scenario.name,
            description: scenario.description,
            cases: cases.into_iter().flatten().collect(),
        })
        .collect();
    Ok(Report {
        scenarios,
        duration_ms: started.elapsed().as_millis() as u64,
    })
}
