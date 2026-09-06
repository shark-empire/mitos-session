use crate::session::SessionId;

/// One unlock/login attempt waiting to be checked.
#[derive(Debug, Clone)]
pub struct AuthRequest {
    pub session_id: SessionId,
    pub user_name: String,
    pub password: String,
}
