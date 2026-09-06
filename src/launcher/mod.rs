//! Spawns processes on behalf of a session: the compositor, autostart
//! applications, and a fallback terminal if the compositor won't stay
//! up. Everything funnels through `Application::spawn`, which is the
//! one place privileges actually get dropped before `exec`.

mod application;
mod compositor;
mod desktop;
mod terminal;

pub use application::Application;
pub use compositor::spawn_compositor;
pub use desktop::spawn_autostart;
pub use terminal::spawn_fallback_terminal;
