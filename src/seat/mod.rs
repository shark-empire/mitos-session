//! Seats: the grouping of one keyboard/mouse/display set that exactly
//! one session can be "active" on at a time (the classic multi-seat
//! concept, and the reason VT switching exists on a single-seat box).

mod device;
mod display;
mod input;
mod manager;
mod seat;

pub use device::{enumerate, Device, DeviceKind};
pub use display::{Display, DisplayBackend};
pub use input::InputDevice;
pub use manager::SeatManager;
pub use seat::Seat;
