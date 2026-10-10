//! Reports for a matrix run: `report.html` (summary matrix and per-case
//! step timelines), `report.json` and `junit.xml`.

use std::fmt::Write as _;
use std::path::Path;

use crate::matrix::{Report, ScenarioReport};
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

/// `win-mac` as `win → mac`: the first device controls the others.
fn arrow(combo: &str) -> String {
    combo.replace('-', " \u{2192} ")
}

/// The matrix columns of one table: every cell label its scenarios use,
/// in matrix order (Windows, macOS, Linux).
fn columns<'a>(scenarios: &[&'a ScenarioReport]) -> Vec<&'a str> {
    let mut columns: Vec<&str> = Vec::new();
    for case in scenarios.iter().flat_map(|s| &s.cases) {
        if !columns.contains(&case.combo.as_str()) {
            columns.push(&case.combo);
        }
    }
    columns
}

/// Scenarios grouped by matrix width (pairs, triples), one table each.
fn tables(report: &Report) -> Vec<(usize, Vec<&ScenarioReport>)> {
    let mut tables: Vec<(usize, Vec<&ScenarioReport>)> = Vec::new();
    for scenario in &report.scenarios {
        let width = scenario
            .cases
            .first()
            .map_or(0, |c| c.combo.matches('-').count() + 1);
        match tables.iter_mut().find(|(w, _)| *w == width) {
            Some((_, list)) => list.push(scenario),
            None => tables.push((width, vec![scenario])),
        }
    }
    tables.sort_by_key(|(w, _)| *w);
    tables
}

pub fn html(report: &Report) -> String {
    let (passed, total) = (report.passed(), report.total());
    let failed = total - passed;
    let verdict = if report.all_passed() { "pass" } else { "fail" };
    let mut out = String::new();
    let _ = write!(
        out,
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Flow Harness Report</title><style>{STYLE}</style></head><body><main>
<header class="top"><div><h1>Flow harness</h1>
<p class="sub">Virtual daemons on emulated operating systems &middot; each cell is one OS combination, controller first</p></div>
<button id="theme" type="button" title="Switch light / dark">Theme</button></header>
<div class="stats {verdict}">
<div class="stat main"><span>Result</span><b>{verdict_text}</b></div>
<div class="stat"><span>Cases passed</span><b>{passed} / {total}</b></div>
<div class="stat"><span>Failed</span><b class="{failed_class}">{failed}</b></div>
<div class="stat"><span>Scenarios</span><b>{n}</b></div>
<div class="stat"><span>Run time</span><b>{secs:.1} s</b></div>
</div>"#,
        verdict_text = if report.all_passed() { "PASS" } else { "FAIL" },
        failed_class = if failed == 0 { "zero" } else { "bad" },
        secs = report.duration_ms as f64 / 1000.0,
        n = report.scenarios.len()
    );

    if failed > 0 {
        out.push_str(r#"<h2>Failures</h2><div class="failures">"#);
        for case in report.cases().filter(|c| !c.passed) {
            let _ = write!(
                out,
                r##"<a class="failure" href="#{}"><b>{}</b> <code>{}</code><pre>{}</pre><code class="rerun">rerun: cargo run -p flow-harness --release -- --scenario {} --pairs {}</code></a>"##,
                esc(&anchor(&case.scenario, &case.combo)),
                esc(&case.scenario),
                esc(&arrow(&case.combo)),
                esc(&failure_text(case)),
                esc(&case.scenario),
                esc(&case.combo)
            );
        }
        out.push_str("</div>");
    }

    for (width, scenarios) in tables(report) {
        let columns = columns(&scenarios);
        let title = match width {
            2 => "OS pairs".to_string(),
            3 => "OS triples".to_string(),
            n => format!("{n} devices"),
        };
        let _ = write!(
            out,
            r#"<h2>{title} <small>controller &rarr; controlled</small></h2><div class="scroll"><table class="matrix"><thead><tr><th class="name">Scenario</th>"#
        );
        for column in &columns {
            let _ = write!(out, "<th>{}</th>", esc(&arrow(column)));
        }
        out.push_str("</tr></thead><tbody>");
        for scenario in &scenarios {
            let _ = write!(
                out,
                r##"<tr><th class="name"><a href="#{0}">{0}</a></th>"##,
                esc(&scenario.name),
            );
            for column in &columns {
                match scenario.cases.iter().find(|c| c.combo == *column) {
                    Some(case) => {
                        let (class, mark) = if case.passed {
                            ("ok", "&#10003;")
                        } else {
                            ("bad", "&#10007;")
                        };
                        let _ = write!(
                            out,
                            r##"<td class="{class}"><a href="#{}" title="{} on {}: {} ms">{mark}</a></td>"##,
                            esc(&anchor(&scenario.name, column)),
                            esc(&scenario.name),
                            esc(&arrow(column)),
                            case.duration_ms
                        );
                    }
                    None => out.push_str(r#"<td class="na">&middot;</td>"#),
                }
            }
            out.push_str("</tr>");
        }
        out.push_str("</tbody></table></div>");
    }

    out.push_str(r#"<h2>Scenarios <small>open a case for its step timeline</small></h2>"#);
    for scenario in &report.scenarios {
        let ok = scenario.cases.iter().filter(|c| c.passed).count();
        let _ = write!(
            out,
            r#"<section id="{name}"><h3>{name} <small class="{cls}">{ok}/{n} passed</small></h3><p>{desc}</p>"#,
            name = esc(&scenario.name),
            cls = if ok == scenario.cases.len() {
                "ok"
            } else {
                "bad"
            },
            n = scenario.cases.len(),
            desc = esc(scenario.description.trim()),
        );
        // The first failing case opens; further failures stay one click away.
        let first_failure = scenario.cases.iter().position(|c| !c.passed);
        for (i, case) in scenario.cases.iter().enumerate() {
            case_html(&mut out, case, first_failure == Some(i));
        }
        out.push_str("</section>");
    }
    let _ = write!(out, "</main><script>{SCRIPT}</script></body></html>");
    out
}

fn case_html(out: &mut String, case: &CaseResult, open: bool) {
    let open = if open { " open" } else { "" };
    let ran = case
        .steps
        .iter()
        .filter(|s| s.status != Status::Skipped)
        .count();
    let _ = write!(
        out,
        r#"<details id="{id}" class="case {cls}"{open}><summary><span class="dot"></span><b>{combo}</b><span class="meta">{ran}/{steps} steps</span><span class="ms">{ms} ms</span></summary>"#,
        id = esc(&anchor(&case.scenario, &case.combo)),
        cls = if case.passed { "ok" } else { "bad" },
        combo = esc(&arrow(&case.combo)),
        steps = case.steps.len(),
        ms = case.duration_ms,
    );
    if let Some(error) = &case.setup_error {
        let _ = write!(out, r#"<p class="err">Setup failed: {}</p>"#, esc(error));
    }
    out.push_str("<ol class=\"steps\">");
    for step in &case.steps {
        let (class, mark) = match step.status {
            Status::Passed => ("ok", "&#10003;"),
            Status::Failed => ("bad", "&#10007;"),
            Status::Skipped => ("skip", "&ndash;"),
        };
        // A skipped step either had its `when` guard false (it says so) or
        // never ran because an earlier step failed.
        let time = match step.status {
            Status::Skipped if step.message.is_none() => "not run".to_string(),
            Status::Skipped => "skipped".to_string(),
            _ => format!("{} ms", step.duration_ms),
        };
        let _ = write!(
            out,
            r#"<li class="{class}"><span class="n">{n}</span><span class="m">{mark}</span><span class="d">{d}</span><span class="t">{time}</span>"#,
            n = step.index + 1,
            d = esc(&step.description),
        );
        if let Some(message) = &step.message {
            let _ = write!(out, r#"<div class="note">{}</div>"#, esc(message));
        }
        if let (Some(expected), Some(actual)) = (&step.expected, &step.actual) {
            let _ = write!(
                out,
                r#"<div class="diff"><div class="exp"><span>expected</span><pre>{}</pre></div><div class="act"><span>actual</span><pre>{}</pre></div></div>"#,
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
:root{--bg:#f4f5f7;--card:#fff;--fg:#1d1d1f;--mut:#6b6b73;--line:#e3e4e8;--ok:#1a7f37;--okbg:#e2f5e7;--bad:#c62828;--badbg:#fde8e8;--acc:#0a66d6;--code:#f2f3f5;color-scheme:light}
:root[data-theme=dark]{--bg:#111113;--card:#1b1b1e;--fg:#f0f0f3;--mut:#9a9aa3;--line:#2d2d33;--ok:#5fd27f;--okbg:#15301e;--bad:#ff8a80;--badbg:#3b1c1c;--acc:#5aa9ff;--code:#232328;color-scheme:dark}
@media (prefers-color-scheme:dark){:root:not([data-theme=light]){--bg:#111113;--card:#1b1b1e;--fg:#f0f0f3;--mut:#9a9aa3;--line:#2d2d33;--ok:#5fd27f;--okbg:#15301e;--bad:#ff8a80;--badbg:#3b1c1c;--acc:#5aa9ff;--code:#232328;color-scheme:dark}}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--fg);font:14px/1.5 -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,sans-serif}
main{max-width:1240px;margin:0 auto;padding:28px 16px 64px}h1{margin:0;font-size:26px;letter-spacing:-.01em}h2{margin:32px 0 10px;font-size:17px}h2 small,h3 small{font-weight:400;color:var(--mut);font-size:12px;margin-left:6px}h3{margin:0 0 4px;font-size:15px}
code,pre,.diff,.note,.err{font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace}
header.top{display:flex;justify-content:space-between;align-items:flex-start;gap:16px}.sub{color:var(--mut);margin:4px 0 18px}
#theme{font:inherit;font-size:12px;color:var(--mut);background:var(--card);border:1px solid var(--line);border-radius:8px;padding:5px 10px;cursor:pointer}
.stats{display:grid;grid-template-columns:repeat(auto-fit,minmax(140px,1fr));gap:10px}.stat{background:var(--card);border:1px solid var(--line);border-radius:12px;padding:10px 14px}
.stat span{display:block;color:var(--mut);font-size:12px}.stat b{font-size:20px;font-variant-numeric:tabular-nums}.stat b.bad{color:var(--bad)}.stat b.zero{color:var(--mut)}
.stats.pass .stat.main{background:var(--okbg);border-color:transparent}.stats.pass .stat.main b{color:var(--ok)}.stats.fail .stat.main{background:var(--badbg);border-color:transparent}.stats.fail .stat.main b{color:var(--bad)}
.failures{display:grid;gap:8px}a.failure{display:block;background:var(--card);border:1px solid var(--line);border-left:4px solid var(--bad);border-radius:10px;padding:10px 14px;color:var(--fg);text-decoration:none}
a.failure code{color:var(--mut);margin-left:6px}a.failure code.rerun{display:block;margin:6px 0 0;font-size:11px}a.failure pre{margin:6px 0 0;white-space:pre-wrap;word-break:break-word;font-size:12px;color:var(--mut)}
.scroll{overflow-x:auto;background:var(--card);border:1px solid var(--line);border-radius:12px}
table.matrix{border-collapse:collapse;width:100%;font-variant-numeric:tabular-nums}.matrix th,.matrix td{padding:5px 6px;text-align:center;border-bottom:1px solid var(--line);font-size:12px}.matrix tr:last-child>*{border-bottom:0}
.matrix thead th{color:var(--mut);font-weight:600;white-space:nowrap}.matrix th.name{text-align:left;font-weight:500;font-size:13px;white-space:nowrap;padding-left:14px}.matrix th.name a{color:var(--fg);text-decoration:none}.matrix th.name a:hover{color:var(--acc)}
.matrix td a{display:block;min-width:44px;border-radius:6px;padding:2px 0;text-decoration:none;font-weight:700}.matrix td.ok a{background:var(--okbg);color:var(--ok)}.matrix td.bad a{background:var(--bad);color:#fff}.matrix td.na{color:var(--line)}
a{color:var(--acc)}section{background:var(--card);border:1px solid var(--line);border-radius:12px;padding:16px;margin-bottom:12px;scroll-margin-top:12px}section>p{color:var(--mut);margin:0 0 10px;max-width:90ch}
h3 small{padding:2px 8px;border-radius:99px}h3 small.ok{background:var(--okbg);color:var(--ok)}h3 small.bad{background:var(--badbg);color:var(--bad)}
details.case{border-top:1px solid var(--line);padding:6px 0;scroll-margin-top:12px}summary{cursor:pointer;display:flex;align-items:center;gap:10px;padding:2px 0}.dot{width:8px;height:8px;border-radius:50%;background:var(--ok);flex:none}.bad>summary .dot{background:var(--bad)}
.meta{color:var(--mut);font-size:12px}.ms{color:var(--mut);font-size:12px;margin-left:auto;font-variant-numeric:tabular-nums}
ol.steps{margin:8px 0 6px;padding:0;list-style:none}ol.steps li{display:flex;flex-wrap:wrap;align-items:baseline;gap:2px 10px;padding:4px 10px;border-left:3px solid var(--line);margin-bottom:2px;font-size:13px}
li .n{color:var(--mut);font-size:11px;min-width:22px;text-align:right;font-variant-numeric:tabular-nums}li .m{width:12px;font-weight:700}li.ok .m{color:var(--ok)}li.bad .m{color:var(--bad)}li .d{flex:1;min-width:0}li .t{color:var(--mut);font-size:12px;font-variant-numeric:tabular-nums}
li.ok{border-left-color:var(--ok)}li.bad{border-left-color:var(--bad);background:var(--badbg);border-radius:0 8px 8px 0;padding:8px 10px}li.skip{color:var(--mut)}
.note,.err{width:100%;color:var(--bad);font-size:12px;margin:4px 0 0 44px}
.diff{width:100%;display:grid;gap:6px;margin:8px 0 2px 44px;max-width:calc(100% - 44px)}.diff>div{background:var(--card);border:1px solid var(--line);border-radius:8px;padding:6px 10px}
.diff span{display:block;font-size:11px;font-weight:700;text-transform:uppercase;letter-spacing:.05em}.diff .exp span{color:var(--ok)}.diff .act span{color:var(--bad)}.diff pre{margin:2px 0 0;white-space:pre-wrap;word-break:break-word;font-size:12px;line-height:1.55}
li.skip .note{color:var(--mut)}
@media (max-width:600px){main{padding:16px}.note,.err,.diff{margin-left:0;max-width:100%}}
"#;

/// The theme button: auto, then the opposite of the current, remembered.
const SCRIPT: &str = r##"
(function(){var r=document.documentElement,k='flow-harness-theme';
try{var s=localStorage.getItem(k);if(s)r.dataset.theme=s}catch(e){}
document.getElementById('theme').onclick=function(){
var dark=r.dataset.theme?r.dataset.theme==='dark':matchMedia('(prefers-color-scheme: dark)').matches;
r.dataset.theme=dark?'light':'dark';try{localStorage.setItem(k,r.dataset.theme)}catch(e){}};
if(location.hash){var t=document.getElementById(decodeURIComponent(location.hash.slice(1)));if(t&&t.tagName==='DETAILS')t.open=true}
document.addEventListener('click',function(e){var a=e.target.closest('a[href^="#"]');if(!a)return;var t=document.getElementById(decodeURIComponent(a.getAttribute('href').slice(1)));if(t&&t.tagName==='DETAILS')t.open=true});
})();
"##;
