use flow_harness::matrix::run_matrix;
use flow_harness::scenario::{catalogue_dir, load_dir};

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
    let glob = std::env::args().nth(1);
    let scenarios = load_dir(&catalogue_dir(), glob.as_deref()).expect("load");
    let only: Option<Vec<String>> = std::env::args()
        .nth(2)
        .map(|s| s.split(',').map(String::from).collect());
    let report = run_matrix(scenarios, only.as_deref(), 12, |c| {
        eprintln!(
            "{} {} {}",
            c.scenario,
            c.combo,
            if c.passed { "ok" } else { "FAILED" }
        );
    })
    .await
    .expect("run");
    for s in &report.scenarios {
        for c in &s.cases {
            if !c.passed {
                eprintln!("--- {} {}: {:?}", c.scenario, c.combo, c.setup_error);
                for st in &c.steps {
                    eprintln!(
                        "  {:?} {} | {:?} | exp {:?} act {:?}",
                        st.status, st.description, st.message, st.expected, st.actual
                    );
                }
            }
        }
    }
}
