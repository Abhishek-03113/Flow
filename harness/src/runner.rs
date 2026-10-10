//! Runs one scenario on one OS combination: builds the world, executes the
//! steps in order and records what happened. The runner knows nothing
//! about particular scenarios; they are data.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::time::{Duration, Instant};

use flow_core::control::layout::{Placement, ScreenLayout};
use flow_core::control::Role;
use flow_core::device::{HostOs, ScreenSize};
use flow_core::keymap::remap::RemapRule;
use flow_core::settings::SettingsPatch;
use flow_daemon::devmode::SecurityMode;
use serde::Serialize;

use crate::events::{observe, os_label, parse_button, parse_expected, Kind, Seen};
use crate::keys::key_for_char;
use crate::scenario::{Check, Combo, DeviceDecl, Hotkey, LinkDecl, Scenario, Step, StepKind};
use crate::world::{is_live, DeviceSpec, LinkExpect, LinkSpec, World, WorldOptions};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Passed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Serialize)]
pub struct StepResult {
    pub index: usize,
    pub description: String,
    pub status: Status,
    pub duration_ms: u64,
    /// What the step required, when it failed a comparison.
    pub expected: Option<String>,
    pub actual: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseResult {
    pub scenario: String,
    pub combo: String,
    pub passed: bool,
    pub duration_ms: u64,
    /// Why the world could not even be set up.
    pub setup_error: Option<String>,
    pub steps: Vec<StepResult>,
}

/// Why a step failed.
#[derive(Debug)]
pub enum Fail {
    /// The world disagreed with the scenario.
    Mismatch { expected: String, actual: String },
    /// The step could not run (bad scenario, engine error, timeout).
    Error(String),
}

impl From<String> for Fail {
    fn from(message: String) -> Self {
        Fail::Error(message)
    }
}

fn mismatch(expected: impl Into<String>, actual: impl Into<String>) -> Fail {
    Fail::Mismatch {
        expected: expected.into(),
        actual: actual.into(),
    }
}

/// Runs `scenario` on `combo`.
pub async fn run_case(scenario: &Scenario, combo: &Combo) -> CaseResult {
    let started = Instant::now();
    let mut case = CaseResult {
        scenario: scenario.name.clone(),
        combo: combo.label.clone(),
        passed: false,
        duration_ms: 0,
        setup_error: None,
        steps: Vec::new(),
    };
    let mut world = match build_world(scenario, combo).await {
        Ok(world) => world,
        Err(message) => {
            case.setup_error = Some(message);
            case.duration_ms = started.elapsed().as_millis() as u64;
            return case;
        }
    };

    let mut failed = false;
    // One entry per top-level step (a `repeat` body runs inside its parent).
    case.steps = scenario
        .steps
        .iter()
        .enumerate()
        .map(|(index, step)| StepResult {
            index,
            description: describe(step),
            status: Status::Skipped,
            duration_ms: 0,
            expected: None,
            actual: None,
            message: None,
        })
        .collect();
    for (i, step) in scenario.steps.iter().enumerate() {
        let result = &mut case.steps[i];
        if failed {
            continue;
        }
        let step_started = Instant::now();
        let outcome = run_step(&mut world, step).await;
        result.duration_ms = step_started.elapsed().as_millis() as u64;
        match outcome {
            Ok(true) => result.status = Status::Passed,
            Ok(false) => result.message = Some("skipped: its `when` condition is false".into()),
            Err(Fail::Mismatch { expected, actual }) => {
                result.status = Status::Failed;
                result.expected = Some(expected);
                result.actual = Some(actual);
                failed = true;
            }
            Err(Fail::Error(message)) => {
                result.status = Status::Failed;
                result.message = Some(message);
                failed = true;
            }
        }
    }
    world.shutdown();
    case.passed = !failed;
    case.duration_ms = started.elapsed().as_millis() as u64;
    case
}

fn resolve_os(text: &str, combo: &Combo) -> Result<HostOs, String> {
    let slot = match text {
        "$a" => Some(0),
        "$b" => Some(1),
        "$c" => Some(2),
        _ => None,
    };
    match slot {
        Some(i) => combo
            .oses
            .get(i)
            .copied()
            .ok_or_else(|| format!("{text} needs a matrix with more devices than {combo}")),
        None => crate::scenario::parse_os_short(text).ok_or_else(|| format!("unknown OS {text:?}")),
    }
}

fn default_devices() -> Vec<DeviceDecl> {
    vec![
        DeviceDecl {
            name: "ctl".into(),
            os: "$a".into(),
            role: Role::Controller,
            screen: None,
        },
        DeviceDecl {
            name: "ctd".into(),
            os: "$b".into(),
            role: Role::Controlled,
            screen: None,
        },
    ]
}

fn link_specs(scenario: &Scenario, decls: &[DeviceDecl]) -> Result<Vec<LinkSpec>, String> {
    let role_of = |name: &str| {
        decls
            .iter()
            .find(|d| d.name == name)
            .map(|d| d.role)
            .ok_or_else(|| format!("link names an unknown device {name:?}"))
    };
    let declared: Vec<LinkDecl> = if scenario.links.is_empty() {
        let controllers = decls.iter().filter(|d| d.role == Role::Controller);
        controllers
            .flat_map(|c| {
                decls
                    .iter()
                    .filter(|d| d.role == Role::Controlled)
                    .map(move |d| LinkDecl {
                        a: c.name.clone(),
                        b: d.name.clone(),
                        expect: None,
                    })
            })
            .collect()
    } else {
        scenario.links.clone()
    };
    declared
        .into_iter()
        .map(|link| {
            let expect = match link.expect.as_deref() {
                Some("live") => LinkExpect::Live,
                Some(other) => match other.strip_prefix("refused:") {
                    Some(reason) => LinkExpect::Refused(reason.to_string()),
                    None => {
                        return Err(format!(
                            "link expect {other:?}: use `live` or `refused:<reason>`"
                        ))
                    }
                },
                None if role_of(&link.a)? == role_of(&link.b)? => {
                    LinkExpect::Refused("role_conflict".to_string())
                }
                None => LinkExpect::Live,
            };
            Ok(LinkSpec {
                a: link.a,
                b: link.b,
                expect,
            })
        })
        .collect()
}

async fn build_world(scenario: &Scenario, combo: &Combo) -> Result<World, String> {
    let decls = if scenario.devices.is_empty() {
        default_devices()
    } else {
        scenario.devices.clone()
    };
    let specs = decls
        .iter()
        .map(|d| {
            let [width, height] = d.screen.unwrap_or([1920, 1080]);
            Ok(DeviceSpec {
                name: d.name.clone(),
                os: resolve_os(&d.os, combo)?,
                role: d.role,
                screen: ScreenSize { width, height },
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let links = link_specs(scenario, &decls)?;
    let mut opts = WorldOptions::default();
    match scenario.world.security.as_deref() {
        None | Some("secure") => {}
        Some("insecure") => opts.security = SecurityMode::Insecure,
        Some(other) => return Err(format!("world.security {other:?}: use secure or insecure")),
    }
    if let Some(ms) = scenario.world.debounce_ms {
        opts.debounce = Duration::from_millis(ms);
    }
    if let Some(ms) = scenario.world.timeout_ms {
        opts.timeout = Duration::from_millis(ms);
    }
    World::start(specs, links, opts).await
}

// ---- steps -----------------------------------------------------------------

type StepFuture<'a> = Pin<Box<dyn Future<Output = Result<bool, Fail>> + Send + 'a>>;

/// `Ok(true)` ran, `Ok(false)` skipped by its `when` guard.
fn run_step<'a>(world: &'a mut World, step: &'a Step) -> StepFuture<'a> {
    Box::pin(async move {
        if let Some(guard) = &step.when {
            if !guard_holds(world, guard)? {
                return Ok(false);
            }
        }
        run_kind(world, &step.kind).await?;
        Ok(true)
    })
}

/// `device=os` or `device!=os`.
fn guard_holds(world: &World, guard: &str) -> Result<bool, String> {
    let (name, os, negate) = match guard.split_once("!=") {
        Some((name, os)) => (name, os, true),
        None => {
            let (name, os) = guard
                .split_once('=')
                .ok_or_else(|| format!("`when = {guard:?}`: use `device=os` or `device!=os`"))?;
            (name, os, false)
        }
    };
    let wanted = crate::scenario::parse_os_short(os.trim())
        .ok_or_else(|| format!("`when = {guard:?}`: unknown OS"))?;
    let actual = world.device(name.trim())?.spec.os;
    Ok((actual == wanted) != negate)
}

async fn run_kind(world: &mut World, kind: &StepKind) -> Result<(), Fail> {
    match kind {
        StepKind::Press { on, key } => world.key(on, key, true)?,
        StepKind::Release { on, key } => world.key(on, key, false)?,
        StepKind::Type { on, text } => {
            for c in text.chars() {
                let key = key_for_char(c).ok_or_else(|| format!("cannot type {c:?}"))?;
                world.key(on, &key, true)?;
                world.key(on, &key, false)?;
            }
        }
        StepKind::Move { on, dx, dy, times } => {
            for _ in 0..*times {
                world.pointer_move(on, *dx, *dy)?;
            }
        }
        StepKind::ButtonDown { on, button } => world.button(on, parse_button(button)?, true)?,
        StepKind::ButtonUp { on, button } => world.button(on, parse_button(button)?, false)?,
        StepKind::Click { on, button } => {
            world.button(on, parse_button(button)?, true)?;
            world.button(on, parse_button(button)?, false)?;
        }
        StepKind::Scroll { on, dx, dy } => world.scroll(on, *dx, *dy)?,
        StepKind::PlaceCursor { on, x, y } => world.place_cursor(on, *x, *y)?,
        StepKind::Hotkey { on, which } => {
            let chord = world.hotkey_chord(on, *which == Hotkey::ReturnHome)?;
            for key in &chord {
                world.key(on, key, true)?;
            }
            for key in chord.iter().rev() {
                world.key(on, key, false)?;
            }
        }
        StepKind::SwitchTo {
            on,
            target,
            expect_error,
        } => {
            let id = world.id_of(on, target)?;
            let result = world.service(on)?.switch_to(&id.0).await;
            command_outcome(result.map_err(|e| e.code()), expect_error)?;
        }
        StepKind::ReleaseRequest { on, expect_error } => {
            let result = world.service(on)?.release_control().await;
            command_outcome(result.map_err(|e| e.code()), expect_error)?;
        }
        StepKind::Disconnect { device } => world.disconnect(device).await?,
        StepKind::Reconnect { device } => world.reconnect(device).await?,
        StepKind::SetSetting { on, key, value } => set_setting(world, on, key, value).await?,
        StepKind::SetRemaps { on, rules } => {
            let rules = rules
                .iter()
                .map(|r| RemapRule {
                    from: r.from.clone(),
                    to: r.to.clone(),
                    when_target_os: r.when_target_os,
                })
                .collect();
            world
                .service(on)?
                .set_remap_rules(rules)
                .await
                .map_err(|e| format!("set_remap_rules: {e}"))?;
        }
        StepKind::SetLayout { on, screens } => {
            let placements = screens
                .iter()
                .map(|s| {
                    let screen = if s.device == "local" {
                        world.device(on)?.spec.screen
                    } else {
                        world.device(&s.device)?.spec.screen
                    };
                    Ok(Placement {
                        device_id: world.id_of(on, &s.device)?,
                        x: s.x,
                        y: s.y,
                        width: screen.width,
                        height: screen.height,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            world
                .service(on)?
                .set_screen_layout(ScreenLayout {
                    screens: placements,
                })
                .await
                .map_err(|e| format!("set_screen_layout: {e}"))?;
        }
        StepKind::Clear { on } => {
            world.settle().await;
            world.clear(on.as_deref())?;
        }
        StepKind::Repeat { times, steps } => {
            for round in 1..=*times {
                for (i, step) in steps.iter().enumerate() {
                    run_step(world, step).await.map_err(|fail| match fail {
                        Fail::Error(m) => {
                            Fail::Error(format!("repeat {round}, step {}: {m}", i + 1))
                        }
                        other => other,
                    })?;
                }
            }
        }
        StepKind::Wait(check) => {
            let timeout = world.options().timeout;
            check_until(world, check, timeout, false).await?;
        }
        StepKind::Expect(check) => {
            let budget = world.options().timeout;
            check_until(world, check, budget, true).await?;
        }
    }
    Ok(())
}

fn command_outcome(
    result: Result<(), &'static str>,
    expect_error: &Option<String>,
) -> Result<(), Fail> {
    match (result, expect_error) {
        (Ok(()), None) => Ok(()),
        (Err(code), Some(want)) if code == want => Ok(()),
        (Ok(()), Some(want)) => Err(mismatch(format!("error {want}"), "accepted")),
        (Err(code), Some(want)) => Err(mismatch(format!("error {want}"), format!("error {code}"))),
        (Err(code), None) => Err(Fail::Error(format!("command refused: {code}"))),
    }
}

async fn set_setting(world: &World, on: &str, key: &str, value: &toml::Value) -> Result<(), Fail> {
    let json = serde_json::json!({ key: value });
    let patch: SettingsPatch =
        serde_json::from_value(json).map_err(|e| format!("setting {key}: {e}"))?;
    // Unknown keys are ignored by serde; a patch that lost the key is a typo.
    let echoed = serde_json::to_value(&patch).map_err(|e| e.to_string())?;
    if echoed.get(key).is_none() {
        return Err(Fail::Error(format!("unknown setting {key:?}")));
    }
    world
        .service(on)?
        .update_settings(patch)
        .await
        .map_err(|e| format!("update_settings: {e}"))?;
    Ok(())
}

// ---- checks ----------------------------------------------------------------

/// Re-evaluates `check` until it holds or `budget` runs out. With `quiet`
/// the world must have gone quiet first, so absence can be asserted.
async fn check_until(
    world: &World,
    check: &Check,
    budget: Duration,
    quiet: bool,
) -> Result<(), Fail> {
    let deadline = Instant::now() + budget;
    loop {
        if quiet {
            world.settle().await;
        }
        match evaluate(world, check) {
            Ok(()) => return Ok(()),
            Err(Fail::Error(message)) => return Err(Fail::Error(message)),
            Err(failure) if Instant::now() >= deadline => return Err(failure),
            Err(_) => tokio::time::sleep(Duration::from_millis(8)).await,
        }
    }
}

#[derive(Clone, Copy)]
enum Log {
    Injected,
    Delivered,
}

fn seen_log(world: &World, name: &str, log: Log, kinds: &[Kind]) -> Result<Vec<Seen>, String> {
    let device = world.device(name)?;
    let events = match log {
        Log::Injected => device.backend.injected(),
        Log::Delivered => device.backend.delivered_locally(),
    };
    Ok(events
        .iter()
        .map(|e| observe(&device.keyboard, e))
        .filter(|s| kinds.is_empty() || kinds.contains(&s.kind()))
        .collect())
}

fn expected_list(
    world: &World,
    name: &str,
    events: &[String],
    repeat: u32,
    kinds: &[Kind],
) -> Result<Vec<Seen>, String> {
    let keyboard = &world.device(name)?.keyboard;
    let once = events
        .iter()
        .map(|text| parse_expected(keyboard, text))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("{name} ({}): {e}", os_label(keyboard.os())))?;
    let once: Vec<Seen> = once
        .into_iter()
        .filter(|s| kinds.is_empty() || kinds.contains(&s.kind()))
        .collect();
    Ok((0..repeat).flat_map(|_| once.iter().cloned()).collect())
}

fn evaluate(world: &World, check: &Check) -> Result<(), Fail> {
    match check {
        Check::Active { on, is } => {
            let actual = world.active_screen(on)?;
            let service = world.service(on)?;
            let detail = format!(
                "{actual} (peers {}, key {:?}, capturing {})",
                service.router().peer_ids().len(),
                service.watch_settings().borrow().switch_key.keys,
                world.device(on)?.backend.is_capturing()
            );
            if *is == actual {
                Ok(())
            } else {
                Err(mismatch(is.clone(), detail))
            }
        }
        Check::Injected {
            on,
            events,
            repeat,
            kinds,
        } => same_events(
            expected_list(world, on, events, *repeat, kinds)?,
            seen_log(world, on, Log::Injected, kinds)?,
        ),
        Check::Delivered {
            on,
            events,
            repeat,
            kinds,
        } => same_events(
            expected_list(world, on, events, *repeat, kinds)?,
            seen_log(world, on, Log::Delivered, kinds)?,
        ),
        Check::NoLocalLeak { on, kinds } => {
            same_events(Vec::new(), seen_log(world, on, Log::Delivered, kinds)?)
        }
        Check::InjectedIncludes { on, events, kinds } => {
            let wanted = expected_list(world, on, events, 1, kinds)?;
            let got = seen_log(world, on, Log::Injected, kinds)?;
            let found = wanted.is_empty()
                || got
                    .windows(wanted.len())
                    .any(|w| w.iter().zip(&wanted).all(|(a, b)| a.same_as(b)));
            if found {
                Ok(())
            } else {
                Err(mismatch(
                    format!("includes, in order: {}", join(&wanted)),
                    join(&got),
                ))
            }
        }
        Check::PointerMoved {
            on,
            dx,
            dy,
            tolerance,
        } => {
            let (mut sx, mut sy) = (0, 0);
            for seen in seen_log(world, on, Log::Injected, &[Kind::Move])? {
                if let Seen::Move { dx, dy } = seen {
                    sx += dx;
                    sy += dy;
                }
            }
            if (sx - dx).abs() <= *tolerance && (sy - dy).abs() <= *tolerance {
                Ok(())
            } else {
                Err(mismatch(
                    format!("pointer moved ({dx}, {dy}) +-{tolerance}"),
                    format!("pointer moved ({sx}, {sy})"),
                ))
            }
        }
        Check::NoForwarding { on } => {
            let fed: std::collections::HashSet<u64> =
                world.fed_timestamps(on)?.into_iter().collect();
            let mut leaked = Vec::new();
            for other in world.devices().iter().filter(|d| d.spec.name != *on) {
                for event in other.backend.injected() {
                    if fed.contains(&event.timestamp_ms()) {
                        leaked.push(format!(
                            "{} received {}",
                            other.spec.name,
                            observe(&other.keyboard, &event)
                        ));
                    }
                }
            }
            if leaked.is_empty() {
                Ok(())
            } else {
                Err(mismatch(
                    format!("nothing typed on {on} reaches another device"),
                    leaked.join(", "),
                ))
            }
        }
        Check::KeyNeverSeen { key } => {
            let mut seen = Vec::new();
            for device in world.devices() {
                let name = &device.spec.name;
                for (log, label) in [
                    (Log::Injected, "injected into"),
                    (Log::Delivered, "delivered to"),
                ] {
                    for event in seen_log(world, name, log, &[Kind::Key])? {
                        if matches!(&event, Seen::Key { name: k, .. } if k == key) {
                            seen.push(format!("{event} {label} {name}"));
                        }
                    }
                }
            }
            if seen.is_empty() {
                Ok(())
            } else {
                Err(mismatch(
                    format!("{key} reaches no application"),
                    seen.join(", "),
                ))
            }
        }
        Check::NothingStuck { on } => {
            // A key or button is down until its release; repeated presses
            // (autorepeat) don't stack.
            let mut down: BTreeMap<String, bool> = BTreeMap::new();
            for seen in seen_log(world, on, Log::Injected, &[])? {
                match seen {
                    Seen::Key { down: d, name, .. } => {
                        down.insert(name, d);
                    }
                    Seen::Button { down: d, button } => {
                        down.insert(format!("{button:?} button"), d);
                    }
                    _ => {}
                }
            }
            let stuck: Vec<String> = down
                .into_iter()
                .filter(|(_, d)| *d)
                .map(|(k, _)| k)
                .collect();
            if stuck.is_empty() {
                Ok(())
            } else {
                Err(mismatch(
                    "every press released",
                    format!("still down: {}", stuck.join(", ")),
                ))
            }
        }
        Check::Suppressed { on, is } => equal(is, &world.device(on)?.backend.is_suppressed()),
        Check::Capturing { on, is } => equal(is, &world.device(on)?.backend.is_capturing()),
        Check::Entered { on, is } => {
            let entered = world.service(on)?.watch_control_state().borrow().entered;
            equal(is, &entered)
        }
        Check::Linked { on, peer, is } => {
            let devices = world.devices_of(on)?;
            let linked = devices.iter().any(|d| d.name == *peer && is_live(d));
            equal(is, &linked)
        }
        Check::LinkError { on, peer, is } => {
            let devices = world.devices_of(on)?;
            let device = devices
                .iter()
                .find(|d| d.name == *peer)
                .ok_or_else(|| format!("{on} does not know {peer}"))?;
            equal(
                is,
                &device.link_error.clone().unwrap_or_else(|| "none".into()),
            )
        }
        Check::Warped { on, x, y } => {
            let last = world.device(on)?.backend.warps().last().copied();
            equal(
                &format!("({x}, {y})"),
                &last.map_or("no warp".into(), |(x, y)| format!("({x}, {y})")),
            )
        }
    }
}

fn equal<T: PartialEq + std::fmt::Display>(expected: &T, actual: &T) -> Result<(), Fail> {
    if expected == actual {
        Ok(())
    } else {
        Err(mismatch(expected.to_string(), actual.to_string()))
    }
}

fn join(events: &[Seen]) -> String {
    const SHOWN: usize = 14;
    let mut parts: Vec<String> = events.iter().take(SHOWN).map(Seen::to_string).collect();
    if events.len() > SHOWN {
        parts.push(format!("... ({} events)", events.len()));
    }
    if parts.is_empty() {
        "(no events)".to_string()
    } else {
        parts.join(", ")
    }
}

fn same_events(expected: Vec<Seen>, actual: Vec<Seen>) -> Result<(), Fail> {
    let first_difference = expected
        .iter()
        .zip(&actual)
        .position(|(e, a)| !e.same_as(a));
    if first_difference.is_none() && expected.len() == actual.len() {
        return Ok(());
    }
    let at = first_difference.unwrap_or(expected.len().min(actual.len()));
    // Show a window around the first difference so long logs stay readable.
    let window = |list: &[Seen]| {
        let start = at.saturating_sub(3);
        let slice = &list[start.min(list.len())..(at + 6).min(list.len())];
        let mut text = join(slice);
        if start > 0 {
            text = format!("... {text}");
        }
        format!("{text}  [{} events total]", list.len())
    };
    Err(mismatch(
        format!(
            "{}  (first difference at event {})",
            window(&expected),
            at + 1
        ),
        window(&actual),
    ))
}

/// A one-line description of a step, for timelines.
pub fn describe(step: &Step) -> String {
    let body = match &step.kind {
        StepKind::Press { on, key } => format!("press {key} on {on}"),
        StepKind::Release { on, key } => format!("release {key} on {on}"),
        StepKind::Type { on, text } => format!("type {text:?} on {on}"),
        StepKind::Move { on, dx, dy, times } => {
            format!("move pointer ({dx}, {dy}) x{times} on {on}")
        }
        StepKind::ButtonDown { on, button } => format!("{button} button down on {on}"),
        StepKind::ButtonUp { on, button } => format!("{button} button up on {on}"),
        StepKind::Click { on, button } => format!("click {button} on {on}"),
        StepKind::Scroll { on, dx, dy } => format!("scroll ({dx}, {dy}) on {on}"),
        StepKind::PlaceCursor { on, x, y } => format!("place {on}'s pointer at ({x}, {y})"),
        StepKind::Hotkey { on, which } => match which {
            Hotkey::Switch => format!("press the switch hotkey on {on}"),
            Hotkey::ReturnHome => format!("press the return-home hotkey on {on}"),
        },
        StepKind::SwitchTo { on, target, .. } => format!("{on}: switch_to {target}"),
        StepKind::ReleaseRequest { on, .. } => format!("{on}: release_control"),
        StepKind::Disconnect { device } => format!("disconnect {device}"),
        StepKind::Reconnect { device } => format!("reconnect {device}"),
        StepKind::SetSetting { on, key, value } => format!("{on}: set {key} = {value}"),
        StepKind::SetRemaps { on, rules } => format!("{on}: set {} remap rule(s)", rules.len()),
        StepKind::SetLayout { on, screens } => {
            format!("{on}: set layout of {} screens", screens.len())
        }
        StepKind::Clear { on } => {
            format!("clear recorded input ({})", on.as_deref().unwrap_or("all"))
        }
        StepKind::Repeat { times, steps } => format!("repeat x{times}: {} step(s)", steps.len()),
        StepKind::Wait(check) => format!("wait until {}", describe_check(check)),
        StepKind::Expect(check) => format!("expect {}", describe_check(check)),
    };
    match &step.when {
        Some(guard) => format!("{body} (when {guard})"),
        None => body,
    }
}

fn describe_check(check: &Check) -> String {
    match check {
        Check::Active { on, is } => format!("{on}'s active screen is {is}"),
        Check::Injected {
            on, events, repeat, ..
        } => format!("{on} was injected exactly {}", event_list(events, *repeat)),
        Check::InjectedIncludes { on, events, .. } => {
            format!("{on} was injected, among others, {}", event_list(events, 1))
        }
        Check::PointerMoved { on, dx, dy, .. } => {
            format!("{on}'s injected pointer moved ({dx}, {dy})")
        }
        Check::Delivered {
            on, events, repeat, ..
        } => format!(
            "{on}'s own apps got exactly {}",
            event_list(events, *repeat)
        ),
        Check::NoLocalLeak { on, .. } => format!("nothing leaked to {on}'s own apps"),
        Check::NoForwarding { on } => format!("nothing from {on} is forwarded"),
        Check::KeyNeverSeen { key } => format!("{key} never reaches any application"),
        Check::NothingStuck { on } => format!("no key or button stuck on {on}"),
        Check::Suppressed { on, is } => format!("{on} suppressed = {is}"),
        Check::Capturing { on, is } => format!("{on} capturing = {is}"),
        Check::Entered { on, is } => format!("{on} entered = {is}"),
        Check::Linked { on, peer, is } => format!("{on} linked to {peer} = {is}"),
        Check::LinkError { on, peer, is } => format!("{on}'s link error for {peer} is {is}"),
        Check::Warped { on, x, y } => format!("{on}'s pointer warped to ({x}, {y})"),
    }
}

/// `[down A, up A] x3` for a step description, shortened when long.
fn event_list(events: &[String], repeat: u32) -> String {
    const SHOWN: usize = 6;
    if events.is_empty() {
        return "nothing".to_string();
    }
    let mut text = events
        .iter()
        .take(SHOWN)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if events.len() > SHOWN {
        text.push_str(&format!(", ... ({} events)", events.len()));
    }
    if repeat > 1 {
        format!("[{text}] x{repeat}")
    } else {
        format!("[{text}]")
    }
}
