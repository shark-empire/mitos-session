use super::policy::ElevationPolicy;
use super::timeout::ElevationTimeouts;
use crate::authentication::{check, AuthOutcome, AuthPolicy, AuthRequest, Authenticator};
use crate::errors::{Result, SessionError};
use crate::ipc::ConnId;
use crate::session::SessionId;
use std::collections::HashMap;
use std::time::Instant;

pub type ElevationRequestId = u64;

/// Per-session attempt count against elevation's own `AuthPolicy` --
/// structurally identical to `lock::lock::SessionLock`, kept as its
/// own type because it's keyed and reset independently: getting your
/// lock-screen password wrong doesn't burn an elevation attempt, and
/// vice versa (see `docs/security.md`'s Elevation section for why
/// that's a deliberate choice, not an oversight).
#[derive(Debug, Default)]
struct SessionElevationAuth {
    locked_out: bool,
    attempts: u32,
}

struct PendingElevation {
    session_id: SessionId,
    /// The session's registered compositor at the moment this prompt
    /// was opened -- who's allowed to answer it. Captured once rather
    /// than re-read from `SessionManager` on every check, so a
    /// mid-flight compositor swap can't quietly redirect an
    /// already-open prompt to a different connection.
    compositor_conn: ConnId,
    /// Whoever asked for this verification (mitos-service, or an
    /// equivalent trusted caller) -- who eventually gets the deferred
    /// `Response::AuthResult`.
    requester_conn: ConnId,
}

/// What's known about a pending request without needing `&mut self` --
/// enough for a caller to look up the session's user name and decide
/// who else needs telling once it resolves.
#[derive(Debug, Clone, Copy)]
pub struct PendingElevationInfo {
    pub session_id: SessionId,
    pub compositor_conn: ConnId,
}

/// The result of resolving one attempt against a pending request:
/// enough for the caller to drive both IPC connections involved (the
/// compositor showing the prompt, and whoever originally asked for
/// the verification) without reaching back into this manager.
#[derive(Debug, Clone)]
pub struct ElevationOutcome {
    pub session_id: SessionId,
    pub compositor_conn: ConnId,
    pub requester_conn: ConnId,
    pub outcome: AuthOutcome,
    /// `false` only for a plain `Failure` with attempts remaining --
    /// every other outcome closes the pending request out.
    pub terminal: bool,
}

/// A pending request torn down without ever being answered -- its
/// session ended, or a connection it depended on (the compositor that
/// would show it, or the caller waiting on the answer) dropped.
/// Enough for the caller to tell whoever's still reachable that it's
/// over.
#[derive(Debug, Clone, Copy)]
pub struct AbandonedElevation {
    pub request_id: ElevationRequestId,
    pub session_id: SessionId,
    pub compositor_conn: ConnId,
    pub requester_conn: ConnId,
}

/// Owns every in-flight "please verify this session's user" request
/// from mitos-service (or an equivalent trusted caller), plus the
/// per-session attempt/lockout state a brute-force run against it
/// would need to defeat.
///
/// Structurally close to `lock::LockManager`'s attempt tracking, but
/// kept as its own module because the shape of the *flow* is
/// genuinely different: lock/unlock is a two-party conversation (a
/// session's own client, talking to mitos-session) where the same
/// connection that shows the prompt is the one answering it. Elevation
/// is a three-party relay -- mitos-service asks, mitos-gui prompts and
/// answers, mitos-session checks -- so a pending request has to
/// remember *two* connections (`compositor_conn`, `requester_conn`)
/// and reply to them independently. See `docs/security.md`'s
/// Elevation section for the full trust-boundary reasoning.
#[derive(Default)]
pub struct ElevationManager {
    pending: HashMap<ElevationRequestId, PendingElevation>,
    next_id: u64,
    session_auth: HashMap<SessionId, SessionElevationAuth>,
    pub timeouts: ElevationTimeouts,
    /// The uid `RequestElevation` is accepted from, resolved from
    /// `[elevation].service_user` at startup and on config reload
    /// (`Daemon::resolve_elevation_requester` in `src/main.rs`).
    /// `None` if that account doesn't resolve -- fails closed: nothing
    /// but root can open an elevation prompt until it's fixed.
    requester_uid: Option<u32>,
}

impl ElevationManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_requester_uid(&mut self, uid: Option<u32>) {
        self.requester_uid = uid;
    }

    /// Whether `uid` is the configured elevation-requesting service.
    /// Root is handled by the caller (`policy::authorize`), the same
    /// way it's handled for every other request type -- this only
    /// ever answers "is this *specifically* the configured service
    /// account".
    pub fn is_authorized_requester(&self, uid: u32) -> bool {
        self.requester_uid == Some(uid)
    }

    pub fn peek(&self, id: ElevationRequestId) -> Option<PendingElevationInfo> {
        self.pending.get(&id).map(|p| PendingElevationInfo {
            session_id: p.session_id,
            compositor_conn: p.compositor_conn,
        })
    }

    /// Who's allowed to answer `id` right now -- `policy::authorize`
    /// compares this against the connection a `RespondElevation`
    /// actually arrived on. Returns `None` for an unknown or
    /// already-resolved id; deliberately indistinguishable from "wrong
    /// connection" to the caller, so a peer can't probe for which
    /// request ids currently exist.
    pub fn compositor_conn_for(&self, id: ElevationRequestId) -> Option<ConnId> {
        self.pending.get(&id).map(|p| p.compositor_conn)
    }

    /// Open a new pending request for `session_id`, or fail fast
    /// without ever creating one or bothering the compositor:
    /// - `SessionError::LockedOut` if this session is still serving
    ///   out a lockout from previous failed attempts.
    /// - `SessionError::PermissionDenied` if it already has
    ///   `policy.max_pending_per_session` prompts outstanding.
    pub fn begin(
        &mut self,
        session_id: SessionId,
        compositor_conn: ConnId,
        requester_conn: ConnId,
        policy: &ElevationPolicy,
        auth_policy: &AuthPolicy,
        now: Instant,
    ) -> Result<ElevationRequestId> {
        if self
            .session_auth
            .get(&session_id)
            .is_some_and(|a| a.locked_out)
        {
            return Err(SessionError::LockedOut(auth_policy.lockout.as_secs()));
        }

        let pending_for_session = self
            .pending
            .values()
            .filter(|p| p.session_id == session_id)
            .count();
        if pending_for_session >= policy.max_pending_per_session {
            return Err(SessionError::PermissionDenied(format!(
                "session {session_id} already has {pending_for_session} elevation prompt(s) pending"
            )));
        }

        self.next_id += 1;
        let id = self.next_id;
        self.pending.insert(
            id,
            PendingElevation {
                session_id,
                compositor_conn,
                requester_conn,
            },
        );
        self.timeouts.start_prompt(id, now, policy.prompt_timeout);
        Ok(id)
    }

    /// Check a password attempt against pending request `id`. Returns
    /// `None` if `id` is unknown or already resolved. On a plain
    /// `Failure` with attempts remaining the request stays open for
    /// another try (`terminal: false`); every other outcome closes it
    /// out.
    pub fn attempt(
        &mut self,
        id: ElevationRequestId,
        authenticator: &dyn Authenticator,
        request: &AuthRequest,
        auth_policy: &AuthPolicy,
        now: Instant,
    ) -> Option<ElevationOutcome> {
        let pending = self.pending.get(&id)?;
        let (session_id, compositor_conn, requester_conn) =
            (pending.session_id, pending.compositor_conn, pending.requester_conn);

        let entry = self.session_auth.entry(session_id).or_default();
        let outcome = check(authenticator, request, auth_policy, &mut entry.attempts);
        let terminal = match &outcome {
            AuthOutcome::Success => {
                entry.attempts = 0;
                true
            }
            AuthOutcome::LockedOut { .. } => {
                entry.locked_out = true;
                self.timeouts.start_lockout(session_id, now, auth_policy.lockout);
                true
            }
            AuthOutcome::Failure { .. } => false,
            // `check` -- the only thing that produces an outcome on
            // this path -- never actually returns these two (`Error`
            // would mean PAM itself is broken; `Cancelled` only ever
            // comes from `cancel`, below, not from a password
            // attempt), but both still end the pending request rather
            // than leaving it silently open if that ever changes.
            AuthOutcome::Error(_) | AuthOutcome::Cancelled => true,
        };

        if terminal {
            self.pending.remove(&id);
            self.timeouts.cancel_prompt(id);
        }

        Some(ElevationOutcome {
            session_id,
            compositor_conn,
            requester_conn,
            outcome,
            terminal,
        })
    }

    /// Resolve `id` as explicitly cancelled -- the user (or whoever
    /// was answering) declined outright rather than getting the
    /// credential wrong. Never touches attempt/lockout state: a
    /// cancel isn't a guess, so it shouldn't cost one.
    pub fn cancel(&mut self, id: ElevationRequestId) -> Option<ElevationOutcome> {
        let pending = self.pending.remove(&id)?;
        self.timeouts.cancel_prompt(id);
        Some(ElevationOutcome {
            session_id: pending.session_id,
            compositor_conn: pending.compositor_conn,
            requester_conn: pending.requester_conn,
            outcome: AuthOutcome::Cancelled,
            terminal: true,
        })
    }

    /// Mirrors `lock::LockManager::clear_lockout`: called once a
    /// second from `Daemon::on_idle_tick` so a session's elevation
    /// lockout actually ends instead of persisting forever once set.
    pub fn clear_expired_lockouts(&mut self, now: Instant) {
        for session_id in self.timeouts.expired_lockouts(now) {
            if let Some(auth) = self.session_auth.get_mut(&session_id) {
                auth.locked_out = false;
                auth.attempts = 0;
            }
        }
    }

    /// Every pending request whose prompt window has elapsed as of
    /// `now`, removed from tracking as they're returned -- the caller
    /// is responsible for telling both connections involved that it's
    /// over (see `Daemon::notify_elevation_abandoned`).
    pub fn expire_pending(&mut self, now: Instant) -> Vec<AbandonedElevation> {
        let expired_ids = self.timeouts.expired_prompts(now);
        expired_ids
            .into_iter()
            .filter_map(|id| self.take_abandoned(id))
            .collect()
    }

    /// Every pending request belonging to `session_id`, torn down
    /// because the session itself just ended.
    pub fn abandon_session(&mut self, session_id: SessionId) -> Vec<AbandonedElevation> {
        self.abandon_where(|p| p.session_id == session_id)
    }

    /// Every pending request touching `conn_id` -- as the compositor
    /// that would have shown it, or as the caller that asked for it --
    /// torn down because that connection just dropped.
    pub fn abandon_connection(&mut self, conn_id: ConnId) -> Vec<AbandonedElevation> {
        self.abandon_where(|p| p.compositor_conn == conn_id || p.requester_conn == conn_id)
    }

    fn abandon_where(&mut self, mut predicate: impl FnMut(&PendingElevation) -> bool) -> Vec<AbandonedElevation> {
        let ids: Vec<ElevationRequestId> = self
            .pending
            .iter()
            .filter(|(_, p)| predicate(p))
            .map(|(&id, _)| id)
            .collect();
        ids.into_iter().filter_map(|id| self.take_abandoned(id)).collect()
    }

    fn take_abandoned(&mut self, id: ElevationRequestId) -> Option<AbandonedElevation> {
        self.timeouts.cancel_prompt(id);
        self.pending.remove(&id).map(|p| AbandonedElevation {
            request_id: id,
            session_id: p.session_id,
            compositor_conn: p.compositor_conn,
            requester_conn: p.requester_conn,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    struct AlwaysFail;
    impl Authenticator for AlwaysFail {
        fn authenticate(&self, _r: &AuthRequest) -> Result<()> {
            Err(SessionError::AuthFailed("nope".into()))
        }
    }

    struct AlwaysSucceed;
    impl Authenticator for AlwaysSucceed {
        fn authenticate(&self, _r: &AuthRequest) -> Result<()> {
            Ok(())
        }
    }

    fn policy() -> ElevationPolicy {
        ElevationPolicy {
            enabled: true,
            prompt_timeout: Duration::from_secs(120),
            max_pending_per_session: 2,
        }
    }

    fn auth_policy() -> AuthPolicy {
        AuthPolicy {
            pam_service: "test".into(),
            allow_empty_password: false,
            max_attempts: 2,
            lockout: Duration::from_secs(30),
        }
    }

    fn req(session_id: SessionId) -> AuthRequest {
        AuthRequest {
            session_id,
            user_name: "alice".into(),
            password: "hunter2".into(),
        }
    }

    #[test]
    fn only_the_configured_uid_is_an_authorized_requester() {
        let mut mgr = ElevationManager::new();
        assert!(!mgr.is_authorized_requester(1000));
        mgr.set_requester_uid(Some(1000));
        assert!(mgr.is_authorized_requester(1000));
        assert!(!mgr.is_authorized_requester(1001));
    }

    #[test]
    fn full_ask_answer_success_cycle() {
        let mut mgr = ElevationManager::new();
        let now = Instant::now();
        let id = mgr.begin(1, 100, 200, &policy(), &auth_policy(), now).unwrap();

        assert_eq!(mgr.peek(id).unwrap().session_id, 1);
        assert_eq!(mgr.compositor_conn_for(id), Some(100));

        let outcome = mgr
            .attempt(id, &AlwaysSucceed, &req(1), &auth_policy(), now)
            .unwrap();
        assert_eq!(outcome.outcome, AuthOutcome::Success);
        assert!(outcome.terminal);
        assert_eq!(outcome.compositor_conn, 100);
        assert_eq!(outcome.requester_conn, 200);

        // Resolved -- answering again finds nothing.
        assert!(mgr.compositor_conn_for(id).is_none());
    }

    #[test]
    fn a_wrong_attempt_leaves_the_prompt_open_until_locked_out() {
        let mut mgr = ElevationManager::new();
        let now = Instant::now();
        let id = mgr.begin(1, 100, 200, &policy(), &auth_policy(), now).unwrap();

        let first = mgr.attempt(id, &AlwaysFail, &req(1), &auth_policy(), now).unwrap();
        assert!(matches!(first.outcome, AuthOutcome::Failure { .. }));
        assert!(!first.terminal);
        // Still open -- a second attempt against the same id works.
        assert!(mgr.compositor_conn_for(id).is_some());

        let second = mgr.attempt(id, &AlwaysFail, &req(1), &auth_policy(), now).unwrap();
        assert!(matches!(second.outcome, AuthOutcome::LockedOut { .. }));
        assert!(second.terminal);
        assert!(mgr.compositor_conn_for(id).is_none());
    }

    #[test]
    fn a_lockout_blocks_brand_new_requests_for_the_same_session() {
        let mut mgr = ElevationManager::new();
        let now = Instant::now();
        let id = mgr.begin(1, 100, 200, &policy(), &auth_policy(), now).unwrap();
        mgr.attempt(id, &AlwaysFail, &req(1), &auth_policy(), now);
        mgr.attempt(id, &AlwaysFail, &req(1), &auth_policy(), now); // locks out

        let second_request = mgr.begin(1, 100, 201, &policy(), &auth_policy(), now);
        assert!(matches!(second_request, Err(SessionError::LockedOut(_))));

        // A different session is unaffected.
        assert!(mgr.begin(2, 100, 202, &policy(), &auth_policy(), now).is_ok());
    }

    #[test]
    fn lockout_clears_once_its_timer_elapses() {
        let mut mgr = ElevationManager::new();
        let now = Instant::now();
        let id = mgr.begin(1, 100, 200, &policy(), &auth_policy(), now).unwrap();
        mgr.attempt(id, &AlwaysFail, &req(1), &auth_policy(), now);
        mgr.attempt(id, &AlwaysFail, &req(1), &auth_policy(), now);

        let later = now + Duration::from_secs(31);
        mgr.clear_expired_lockouts(later);
        assert!(mgr.begin(1, 100, 203, &policy(), &auth_policy(), later).is_ok());
    }

    #[test]
    fn cancelling_never_touches_the_attempt_counter() {
        let mut mgr = ElevationManager::new();
        let now = Instant::now();
        let id = mgr.begin(1, 100, 200, &policy(), &auth_policy(), now).unwrap();
        mgr.attempt(id, &AlwaysFail, &req(1), &auth_policy(), now);

        let id2 = mgr.begin(1, 100, 201, &policy(), &auth_policy(), now).unwrap();
        let cancelled = mgr.cancel(id2).unwrap();
        assert_eq!(cancelled.outcome, AuthOutcome::Cancelled);

        // The one earlier failure is still all that's on the books --
        // one more wrong guess should fail, not lock out.
        let id3 = mgr.begin(1, 100, 202, &policy(), &auth_policy(), now).unwrap();
        let outcome = mgr.attempt(id3, &AlwaysFail, &req(1), &auth_policy(), now).unwrap();
        assert!(matches!(outcome.outcome, AuthOutcome::Failure { .. }));
    }

    #[test]
    fn max_pending_per_session_is_enforced() {
        let mut mgr = ElevationManager::new();
        let now = Instant::now();
        mgr.begin(1, 100, 200, &policy(), &auth_policy(), now).unwrap();
        mgr.begin(1, 100, 201, &policy(), &auth_policy(), now).unwrap();
        let third = mgr.begin(1, 100, 202, &policy(), &auth_policy(), now);
        assert!(matches!(third, Err(SessionError::PermissionDenied(_))));
    }

    #[test]
    fn an_unanswered_prompt_expires_and_is_reported_once() {
        let mut mgr = ElevationManager::new();
        let now = Instant::now();
        let id = mgr.begin(1, 100, 200, &policy(), &auth_policy(), now).unwrap();

        let later = now + Duration::from_secs(200);
        let abandoned = mgr.expire_pending(later);
        assert_eq!(abandoned.len(), 1);
        assert_eq!(abandoned[0].request_id, id);
        assert_eq!(abandoned[0].compositor_conn, 100);
        assert_eq!(abandoned[0].requester_conn, 200);

        assert!(mgr.expire_pending(later).is_empty());
    }

    #[test]
    fn terminating_a_session_abandons_only_its_own_prompts() {
        let mut mgr = ElevationManager::new();
        let now = Instant::now();
        let a = mgr.begin(1, 100, 200, &policy(), &auth_policy(), now).unwrap();
        let _b = mgr.begin(2, 101, 201, &policy(), &auth_policy(), now).unwrap();

        let abandoned = mgr.abandon_session(1);
        assert_eq!(abandoned.len(), 1);
        assert_eq!(abandoned[0].request_id, a);
        // Session 2's prompt is untouched.
        assert!(mgr.peek(_b).is_some());
    }

    #[test]
    fn a_dropped_connection_abandons_prompts_it_holds_either_role_in() {
        let mut mgr = ElevationManager::new();
        let now = Instant::now();
        // conn 100 is the compositor for one pending request...
        let a = mgr.begin(1, 100, 200, &policy(), &auth_policy(), now).unwrap();
        // ...and the requester for a different one.
        let b = mgr.begin(2, 300, 100, &policy(), &auth_policy(), now).unwrap();

        let abandoned = mgr.abandon_connection(100);
        let ids: Vec<_> = abandoned.iter().map(|a| a.request_id).collect();
        assert!(ids.contains(&a));
        assert!(ids.contains(&b));
    }
}
