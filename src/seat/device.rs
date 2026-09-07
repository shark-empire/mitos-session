use crate::errors::Result;

/// A piece of hardware belonging to a seat. mitos-session only tracks
/// *metadata* about devices (for authorization and idle-detection
/// bookkeeping) -- it never opens or grabs them itself. Actual device
/// access is negotiated between mitos-gui and logind-style seat
/// arbitration (libseat) at the compositor layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub syspath: String,
    pub kind: DeviceKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    Keyboard,
    Pointer,
    Touch,
    Drm,
    Other,
}

/// Enumerate every currently-present input and DRM device via udev.
/// Called once at startup (see `main::run`) -- this is a snapshot, not
/// a live view: a device plugged in afterward won't show up here until
/// the daemon restarts. Real hotplug tracking would mean wiring a
/// `udev::MonitorSocket`'s raw fd into the calloop loop as its own
/// event source, which is a bigger change left for a follow-up (see
/// docs/architecture.md's "Known gaps").
///
/// NOTE: matches the `udev` crate's documented shape
/// (`Enumerator::new`/`match_subsystem`/`scan_devices`, all
/// `io::Result`-returning), but this box has no network access to
/// check it against whatever version `cargo update` resolves.
pub fn enumerate() -> Result<Vec<Device>> {
    let mut devices = Vec::new();
    for subsystem in ["input", "drm"] {
        let mut enumerator = udev::Enumerator::new()?;
        enumerator.match_subsystem(subsystem)?;
        for entry in enumerator.scan_devices()? {
            devices.push(Device {
                syspath: entry.syspath().to_string_lossy().into_owned(),
                kind: classify(subsystem, &entry),
            });
        }
    }
    Ok(devices)
}

/// Guess a device's kind from udev's own input-device tags where
/// available (`ID_INPUT_KEYBOARD` etc, set by udev's built-in
/// `60-input-id.rules`), falling back to the subsystem for anything
/// that isn't a tagged input node -- DRM cards don't get these tags.
fn classify(subsystem: &str, device: &udev::Device) -> DeviceKind {
    if subsystem == "drm" {
        return DeviceKind::Drm;
    }
    if device.property_value("ID_INPUT_KEYBOARD").is_some() {
        DeviceKind::Keyboard
    } else if device.property_value("ID_INPUT_TOUCHSCREEN").is_some() {
        DeviceKind::Touch
    } else if device.property_value("ID_INPUT_MOUSE").is_some()
        || device.property_value("ID_INPUT_TOUCHPAD").is_some()
    {
        DeviceKind::Pointer
    } else {
        DeviceKind::Other
    }
}
