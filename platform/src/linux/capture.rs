//! [`LinuxInputCapture`]: reads keyboard/mouse events from every
//! keyboard/mouse-capable evdev node and forwards translated
//! [`InputEvent`]s over a channel.
//!
//! evdev reads block, so the read loop runs on its own thread
//! (`docs/archive/tasks/daemon-todos.json` E1) rather than tying up an async runtime worker.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use evdev::Device;
use flow_core::input::{CaptureSink, Disposition, InputCapture, InputInjector};

use super::discovery::discover_devices;
use super::injector::LinuxInputInjector;
use super::passthrough::PassThrough;
use super::translate::EventTranslator;

/// How long the capture loop sleeps after a pass over every device found
/// nothing to read, before polling again.
const IDLE_POLL_INTERVAL: Duration = Duration::from_millis(5);

/// Captures keyboard/mouse input via evdev and hands each translated
/// event to the [`CaptureSink`].
///
/// evdev delivers events to every reader at once, so a per-event decision
/// can't hide an event that has already been read; instead the devices are
/// grabbed exclusively (`EVIOCGRAB`) while the sink says input is hidden.
/// The first event of a switch can therefore still reach local
/// applications.
pub struct LinuxInputCapture {
    sink: Arc<dyn CaptureSink>,
    stop_flag: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl LinuxInputCapture {
    pub fn new(sink: Arc<dyn CaptureSink>) -> Self {
        Self {
            sink,
            stop_flag: Arc::new(AtomicBool::new(false)),
            worker: None,
        }
    }
}

impl InputCapture for LinuxInputCapture {
    type Error = io::Error;

    fn start(&mut self) -> Result<(), Self::Error> {
        if self.worker.is_some() {
            return Ok(());
        }

        let mut devices = discover_devices();
        if devices.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "no keyboard- or mouse-capable /dev/input device found",
            ));
        }
        for device in &mut devices {
            device.set_nonblocking(true)?;
        }

        self.stop_flag.store(false, Ordering::SeqCst);
        let stop_flag = self.stop_flag.clone();
        let sink = self.sink.clone();
        // Pass events must still reach the local session while grabbed.
        // Without uinput access capture still works, but a `Pass` while
        // grabbed is then lost.
        let injector = match LinuxInputInjector::new() {
            Ok(injector) => Some(injector),
            Err(err) => {
                tracing::warn!("no uinput pass-through for grabbed input: {err}");
                None
            }
        };
        self.worker = Some(thread::spawn(move || {
            run_capture_loop(devices, &stop_flag, sink.as_ref(), injector)
        }));
        Ok(())
    }

    fn stop(&mut self) -> Result<(), Self::Error> {
        self.stop_flag.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| io::Error::other("input capture thread panicked"))?;
        }
        Ok(())
    }
}

fn run_capture_loop(
    mut devices: Vec<Device>,
    stop_flag: &AtomicBool,
    sink: &dyn CaptureSink,
    mut injector: Option<LinuxInputInjector>,
) {
    let mut translator = EventTranslator::new();
    let mut pass = PassThrough::default();
    // What is actually applied, so the ioctl runs only on a transition.
    let mut grabbed = false;
    while !stop_flag.load(Ordering::SeqCst) {
        sync_grab(sink, &mut devices, &mut grabbed, &mut pass, &mut injector);

        let mut read_any = false;
        for index in 0..devices.len() {
            let events = match devices[index].fetch_events() {
                Ok(events) => events.collect::<Vec<_>>(),
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => continue,
                // A node erroring (unplugged mid-run) must not stop the others.
                Err(_) => continue,
            };
            for raw_event in events {
                read_any = true;
                if let Some(event) = translator.translate(raw_event) {
                    pass.observe(&event);
                    let disposition = sink.on_event(&event);
                    // Grab as soon as the sink starts hiding input, not on
                    // the next poll.
                    if disposition == Disposition::Swallow || grabbed {
                        sync_grab(sink, &mut devices, &mut grabbed, &mut pass, &mut injector);
                    }
                    // A grabbed device delivers nothing locally, so
                    // re-emit what the router let pass.
                    if let Some(local) = pass.forward(&event, disposition, grabbed) {
                        emit_local(injector.as_mut(), &[local]);
                    }
                }
            }
        }
        if !read_any {
            thread::sleep(IDLE_POLL_INTERVAL);
        }
    }

    // Never leave the user's keyboard/mouse grabbed on the way out. The
    // kernel does release an `EVIOCGRAB` when the fd closes (so process
    // death can't permanently capture a keyboard), but this loop can end
    // while the devices themselves stay alive a little longer in the
    // caller's hands — releasing explicitly means a `stop()` restores
    // local input immediately rather than whenever the `Vec` happens to
    // drop.
    if grabbed {
        apply_grab(&mut devices, false);
        emit_local(injector.as_mut(), &pass.on_ungrab());
    }
}

/// Grabs or releases the devices when the sink's wish changed, repairing
/// local key state across the transition.
fn sync_grab(
    sink: &dyn CaptureSink,
    devices: &mut [Device],
    grabbed: &mut bool,
    pass: &mut PassThrough,
    injector: &mut Option<LinuxInputInjector>,
) {
    let want = sink.suppressing();
    if want != *grabbed {
        apply_grab(devices, want);
        *grabbed = want;
        let events = if want {
            pass.on_grab()
        } else {
            pass.on_ungrab()
        };
        emit_local(injector.as_mut(), &events);
    }
}

/// Delivers events to the local session through the virtual device.
/// Best-effort: a failed emit must not stop capture.
fn emit_local(
    injector: Option<&mut LinuxInputInjector>,
    events: &[flow_core::protocol::InputEvent],
) {
    let Some(injector) = injector else { return };
    for event in events {
        if let Err(err) = injector.inject(event) {
            tracing::warn!("local pass-through failed: {err}");
        }
    }
}

/// Applies (or releases) an exclusive `EVIOCGRAB` across every captured
/// device. A failure on one device is logged past rather than aborting
/// the rest: a partial grab still suppresses most local input, and the
/// alternative — bailing out halfway — would leave an inconsistent mix
/// with no path back. Grabbing is best-effort by nature (a device can
/// be unplugged, or already grabbed by another process).
fn apply_grab(devices: &mut [Device], grab: bool) {
    for device in devices {
        let result = if grab { device.grab() } else { device.ungrab() };
        if let Err(err) = result {
            let name = device.name().unwrap_or("<unnamed>").to_string();
            let verb = if grab { "grab" } else { "ungrab" };
            tracing::warn!("failed to {verb} input device {name:?}: {err}");
        }
    }
}
