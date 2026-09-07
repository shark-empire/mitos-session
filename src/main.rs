//! mitos-session: the session, seat, lock, idle, authentication, and
//! power manager for MITOS. See README.md for the overall architecture
//! and docs/ for the protocol/security/lifecycle details.
//!
//! Everything in this file runs on one thread. IPC connections get
//! their own I/O threads (see `ipc::server`), but no daemon *state* is
//! ever touched off this thread -- see README's "one thread, one
//! brain" design note for why.

use mitos_session::{
    authentication, config, errors, idle, ipc, launcher, lock, logging, policy, power, seat,
    session, signals, user,
};

use errors::Result;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn main() {
    if let Err(e) = run() {
        eprintln!("mitos-session: fatal: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let config_path = std::env::args().nth(1).map(PathBuf::from);
    let settings = config::load(config_path.as_deref())?;

    logging::init(&settings.logging);
    logging::configure_audit_log(&settings.logging);
    tracing::info!("mitos-session starting");

    let default_seat = settings.seat.default_seat.clone();
    let mut daemon = Daemon::new(settings);
    daemon.seats.ensure_seat(&default_seat);
    match seat::enumerate() {
        Ok(devices) => {
            tracing::info!(count = devices.len(), seat = %default_seat, "enumerated devices via udev");
            daemon.seats.set_devices(&default_seat, devices);
        }
        // Not fatal -- a seat with no known devices yet is still a
        // usable seat; mitos-gui/libinput do their own device access
        // independently of this bookkeeping (see docs/security.md).
        Err(e) => {
            tracing::warn!(error = %e, "udev device enumeration failed, continuing with an empty device list")
        }
    }

    let mut event_loop: calloop::EventLoop<Daemon> = calloop::EventLoop::try_new()
        .map_err(|e| errors::SessionError::Protocol(format!("failed to create event loop: {e}")))?;
    let handle = event_loop.handle();

    // IPC: connection threads feed parsed requests into this channel;
    // the receiving half runs on this thread via calloop.
    let (cmd_tx, cmd_channel) = calloop::channel::channel::<ipc::ManagerMessage>();
    ipc::spawn(&daemon.socket_path, daemon.settings.ipc.socket_mode, cmd_tx)?;
    handle
        .insert_source(cmd_channel, |event, _, daemon: &mut Daemon| {
            if let calloop::channel::Event::Msg(msg) = event {
                daemon.handle_ipc(msg);
            }
        })
        .map_err(|e| {
            errors::SessionError::Protocol(format!("failed to register IPC channel: {e}"))
        })?;

    // Idle tick: 1Hz is plenty for dim/lock/suspend thresholds measured
    // in tens of seconds to minutes, and cheap enough not to bother
    // with a per-seat timer tree.
    let idle_timer = calloop::timer::Timer::from_duration(Duration::from_secs(1));
    handle
        .insert_source(idle_timer, |_deadline, _, daemon: &mut Daemon| {
            daemon.on_idle_tick();
            calloop::timer::TimeoutAction::ToDuration(Duration::from_secs(1))
        })
        .map_err(|e| {
            errors::SessionError::Protocol(format!("failed to register idle timer: {e}"))
        })?;

    let signal_source = calloop::signals::Signals::new(signals::WATCHED)
        .map_err(|e| errors::SessionError::Io(std::io::Error::from(e)))?;

    handle
        .insert_source(signal_source, |event, _, daemon: &mut Daemon| {
            if let Some(session_event) = signals::classify(event.signal()) {
                daemon.on_signal(session_event);
            }
        })
        .map_err(|e| {
            errors::SessionError::Protocol(format!("failed to register signal source: {e}"))
        })?;

    while !daemon.should_exit {
        event_loop
            .dispatch(Some(Duration::from_millis(500)), &mut daemon)
            .map_err(|e| errors::SessionError::Protocol(format!("event loop error: {e}")))?;
    }

    tracing::info!("mitos-session exited cleanly");
    Ok(())
}

/// Everything the daemon owns. Every field is touched only from
/// calloop callbacks running on the single thread `run()` drives.
struct Daemon {
    settings: config::Settings,
    sessions: session::SessionManager,
    seats: seat::SeatManager,
    locks: lock::LockManager,
    idle: idle::IdleDetector,
    registry: ipc::ConnRegistry,
    authenticator: Box<dyn authentication::Authenticator>,
    socket_path: PathBuf,
    should_exit: bool,
}

impl Daemon {
    fn new(settings: config::Settings) -> Self {
        let auth_policy = policy::auth_policy(&settings.authentication, &settings.lock);
        let authenticator = Box::new(authentication::PamAuthenticator::new(&auth_policy));
        let socket_path = settings.ipc.socket_path.clone();
        Self {
            settings,
            sessions: session::SessionManager::new(),
            seats: seat::SeatManager::new(),
            locks: lock::LockManager::new(),
            idle: idle::IdleDetector::new(),
            registry: ipc::ConnRegistry::new(),
            authenticator,
            socket_path,
            should_exit: false,
        }
    }

    fn handle_ipc(&mut self, msg: ipc::ManagerMessage) {
        match msg {
            ipc::ManagerMessage::Connected {
                conn_id, outbox, ..
            } => {
                self.registry.register(conn_id, outbox);
            }
            ipc::ManagerMessage::Disconnected { conn_id } => {
                self.registry.unregister(conn_id);
                // A connection can drop without the process behind it
                // exiting (or vice versa: SIGCHLD can arrive before or
                // after this). Actual crash-restart-with-backoff is
                // driven by `handle_compositor_exit` off `SIGCHLD`,
                // which is the only signal that reliably means the
                // process itself is gone -- this branch just clears
                // the now-stale connection handle so events don't get
                // sent into the void.
                for ctx in self.sessions.iter_mut() {
                    if ctx.compositor_conn == Some(conn_id) {
                        ctx.compositor_conn = None;
                        tracing::warn!(
                            session_id = ctx.id(),
                            "session's compositor connection dropped"
                        );
                    }
                }
            }
            ipc::ManagerMessage::Command(cmd) => self.handle_command(cmd),
        }
    }

    fn handle_command(&mut self, cmd: ipc::ManagerCommand) {
        let ipc::ManagerCommand {
            conn_id,
            peer,
            request,
        } = cmd;

        if let Err(e) = policy::authorize(&peer, &request, &self.sessions) {
            self.registry
                .send_response(conn_id, ipc::Response::Error(e.to_string()));
            return;
        }

        let response = self.dispatch(conn_id, peer, request);
        self.registry.send_response(conn_id, response);
    }

    fn dispatch(
        &mut self,
        conn_id: ipc::ConnId,
        peer: ipc::PeerCred,
        request: ipc::Request,
    ) -> ipc::Response {
        use ipc::{Event, Request, Response};

        match request {
            Request::CreateSession {
                user_name,
                seat_id,
                session_type,
            } => self.create_session(&peer, &user_name, seat_id, session_type),
            Request::TerminateSession { session_id } => {
                match self.sessions.terminate_session(session_id) {
                    Ok(()) => {
                        self.seats.detach_session(session_id);
                        logging::audit_log(
                            logging::AuditEvent::new(peer.uid, "terminate_session", "ok")
                                .target(session_id.to_string()),
                        );
                        Response::Ok
                    }
                    Err(e) => Response::Error(e.to_string()),
                }
            }
            Request::RegisterCompositor { session_id } => match self.sessions.get_mut(session_id) {
                Ok(ctx) => {
                    ctx.compositor_conn = Some(conn_id);
                    // First confirmation that the session's display is
                    // actually up -- this is what moves it out of
                    // `Starting`. `LockManager` is the source of truth
                    // for whether it's locked, independent of whatever
                    // state a compositor crash briefly left it in: if
                    // this is a fresh compositor instance coming back
                    // after one, and the session was locked, it stays
                    // locked and gets told to show the lock screen
                    // again rather than silently coming up unlocked.
                    if self.locks.is_locked(session_id) {
                        let _ = ctx.transition(session::SessionState::Locked);
                        self.registry.send_event(
                            conn_id,
                            Event::ShowLockScreen {
                                session_id,
                                reason: lock::LockReason::Manual,
                            },
                        );
                    } else {
                        let _ = ctx.transition(session::SessionState::Active);
                    }
                    Response::Ok
                }
                Err(e) => Response::Error(e.to_string()),
            },
            Request::ListSessions => {
                Response::Sessions(self.sessions.list().map(session_info).collect())
            }
            Request::SessionStatus { session_id } => match self.sessions.get(session_id) {
                Ok(ctx) => Response::Session(session_info(ctx)),
                Err(e) => Response::Error(e.to_string()),
            },
            Request::LockSession { session_id } => {
                self.lock_session(&peer, session_id, lock::LockReason::Manual)
            }
            Request::Unlock {
                session_id,
                user_name,
                password,
            } => self.unlock(&peer, session_id, user_name, password),
            Request::ReportActivity { seat_id } => {
                self.idle.record_activity(&seat_id, Instant::now());
                Response::Ok
            }
            Request::SwitchSession {
                seat_id,
                session_id,
            } => match self.seats.switch_active(&seat_id, session_id) {
                Ok(_previous) => {
                    if let Ok(ctx) = self.sessions.get(session_id) {
                        if let Some(c) = ctx.compositor_conn {
                            self.registry.send_event(
                                c,
                                Event::SessionActivated {
                                    seat_id,
                                    session_id,
                                },
                            );
                        }
                    }
                    Response::Ok
                }
                Err(e) => Response::Error(e.to_string()),
            },
            Request::Inhibit {
                what,
                who,
                why,
                mode,
            } => {
                let id = self.locks.add_inhibitor(what, who, why, mode);
                Response::InhibitGranted { inhibit_id: id }
            }
            Request::ReleaseInhibit { inhibit_id } => {
                match self.locks.release_inhibitor(inhibit_id) {
                    Ok(()) => Response::Ok,
                    Err(e) => Response::Error(e.to_string()),
                }
            }
            Request::ListInhibitors => Response::Inhibitors(
                self.locks
                    .inhibitors
                    .list()
                    .map(|i| ipc::InhibitorInfo {
                        id: i.id,
                        what: i.what,
                        who: i.who.clone(),
                        why: i.why.clone(),
                        mode: i.mode,
                    })
                    .collect(),
            ),
            Request::Suspend => {
                let lock_policy = lock::LockPolicy::from(&self.settings.lock);
                let grace = Duration::from_secs(self.settings.power.suspend_inhibit_grace_secs);
                match power::suspend(
                    &self.sessions,
                    &mut self.locks,
                    &lock_policy,
                    &self.registry,
                    grace,
                ) {
                    Ok(()) => Response::Ok,
                    Err(e) => Response::Error(e.to_string()),
                }
            }
            Request::Reboot => {
                let grace = Duration::from_secs(self.settings.power.suspend_inhibit_grace_secs);
                match power::reboot(&mut self.sessions, &self.locks, grace) {
                    Ok(()) => Response::Ok,
                    Err(e) => Response::Error(e.to_string()),
                }
            }
            Request::PowerOff => {
                let grace = Duration::from_secs(self.settings.power.suspend_inhibit_grace_secs);
                match power::poweroff(&mut self.sessions, &self.locks, grace) {
                    Ok(()) => Response::Ok,
                    Err(e) => Response::Error(e.to_string()),
                }
            }
        }
    }

    fn create_session(
        &mut self,
        peer: &ipc::PeerCred,
        user_name: &str,
        seat_id: Option<String>,
        session_type: Option<String>,
    ) -> ipc::Response {
        let user = match user::User::by_name(user_name) {
            Ok(u) => u,
            Err(e) => return ipc::Response::Error(e.to_string()),
        };

        let seat_id = seat_id.unwrap_or_else(|| self.settings.seat.default_seat.clone());
        let session_type = match session_type.as_deref() {
            Some("x11") => session::SessionType::X11,
            Some("tty") => session::SessionType::Tty,
            Some(_) | None => {
                policy::SessionPolicy::from(&self.settings.session).default_session_type
            }
        };

        match self
            .sessions
            .create_session(user, &seat_id, session_type, &self.settings.session)
        {
            Ok(id) => {
                self.seats.attach_session(&seat_id, id);
                logging::audit_log(
                    logging::AuditEvent::new(peer.uid, "create_session", "ok")
                        .target(user_name.to_string()),
                );

                // A tty session is its own "compositor" (a shell);
                // everything else gets mitos-gui. The session stays
                // valid even if this fails to spawn -- `RegisterCompositor`
                // is what actually confirms a display came up, so a
                // launch failure here just leaves it in `Starting`
                // rather than tearing the whole session down.
                if session_type != session::SessionType::Tty {
                    self.try_spawn_compositor(id);
                }

                match self.sessions.get(id) {
                    Ok(ctx) => ipc::Response::Session(session_info(ctx)),
                    Err(e) => ipc::Response::Error(e.to_string()),
                }
            }
            Err(e) => {
                logging::audit_log(
                    logging::AuditEvent::new(peer.uid, "create_session", e.to_string())
                        .target(user_name.to_string()),
                );
                ipc::Response::Error(e.to_string())
            }
        }
    }

    /// Spawn mitos-gui for a session and record the child, or just log
    /// a warning and leave it in `Starting` on failure. Shared by
    /// `create_session` (first launch) and `handle_compositor_exit`
    /// (relaunch after a crash) so the two can't drift apart.
    fn try_spawn_compositor(&mut self, id: session::SessionId) {
        if let Ok(ctx) = self.sessions.get_mut(id) {
            let binary = self.settings.session.compositor_binary.clone();
            match launcher::spawn_compositor(&ctx.user, &ctx.environment, &binary) {
                Ok(child) => ctx.compositor_process = Some(child),
                Err(e) => tracing::warn!(session_id = id, error = %e, "failed to spawn compositor"),
            }
        }
    }

    fn lock_session(
        &mut self,
        peer: &ipc::PeerCred,
        session_id: session::SessionId,
        reason: lock::LockReason,
    ) -> ipc::Response {
        let lock_policy = lock::LockPolicy::from(&self.settings.lock);
        match self.locks.lock(session_id, &lock_policy) {
            Ok(()) => {
                if let Ok(ctx) = self.sessions.get_mut(session_id) {
                    let _ = ctx.transition(session::SessionState::Locked);
                    if let Some(c) = ctx.compositor_conn {
                        self.registry
                            .send_event(c, ipc::Event::ShowLockScreen { session_id, reason });
                    }
                }
                logging::audit_log(
                    logging::AuditEvent::new(peer.uid, "lock_session", "ok")
                        .target(session_id.to_string()),
                );
                ipc::Response::Ok
            }
            Err(e) => ipc::Response::Error(e.to_string()),
        }
    }

    fn unlock(
        &mut self,
        peer: &ipc::PeerCred,
        session_id: session::SessionId,
        user_name: String,
        password: String,
    ) -> ipc::Response {
        let auth_policy = policy::auth_policy(&self.settings.authentication, &self.settings.lock);
        let request = authentication::AuthRequest {
            session_id,
            user_name: user_name.clone(),
            password,
        };
        let outcome = self.locks.attempt_unlock(
            self.authenticator.as_ref(),
            &request,
            &auth_policy,
            Instant::now(),
        );

        match &outcome {
            authentication::AuthOutcome::Success => {
                if let Ok(ctx) = self.sessions.get_mut(session_id) {
                    let _ = ctx.transition(session::SessionState::Active);
                    if let Some(c) = ctx.compositor_conn {
                        self.registry
                            .send_event(c, ipc::Event::HideLockScreen { session_id });
                    }
                }
            }
            _ => {
                if let Ok(ctx) = self.sessions.get(session_id) {
                    if let Some(c) = ctx.compositor_conn {
                        self.registry.send_event(
                            c,
                            ipc::Event::AuthFeedback {
                                session_id,
                                outcome: outcome.clone(),
                            },
                        );
                    }
                }
            }
        }

        logging::audit_log(
            logging::AuditEvent::new(peer.uid, "unlock_attempt", format!("{outcome:?}"))
                .target(user_name),
        );
        ipc::Response::AuthResult(outcome)
    }

    fn on_idle_tick(&mut self) {
        let now = Instant::now();
        let idle_policy = idle::IdlePolicy::from(&self.settings.idle);
        let lock_policy = lock::LockPolicy::from(&self.settings.lock);

        for (seat_id, stage) in self.idle.tick(now, &idle_policy) {
            match stage {
                idle::IdleStage::Active => {}
                idle::IdleStage::Dimmed => self.notify_active_compositor(
                    &seat_id,
                    ipc::Event::Dim {
                        seat_id: seat_id.clone(),
                    },
                ),
                idle::IdleStage::LockRequested => {
                    if !lock_policy.lock_on_idle {
                        continue;
                    }
                    if let Ok(Some(session_id)) = self.seats.active_session(&seat_id) {
                        if self.locks.lock(session_id, &lock_policy).is_ok() {
                            if let Ok(ctx) = self.sessions.get_mut(session_id) {
                                let _ = ctx.transition(session::SessionState::Locked);
                                if let Some(c) = ctx.compositor_conn {
                                    self.registry.send_event(
                                        c,
                                        ipc::Event::ShowLockScreen {
                                            session_id,
                                            reason: lock::LockReason::Idle,
                                        },
                                    );
                                }
                            }
                        }
                    }
                }
                idle::IdleStage::SuspendRequested => {
                    let grace = Duration::from_secs(self.settings.power.suspend_inhibit_grace_secs);
                    if let Err(e) = power::suspend(
                        &self.sessions,
                        &mut self.locks,
                        &lock_policy,
                        &self.registry,
                        grace,
                    ) {
                        tracing::warn!(error = %e, "idle-triggered suspend did not proceed");
                    }
                }
            }
        }

        for session_id in self.locks.timeouts.expired_lockouts(now) {
            self.locks.clear_lockout(session_id);
        }
    }

    fn notify_active_compositor(&self, seat_id: &str, event: ipc::Event) {
        if let Ok(Some(session_id)) = self.seats.active_session(seat_id) {
            if let Ok(ctx) = self.sessions.get(session_id) {
                if let Some(c) = ctx.compositor_conn {
                    self.registry.send_event(c, event);
                }
            }
        }
    }

    fn on_signal(&mut self, event: signals::SignalEvent) {
        match event {
            signals::SignalEvent::Terminate => {
                tracing::info!("received termination signal, shutting down");
                if let Err(e) = signals::graceful_shutdown(&mut self.sessions, &self.socket_path) {
                    tracing::error!(error = %e, "error during graceful shutdown");
                }
                self.should_exit = true;
            }
            signals::SignalEvent::ReloadConfig => match config::load(None) {
                Ok(new_settings) => {
                    let auth_policy =
                        policy::auth_policy(&new_settings.authentication, &new_settings.lock);
                    self.authenticator =
                        Box::new(authentication::PamAuthenticator::new(&auth_policy));
                    logging::configure_audit_log(&new_settings.logging);
                    self.settings = new_settings;
                    tracing::info!("configuration reloaded");
                }
                Err(e) => {
                    tracing::error!(error = %e, "failed to reload configuration, keeping previous settings")
                }
            },
            signals::SignalEvent::ReapChildren => self.reap_children(),
        }
    }

    /// Reap every exited child without blocking, and for each one,
    /// check whether it was a session's compositor -- if so, that's
    /// what actually drives crash-restart-with-backoff
    /// (`handle_compositor_exit`). Autostart applications also exit
    /// through here, but nothing currently supervises those beyond
    /// reaping them so they don't linger as zombies.
    fn reap_children(&mut self) {
        use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
        use nix::unistd::Pid;

        loop {
            let (pid, status) = match waitpid(Pid::from_raw(-1), Some(WaitPidFlag::WNOHANG)) {
                Ok(WaitStatus::StillAlive) => break,
                Err(_) => break, // ECHILD: no children left
                Ok(status) => match status.pid() {
                    Some(pid) => (pid, status),
                    None => continue, // a status with no pid isn't one we can correlate to a session
                },
            };
            tracing::debug!(?status, "reaped child process");
            self.handle_compositor_exit(pid);
        }
    }

    /// If `pid` was a session's compositor, clear it out and either
    /// relaunch it or fall back to a terminal, per
    /// `launcher::decide_restart`. If `pid` belongs to something else
    /// (an autostart app, most likely), this is a no-op -- `reap_children`
    /// already did the only thing that needed doing for it.
    fn handle_compositor_exit(&mut self, pid: nix::unistd::Pid) {
        let raw_pid = pid.as_raw() as u32;

        let found = self
            .sessions
            .iter_mut()
            .find(|ctx| {
                ctx.compositor_process.as_ref().map(std::process::Child::id) == Some(raw_pid)
            })
            .map(|ctx| {
                ctx.compositor_process = None;
                let stale_conn = ctx.compositor_conn.take();
                (
                    ctx.id(),
                    ctx.session.user_name.clone(),
                    ctx.compositor_restarts,
                    stale_conn,
                )
            });

        let Some((session_id, user_name, restarts, stale_conn)) = found else {
            return;
        };

        if let Some(conn) = stale_conn {
            self.registry.unregister(conn);
        }
        if let Ok(ctx) = self.sessions.get_mut(session_id) {
            // Best-effort: if the session is already on its way out
            // (`Closing`/`Closed`) this transition is simply invalid
            // and ignored -- there's nothing to relaunch for a session
            // that's being torn down anyway.
            let _ = ctx.transition(session::SessionState::Starting);
        }

        match launcher::decide_restart(restarts, self.settings.session.max_compositor_restarts) {
            launcher::RestartDecision::Restart => {
                tracing::warn!(session_id, %user_name, restarts, "compositor exited unexpectedly, restarting it");
                if let Ok(ctx) = self.sessions.get_mut(session_id) {
                    ctx.compositor_restarts += 1;
                }
                self.try_spawn_compositor(session_id);
            }
            launcher::RestartDecision::FallbackToTerminal => {
                tracing::error!(session_id, %user_name, restarts, "compositor kept crashing, falling back to a terminal");
                let fallback = self
                    .sessions
                    .get(session_id)
                    .map(|ctx| launcher::spawn_fallback_terminal(&ctx.user, &ctx.environment));
                match fallback {
                    Ok(Ok(child)) => {
                        if let Ok(ctx) = self.sessions.get_mut(session_id) {
                            ctx.compositor_process = Some(child);
                        }
                    }
                    Ok(Err(e)) => {
                        tracing::error!(session_id, error = %e, "failed to spawn fallback terminal too")
                    }
                    Err(e) => {
                        tracing::error!(session_id, error = %e, "session vanished before a fallback terminal could be spawned")
                    }
                }
            }
        }
    }
}

fn session_info(ctx: &session::SessionContext) -> ipc::SessionInfo {
    ipc::SessionInfo {
        id: ctx.session.id,
        uid: ctx.session.uid.as_raw(),
        user_name: ctx.session.user_name.clone(),
        seat_id: ctx.session.seat_id.clone(),
        session_type: ctx.session.session_type,
        state: format!("{:?}", ctx.state),
        locked: ctx.state.is_locked(),
        created_at: ctx.session.created_at,
    }
}
