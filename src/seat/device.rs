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

// TODO(stage 5 hardening): enumerate real devices via `udev` and keep
// this list live as devices are hot-plugged. Left unimplemented in
// this scaffold since it needs to be tested against real hardware --
// see README.md's roadmap.
