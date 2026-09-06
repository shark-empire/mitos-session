/// Which output backend a seat's active session is being driven by.
/// Populated once the compositor registers itself over IPC
/// (`ipc::messages::Request::RegisterCompositor`) and reports what it
/// actually opened.
#[derive(Debug, Clone)]
pub struct Display {
    pub backend: DisplayBackend,
    /// e.g. `wayland-1`; set once the compositor's socket is live.
    pub wayland_socket: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayBackend {
    /// Real KMS/DRM output -- a normal boot into MITOS.
    Drm,
    /// Nested inside another Wayland/X11 session -- development mode.
    Nested,
}

impl Display {
    pub fn new(backend: DisplayBackend) -> Self {
        Self {
            backend,
            wayland_socket: None,
        }
    }
}
