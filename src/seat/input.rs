use super::device::{Device, DeviceKind};

/// A keyboard/pointer/touch device, as reported by the compositor.
/// mitos-session doesn't read raw input events (that stays in
/// mitos-gui/libinput) -- it only receives coalesced "activity
/// happened" pings used to drive idle detection (see
/// `idle::tracker`).
#[derive(Debug, Clone)]
pub struct InputDevice {
    pub device: Device,
    pub name: String,
}

impl InputDevice {
    pub fn is_pointer_or_keyboard(&self) -> bool {
        matches!(
            self.device.kind,
            DeviceKind::Keyboard | DeviceKind::Pointer | DeviceKind::Touch
        )
    }
}
