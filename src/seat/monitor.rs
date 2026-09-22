use crate::errors::{Result, SessionError};
use crate::seat::device::{Device, DeviceKind};
use calloop::{generic::Generic, Interest, LoopHandle, Mode, PostAction};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, RawFd};
use udev::{EventType, MonitorBuilder, MonitorSocket};

/// A live device event observed from `udev`.
#[derive(Debug, Clone)]
pub enum DeviceEvent {
    Added(Device),
    Removed(Device),
    Changed(Device),
}

/// A hotplug event plus the seat it belongs to.
///
/// If udev did not provide `ID_SEAT`, the monitor falls back to the
/// daemon's default seat.
#[derive(Debug, Clone)]
pub struct HotplugEvent {
    pub seat: String,
    pub event: DeviceEvent,
}

/// Implemented by whatever state object owns seat/device bookkeeping.
///
/// In production this is the `Daemon` struct in `main.rs`.
pub trait HotplugSink {
    fn handle_hotplug(&mut self, event: HotplugEvent);
}

/// Newtype wrapper so we can implement `AsFd` for `MonitorSocket`.
///
/// `calloop::generic::Generic` wants an `AsFd` source. The `udev`
/// crate exposes a raw fd, so we borrow it for the lifetime of the
/// wrapper.
struct UdevMonitorFd(MonitorSocket);

impl AsFd for UdevMonitorFd {
    fn as_fd(&self) -> BorrowedFd<'_> {
        // SAFETY:
        //
        // `MonitorSocket` owns the underlying fd and keeps it alive
        // for the lifetime of this wrapper. We are borrowing it, not
        // transferring ownership.
        unsafe { BorrowedFd::borrow_raw(self.0.as_raw_fd()) }
    }
}

/// Register a live udev monitor on the calloop event loop.
///
/// This watches:
///
/// - `input` devices: keyboards, mice, touchpads, touchscreens
/// - `drm` devices: display cards/render nodes
///
/// Events are delivered to the shared state through [`HotplugSink`].
pub fn register<State>(handle: &LoopHandle<'_, State>, fallback_seat: String) -> Result<()>
where
    State: HotplugSink + 'static,
{
    let socket = MonitorBuilder::new()?
        .match_subsystem("input")?
        .match_subsystem("drm")?
        .listen()?;

    set_nonblocking(socket.as_raw_fd())?;

    let source = Generic::new(UdevMonitorFd(socket), Interest::READ, Mode::Level);

    handle
        .insert_source(source, move |_readiness, monitor, state: &mut State| {
            for event in monitor.0.iter() {
                if let Some(hotplug) = classify_event(&event, &fallback_seat) {
                    state.handle_hotplug(hotplug);
                }
            }

            Ok::<_, std::io::Error>(PostAction::Continue)
        })
        .map_err(|e| SessionError::Protocol(format!("failed to register udev monitor: {e}")))?;

    Ok(())
}

fn classify_event(event: &udev::Event, fallback_seat: &str) -> Option<HotplugEvent> {
    let subsystem = event.subsystem()?.to_string_lossy().into_owned();

    if subsystem != "input" && subsystem != "drm" {
        return None;
    }

    let syspath = event.syspath().to_string_lossy().into_owned();

    let kind = match subsystem.as_str() {
        "drm" => DeviceKind::Drm,
        "input" => classify_input(event),
        _ => return None,
    };

    let device = Device { syspath, kind };

    let seat = event
        .property_value("ID_SEAT")
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| fallback_seat.to_string());

    let event = match event.event_type() {
        EventType::Add => DeviceEvent::Added(device),
        EventType::Remove => DeviceEvent::Removed(device),
        EventType::Change => DeviceEvent::Changed(device),
        _ => return None,
    };

    Some(HotplugEvent { seat, event })
}

fn classify_input(event: &udev::Event) -> DeviceKind {
    // This matches the existing startup enumeration logic in
    // `src/seat/device.rs`: presence of the udev property is treated
    // as true.
    if event.property_value("ID_INPUT_KEYBOARD").is_some() {
        DeviceKind::Keyboard
    } else if event.property_value("ID_INPUT_TOUCHSCREEN").is_some() {
        DeviceKind::Touch
    } else if event.property_value("ID_INPUT_MOUSE").is_some()
        || event.property_value("ID_INPUT_TOUCHPAD").is_some()
    {
        DeviceKind::Pointer
    } else {
        DeviceKind::Other
    }
}

fn set_nonblocking(fd: RawFd) -> std::io::Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL, 0) };

    if flags < 0 {
        return Err(std::io::Error::last_os_error());
    }

    let rc = unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };

    if rc < 0 {
        return Err(std::io::Error::last_os_error());
    }

    Ok(())
}
