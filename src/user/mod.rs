//! Account lookup and privilege handling. This module is deliberately
//! thin: it does not maintain its own user database, it asks the real
//! one (`/etc/passwd`, `/etc/group`, `/etc/shadow` via PAM) every time,
//! so mitos-session never drifts out of sync with `useradd`/`usermod`
//! run outside of it.

mod account;
mod groups;
mod home;
mod permissions;
mod user;

pub use account::Account;
pub use groups::supplementary_gids;
pub use home::ensure_home_ready;
pub use permissions::drop_privileges;
pub use user::User;
