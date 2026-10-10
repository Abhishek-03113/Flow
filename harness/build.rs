//! Generates one `#[tokio::test]` per scenario file in `scenarios/`, so a
//! new scenario is a test the moment it is added.

use std::fmt::Write as _;

fn main() {
    println!("cargo:rerun-if-changed=scenarios");
    let mut names: Vec<String> = std::fs::read_dir("scenarios")
        .expect("scenarios dir")
        .filter_map(Result::ok)
        .filter_map(|e| {
            let text = std::fs::read_to_string(e.path()).ok()?;
            let line = text.lines().find(|l| l.starts_with("name = "))?;
            Some(
                line.trim_start_matches("name = ")
                    .trim_matches('"')
                    .to_string(),
            )
        })
        .collect();
    names.sort();
    let mut out = String::new();
    for name in names {
        let ident = name.replace('-', "_");
        let _ = writeln!(
            out,
            "#[tokio::test(flavor = \"multi_thread\", worker_threads = 4)]\nasync fn {ident}() {{ run_scenario(\"{name}\").await; }}"
        );
    }
    let dir = std::env::var("OUT_DIR").expect("OUT_DIR");
    std::fs::write(std::path::Path::new(&dir).join("scenarios_gen.rs"), out).expect("write");
}
