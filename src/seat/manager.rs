use super::seat::Seat;
use crate::errors::{Result, SessionError};
use crate::session::SessionId;
use std::collections::HashMap;

/// Registry of every seat MITOS knows about, plus the logic for moving
/// sessions on and off the "active" slot of a seat (VT switching, in
/// single-seat terms).
#[derive(Debug, Default)]
pub struct SeatManager {
    seats: HashMap<String, Seat>,
}

impl SeatManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ensure a seat exists, creating it if this is the first time
    /// we've heard of it. Called for `default_seat` at startup and for
    /// any seat a compositor registers against.
    pub fn ensure_seat(&mut self, id: &str) -> &mut Seat {
        self.seats
            .entry(id.to_string())
            .or_insert_with(|| Seat::new(id))
    }

    pub fn seat(&self, id: &str) -> Result<&Seat> {
        self.seats
            .get(id)
            .ok_or_else(|| SessionError::UnknownSeat(id.to_string()))
    }

    pub fn seat_mut(&mut self, id: &str) -> Result<&mut Seat> {
        self.seats
            .get_mut(id)
            .ok_or_else(|| SessionError::UnknownSeat(id.to_string()))
    }

    pub fn seats(&self) -> impl Iterator<Item = &Seat> {
        self.seats.values()
    }

    /// Attach a newly-created session to a seat. If the seat has no
    /// active session yet, the new one becomes active immediately;
    /// otherwise it's queued behind whatever is already running (the
    /// same behaviour as logging in on an already-occupied VT).
    pub fn attach_session(&mut self, seat_id: &str, session: SessionId) {
        let seat = self.ensure_seat(seat_id);
        if seat.active_session.is_none() {
            seat.active_session = Some(session);
        } else {
            seat.queued_sessions.push_back(session);
        }
    }

    /// Remove a session from whichever seat it's on (logout). If it
    /// was the active session, the next queued one (if any) is
    /// promoted, mirroring what happens when you log out of the
    /// foreground VT.
    pub fn detach_session(&mut self, session: SessionId) {
        for seat in self.seats.values_mut() {
            if seat.active_session == Some(session) {
                seat.active_session = seat.queued_sessions.pop_front();
            } else {
                seat.queued_sessions.retain(|&s| s != session);
            }
        }
    }

    /// Make `session` the active one on `seat_id`, pushing whatever
    /// was active back onto the queue. Returns the session that was
    /// active before the switch, if any -- callers use this to tell
    /// the outgoing session's compositor to stop rendering.
    pub fn switch_active(
        &mut self,
        seat_id: &str,
        session: SessionId,
    ) -> Result<Option<SessionId>> {
        let seat = self.seat_mut(seat_id)?;
        if !seat.has_session(session) {
            return Err(SessionError::UnknownSession(session));
        }
        let previous = seat.active_session;
        if previous != Some(session) {
            if let Some(prev) = previous {
                seat.queued_sessions.push_back(prev);
            }
            seat.queued_sessions.retain(|&s| s != session);
            seat.active_session = Some(session);
        }
        Ok(previous.filter(|&p| p != session))
    }

    pub fn active_session(&self, seat_id: &str) -> Result<Option<SessionId>> {
        Ok(self.seat(seat_id)?.active_session)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_session_becomes_active() {
        let mut mgr = SeatManager::new();
        mgr.attach_session("seat0", 1);
        assert_eq!(mgr.active_session("seat0").unwrap(), Some(1));
    }

    #[test]
    fn second_session_is_queued_then_promoted_on_logout() {
        let mut mgr = SeatManager::new();
        mgr.attach_session("seat0", 1);
        mgr.attach_session("seat0", 2);
        assert_eq!(mgr.active_session("seat0").unwrap(), Some(1));

        mgr.detach_session(1);
        assert_eq!(mgr.active_session("seat0").unwrap(), Some(2));
    }

    #[test]
    fn switch_active_round_trips() {
        let mut mgr = SeatManager::new();
        mgr.attach_session("seat0", 1);
        mgr.attach_session("seat0", 2);

        let previous = mgr.switch_active("seat0", 2).unwrap();
        assert_eq!(previous, Some(1));
        assert_eq!(mgr.active_session("seat0").unwrap(), Some(2));
    }
}
