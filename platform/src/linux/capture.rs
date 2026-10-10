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
use flow_core::input::{CaptureSink, Disposition, InputCapture};

use super::discovery::discover_devices;
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
        self.worker = Some(thread::spawn(move || {
            run_capture_loop(devices, &stop_flag, sink.as_ref())
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

fn run_capture_loop(mut devices: Vec<Device>, stop_flag: &AtomicBool, sink: &dyn CaptureSink) {
    let mut translator = EventTranslator::new();
    // What is actually applied, so the ioctl runs only on a transition.
    let mut grabbed = false;
    let sync_grab = |devices: &mut Vec<Device>, grabbed: &mut bool| {
        let want = sink.suppressing();
        if want != *grabbed {
            apply_grab(devices, want);
            *grabbed = want;
        }
    };
    while !stop_flag.load(Ordering::SeqCst) {
        sync_grab(&mut devices, &mut grabbed);

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
                    // Grab as soon as the sink starts hiding input, not on
                    // the next poll.
                    if sink.on_event(&event) == Disposition::Swallow || grabbed {
                        sync_grab(&mut devices, &mut grabbed);
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
