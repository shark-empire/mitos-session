use crate::config::SessionSettings;
use crate::session::SessionType;

/// Session-related limits and defaults, resolved once from config.
#[derive(Debug, Clone)]
pub struct SessionPolicy {
    pub max_sessions_per_user: u32,
    pub default_session_type: SessionType,
}

impl From<&SessionSettings> for SessionPolicy {
    fn from(s: &SessionSettings) -> Self {
        Self {
            max_sessions_per_user: s.max_sessions_per_user,
            default_session_type: parse_session_type(&s.default_session_type),
        }
    }
}

fn parse_session_type(s: &str) -> SessionType {
    match s {
        "x11" => SessionType::X11,
        "tty" => SessionType::Tty,
        _ => SessionType::Wayland,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_type_falls_back_to_wayland() {
        assert_eq!(parse_session_type("carrier-pigeon"), SessionType::Wayland);
        assert_eq!(parse_session_type("x11"), SessionType::X11);
        assert_eq!(parse_session_type("tty"), SessionType::Tty);
    }
}
