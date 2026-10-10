//! Reports for a matrix run: `report.html` (summary matrix and per-case
//! step timelines), `report.json` and `junit.xml`.

use std::fmt::Write as _;
use std::path::Path;

use crate::matrix::Report;
use crate::runner::{CaseResult, Status};

/// Writes all three reports into `dir`.
pub fn write_all(report: &Report, dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join("report.html"), html(report))?;
    std::fs::write(
        dir.join("report.json"),
        serde_json::to_string_pretty(report).map_err(std::io::Error::other)?,
    )?;
    std::fs::write(dir.join("junit.xml"), junit(report))?;
    Ok(())
}

fn esc(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn anchor(scenario: &str, combo: &str) -> String {
    format!("{scenario}--{combo}")
}

/// Matrix columns: every cell label used by any scenario, pairs first.
fn columns(report: &Report) -> Vec<String> {
    let mut columns: Vec<String> = Vec::new();
    for case in report.cases() {
        if !columns.contains(&case.combo) {
            columns.push(case.combo.clone());
        }
    }
    columns.sort_by_key(|c| (c.matches('-').count(), c.clone()));
    columns
}

pub fn html(report: &Report) -> String {
    let columns = columns(report);
    let (passed, total) = (report.passed(), report.total());
    let verdict = if report.all_passed() { "pass" } else { "fail" };
    let mut out = String::new();
    let _ = write!(
        out,
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Flow Harness Report</title><style>{STYLE}</style></head><body><main>
<header><h1>Flow harness</h1>
<p class="sub">Virtual daemons on emulated operating systems, controller &times; controlled</p>
<div class="verdict {verdict}"><b>{passed} / {total}</b> cases passed <span>in {secs:.1}s across {n} scenarios</span></div></header>
<h2>Summary</h2><div class="scroll"><table class="matrix"><thead><tr><th>Scenario</th>"#,
        secs = report.duration_ms as f64 / 1000.0,
        n = report.scenarios.len()
    );
    for column in &columns {
        let _ = write!(out, "<th>{}</th>", esc(column));
    }
    out.push_str("</tr></thead><tbody>");
    for scenario in &report.scenarios {
        let _ = write!(
            out,
            r##"<tr><th class="name"><a href="#{}">{}</a></th>"##,
            esc(&scenario.name),
            esc(&scenario.name)
        );
        for column in &columns {
            match scenario.cases.iter().find(|c| &c.combo == column) {
                Some(case) => {
                    let (class, mark) = if case.passed {
                        ("ok", "&#10003;")
                    } else {
                        ("bad", "&#10007;")
                    };
                    let _ = write!(
                        out,
                        r##"<td class="{class}"><a href="#{}" title="{} ms">{mark}</a></td>"##,
                        esc(&anchor(&scenario.name, column)),
                        case.duration_ms
                    );
                }
                None => out.push_str(r#"<td class="na">&middot;</td>"#),
            }
        }
        out.push_str("</tr>");
    }
    out.push_str("</tbody></table></div><h2>Scenarios</h2>");
    for scenario in &report.scenarios {
        let ok = scenario.cases.iter().filter(|c| c.passed).count();
        let _ = write!(
            out,
            r#"<section id="{name}"><h3>{name} <small class="{cls}">{ok}/{n}</small></h3><p>{desc}</p>"#,
            name = esc(&scenario.name),
            cls = if ok == scenario.cases.len() {
                "ok"
            } else {
                "bad"
            },
            n = scenario.cases.len(),
            desc = esc(scenario.description.trim()),
        );
        for case in &scenario.cases {
            case_html(&mut out, case);
        }
        out.push_str("</section>");
    }
    out.push_str("</main></body></html>");
    out
}

fn case_html(out: &mut String, case: &CaseResult) {
    let open = if case.passed { "" } else { " open" };
    let _ = write!(
        out,
        r#"<details id="{id}" class="case {cls}"{open}><summary><span class="dot"></span><b>{combo}</b><span class="ms">{ms} ms</span></summary>"#,
        id = esc(&anchor(&case.scenario, &case.combo)),
        cls = if case.passed { "ok" } else { "bad" },
        combo = esc(&case.combo),
        ms = case.duration_ms,
    );
    if let Some(error) = &case.setup_error {
        let _ = write!(out, r#"<p class="err">Setup failed: {}</p>"#, esc(error));
    }
    out.push_str("<ol class=\"steps\">");
    for step in &case.steps {
        let class = match step.status {
            Status::Passed => "ok",
            Status::Failed => "bad",
            Status::Skipped => "skip",
        };
        let _ = write!(
            out,
            r#"<li class="{class}"><span class="d">{}</span><span class="t">{} ms</span>"#,
            esc(&step.description),
            step.duration_ms
        );
        if let Some(message) = &step.message {
            let _ = write!(out, r#"<div class="note">{}</div>"#, esc(message));
        }
        if let (Some(expected), Some(actual)) = (&step.expected, &step.actual) {
            let _ = write!(
                out,
                r#"<dl class="diff"><dt>expected</dt><dd>{}</dd><dt>actual</dt><dd>{}</dd></dl>"#,
                esc(expected),
                esc(actual)
            );
        }
        out.push_str("</li>");
    }
    out.push_str("</ol></details>");
}

pub fn junit(report: &Report) -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<testsuites>\n");
    for scenario in &report.scenarios {
        let failures = scenario.cases.iter().filter(|c| !c.passed).count();
        let _ = writeln!(
            out,
            r#"  <testsuite name="{}" tests="{}" failures="{failures}">"#,
            esc(&scenario.name),
            scenario.cases.len()
        );
        for case in &scenario.cases {
            let _ = write!(
                out,
                r#"    <testcase classname="{}" name="{}" time="{:.3}""#,
                esc(&scenario.name),
                esc(&case.combo),
                case.duration_ms as f64 / 1000.0
            );
            if case.passed {
                out.push_str("/>\n");
                continue;
            }
            let message = failure_text(case);
            let _ = writeln!(
                out,
                ">\n      <failure message=\"{}\">{}</failure>\n    </testcase>",
                esc(message.lines().next().unwrap_or("failed")),
                esc(&message)
            );
        }
        out.push_str("  </testsuite>\n");
    }
    out.push_str("</testsuites>\n");
    out
}

/// A plain-text account of why a case failed.
pub fn failure_text(case: &CaseResult) -> String {
    if let Some(error) = &case.setup_error {
        return format!("setup failed: {error}");
    }
    case.steps
        .iter()
        .find(|s| s.status == Status::Failed)
        .map(|s| {
            let mut text = format!("step {} failed: {}", s.index + 1, s.description);
            if let Some(m) = &s.message {
                let _ = write!(text, "\n  {m}");
            }
            if let (Some(e), Some(a)) = (&s.expected, &s.actual) {
                let _ = write!(text, "\n  expected: {e}\n  actual:   {a}");
            }
            text
        })
        .unwrap_or_else(|| "failed".to_string())
}

const STYLE: &str = r#"
:root{--bg:#f5f5f7;--card:#fff;--fg:#1d1d1f;--mut:#6e6e73;--line:#e5e5ea;--ok:#1a7f37;--okbg:#dff5e5;--bad:#c62828;--badbg:#fde4e4;--acc:#0a66d6}
@media (prefers-color-scheme:dark){:root{--bg:#111113;--card:#1c1c1f;--fg:#f2f2f4;--mut:#9a9aa3;--line:#2e2e33;--ok:#58d17a;--okbg:#14301d;--bad:#ff7b72;--badbg:#3a1a1a;--acc:#4da3ff}}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--fg);font:14px/1.5 -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,sans-serif}
main{max-width:1200px;margin:0 auto;padding:24px 16px 64px}h1{margin:0;font-size:28px}h2{margin:32px 0 12px;font-size:18px}h3{margin:0 0 4px;font-size:16px}
h3 small{font-size:12px;padding:2px 8px;border-radius:99px;margin-left:8px}small.ok{background:var(--okbg);color:var(--ok)}small.bad{background:var(--badbg);color:var(--bad)}
.sub{color:var(--mut);margin:2px 0 16px}.verdict{display:inline-block;padding:10px 16px;border-radius:12px;font-size:16px}
.verdict.pass{background:var(--okbg);color:var(--ok)}.verdict.fail{background:var(--badbg);color:var(--bad)}.verdict span{color:var(--mut);margin-left:8px;font-size:13px}
.scroll{overflow-x:auto;background:var(--card);border:1px solid var(--line);border-radius:12px}
table.matrix{border-collapse:collapse;width:100%;font-variant-numeric:tabular-nums}.matrix th,.matrix td{padding:6px 8px;text-align:center;border-bottom:1px solid var(--line);font-size:12px}
.matrix thead th{color:var(--mut);font-weight:600;white-space:nowrap}.matrix th.name{text-align:left;font-weight:500;font-size:13px;white-space:nowrap}
.matrix td a{display:block;border-radius:6px;padding:2px 0;text-decoration:none;font-weight:700}.matrix td.ok a{background:var(--okbg);color:var(--ok)}.matrix td.bad a{background:var(--badbg);color:var(--bad)}.matrix td.na{color:var(--line)}
a{color:var(--acc)}section{background:var(--card);border:1px solid var(--line);border-radius:12px;padding:16px;margin-bottom:16px}section>p{color:var(--mut);margin:0 0 12px}
details.case{border-top:1px solid var(--line);padding:6px 0}summary{cursor:pointer;display:flex;align-items:center;gap:8px}.dot{width:8px;height:8px;border-radius:50%;background:var(--ok)}.bad .dot{background:var(--bad)}
.ms,.t{color:var(--mut);font-size:12px;margin-left:auto}ol.steps{margin:8px 0 4px;padding-left:0;list-style:none}ol.steps li{display:flex;flex-wrap:wrap;gap:4px 8px;padding:3px 8px;border-left:3px solid var(--line);margin-bottom:2px;font-size:13px}
li.ok{border-left-color:var(--ok)}li.bad{border-left-color:var(--bad);background:var(--badbg)}li.skip{color:var(--mut)}li .t{margin-left:auto}.note,.err{width:100%;color:var(--bad);font-family:ui-monospace,Menlo,monospace;font-size:12px}
dl.diff{width:100%;display:grid;grid-template-columns:70px 1fr;gap:2px 8px;margin:4px 0 0;font-family:ui-monospace,Menlo,monospace;font-size:12px}dl.diff dt{color:var(--mut)}dl.diff dd{margin:0;word-break:break-word}
@media (max-width:600px){main{padding:16px}}
"#;
