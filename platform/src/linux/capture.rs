//! [`LinuxInputCapture`]: reads keyboard/mouse events from every
//! keyboard/mouse-capable evdev node and forwards translated
//! [`InputEvent`]s over a channel.
//!
//! evdev reads block, so the read loop runs on its own thread
//! (`daemon/todos.json` E1) rather than tying up an async runtime worker.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use evdev::Device;
use flow_core::input::InputCapture;
use flow_core::protocol::InputEvent;

use super::discovery::discover_devices;
use super::translate::EventTranslator;

/// How long the capture loop sleeps after a pass over every device found
/// nothing to read, before polling again.
const IDLE_POLL_INTERVAL: Duration = Duration::from_millis(5);

/// Captures keyboard/mouse input via evdev and forwards it as
/// [`InputEvent`]s on the channel given at construction.
///
/// The `InputCapture` trait only has `start()`/`stop()` — no way to hand
/// back captured events — so the channel is supplied up front instead of
/// returned from `start()`.
pub struct LinuxInputCapture {
    sender: Sender<InputEvent>,
    stop_flag: Arc<AtomicBool>,
    /// Whether the read loop should hold an exclusive `EVIOCGRAB` on
    /// every device it reads (`InputCapture::set_suppress_local`). Read
    /// by the loop rather than applied directly here: the `Device`
    /// handles are owned by the read thread, so this flag is the only
    /// way to reach them once `start()` has moved them across.
    suppress_flag: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl LinuxInputCapture {
    pub fn new(sender: Sender<InputEvent>) -> Self {
        Self {
            sender,
            stop_flag: Arc::new(AtomicBool::new(false)),
            suppress_flag: Arc::new(AtomicBool::new(false)),
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
        let suppress_flag = self.suppress_flag.clone();
        let sender = self.sender.clone();
        self.worker = Some(thread::spawn(move || {
            run_capture_loop(devices, &stop_flag, &suppress_flag, &sender)
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

    /// Publishes the request; the read loop applies the actual
    /// `EVIOCGRAB`/`EVIOCGRAB(0)` on its next pass (within
    /// [`IDLE_POLL_INTERVAL`] when idle, sooner when events are
    /// flowing). Deliberately doesn't block on that: an ioctl failing on
    /// one device shouldn't leave the caller's switch half-applied, and
    /// the loop already logs-and-continues per device the same way it
    /// does for a node erroring mid-run.
    fn set_suppress_local(&mut self, suppress: bool) -> Result<(), Self::Error> {
        if self.worker.is_none() {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "input capture is not running",
            ));
        }
        self.suppress_flag.store(suppress, Ordering::SeqCst);
        Ok(())
    }
}

fn run_capture_loop(
    mut devices: Vec<Device>,
    stop_flag: &AtomicBool,
    suppress_flag: &AtomicBool,
    sender: &Sender<InputEvent>,
) {
    let mut translator = EventTranslator::new();
    // Tracks what's actually been applied, so grab/ungrab runs only on a
    // real transition rather than re-issuing the ioctl every pass.
    let mut grabbed = false;
    while !stop_flag.load(Ordering::SeqCst) {
        let want_grabbed = suppress_flag.load(Ordering::SeqCst);
        if want_grabbed != grabbed {
            apply_grab(&mut devices, want_grabbed);
            grabbed = want_grabbed;
        }

        let mut read_any = false;
        let mut receiver_gone = false;
        for device in &mut devices {
            match device.fetch_events() {
                Ok(events) => {
                    for raw_event in events {
                        read_any = true;
                        if let Some(event) = translator.translate(raw_event) {
                            if sender.send(event).is_err() {
                                // Receiver dropped: nothing left to forward to.
                                receiver_gone = true;
                                break;
                            }
                        }
                    }
                }
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => {}
                // A device node erroring (e.g. unplugged mid-run) shouldn't
                // take down capture from the others.
                Err(_) => {}
            }
            if receiver_gone {
                break;
            }
        }
        if receiver_gone {
            break;
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
