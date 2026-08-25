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
    worker: Option<JoinHandle<()>>,
}

impl LinuxInputCapture {
    pub fn new(sender: Sender<InputEvent>) -> Self {
        Self {
            sender,
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
        let sender = self.sender.clone();
        self.worker = Some(thread::spawn(move || {
            run_capture_loop(devices, &stop_flag, &sender)
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

fn run_capture_loop(mut devices: Vec<Device>, stop_flag: &AtomicBool, sender: &Sender<InputEvent>) {
    let mut translator = EventTranslator::new();
    while !stop_flag.load(Ordering::SeqCst) {
        let mut read_any = false;
        for device in &mut devices {
            match device.fetch_events() {
                Ok(events) => {
                    for raw_event in events {
                        read_any = true;
                        if let Some(event) = translator.translate(raw_event) {
                            if sender.send(event).is_err() {
                                // Receiver dropped: nothing left to forward to.
                                return;
                            }
                        }
                    }
                }
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => {}
                // A device node erroring (e.g. unplugged mid-run) shouldn't
                // take down capture from the others.
                Err(_) => {}
            }
        }
        if !read_any {
            thread::sleep(IDLE_POLL_INTERVAL);
        }
    }
}
