//! `mitos-sessionctl`: a thin CLI over mitos-session's IPC socket.
//! Shares `Request`/`Response` and friends with the daemon via the
//! `mitos_session` library crate (see `src/lib.rs`) instead of
//! re-declaring the protocol here.

use clap::{Parser, Subcommand};
use mitos_session::config;
use mitos_session::errors::Result;
use mitos_session::ipc::{IpcClient, Request, Response};
use mitos_session::lock::{InhibitMode, InhibitWhat};
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "mitos-sessionctl", about = "Inspect and control mitos-session")]
struct Cli {
    /// Override the IPC socket path instead of reading it from
    /// session.toml.
    #[arg(long, global = true)]
    socket: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List every active session.
    ListSessions,
    /// Show one session's status.
    Status { session_id: u32 },
    /// Start a new session for a user.
    CreateSession {
        user_name: String,
        #[arg(long)]
        seat: Option<String>,
        #[arg(long, value_name = "wayland|x11|tty")]
        session_type: Option<String>,
    },
    /// End a session outright (logout).
    Terminate { session_id: u32 },
    /// Lock a session's screen.
    Lock { session_id: u32 },
    /// Attempt to unlock a session. Prompts for the password on stdin
    /// in plain sight -- this CLI is for scripting/testing, a real
    /// unlock attempt comes from mitos-gui's lock screen talking to
    /// the daemon directly, never through this tool.
    Unlock { session_id: u32, user_name: String },
    /// Report input activity on a seat (mostly useful for testing idle
    /// timeouts without waiting for them).
    Activity { seat_id: String },
    /// Switch the active session on a seat.
    Switch { seat_id: String, session_id: u32 },
    /// Take out an inhibitor lock.
    Inhibit {
        what: InhibitWhatArg,
        who: String,
        why: String,
        #[arg(long, default_value = "block")]
        mode: InhibitModeArg,
    },
    /// Release a previously taken inhibitor.
    ReleaseInhibit { inhibit_id: u64 },
    /// List every active inhibitor.
    ListInhibitors,
    /// Suspend the machine.
    Suspend,
    /// Reboot the machine.
    Reboot,
    /// Power the machine off.
    Poweroff,
}

#[derive(Clone, clap::ValueEnum)]
enum InhibitWhatArg {
    Idle,
    Lock,
    Suspend,
    Shutdown,
}

impl From<InhibitWhatArg> for InhibitWhat {
    fn from(v: InhibitWhatArg) -> Self {
        match v {
            InhibitWhatArg::Idle => InhibitWhat::Idle,
            InhibitWhatArg::Lock => InhibitWhat::Lock,
            InhibitWhatArg::Suspend => InhibitWhat::Suspend,
            InhibitWhatArg::Shutdown => InhibitWhat::Shutdown,
        }
    }
}

#[derive(Clone, clap::ValueEnum)]
enum InhibitModeArg {
    Block,
    Delay,
}

impl From<InhibitModeArg> for InhibitMode {
    fn from(v: InhibitModeArg) -> Self {
        match v {
            InhibitModeArg::Block => InhibitMode::Block,
            InhibitModeArg::Delay => InhibitMode::Delay,
        }
    }
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("mitos-sessionctl: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    let socket_path = match cli.socket {
        Some(path) => path,
        None => config::load(None)?.ipc.socket_path,
    };
    let mut client = IpcClient::connect(&socket_path)?;

    let request = match cli.command {
        Command::ListSessions => Request::ListSessions,
        Command::Status { session_id } => Request::SessionStatus { session_id },
        Command::CreateSession {
            user_name,
            seat,
            session_type,
        } => Request::CreateSession {
            user_name,
            seat_id: seat,
            session_type,
        },
        Command::Terminate { session_id } => Request::TerminateSession { session_id },
        Command::Lock { session_id } => Request::LockSession { session_id },
        Command::Unlock {
            session_id,
            user_name,
        } => {
            let password = read_password()?;
            Request::Unlock {
                session_id,
                user_name,
                password,
            }
        }
        Command::Activity { seat_id } => Request::ReportActivity { seat_id },
        Command::Switch {
            seat_id,
            session_id,
        } => Request::SwitchSession {
            seat_id,
            session_id,
        },
        Command::Inhibit {
            what,
            who,
            why,
            mode,
        } => Request::Inhibit {
            what: what.into(),
            who,
            why,
            mode: mode.into(),
        },
        Command::ReleaseInhibit { inhibit_id } => Request::ReleaseInhibit { inhibit_id },
        Command::ListInhibitors => Request::ListInhibitors,
        Command::Suspend => Request::Suspend,
        Command::Reboot => Request::Reboot,
        Command::Poweroff => Request::PowerOff,
    };

    print_response(client.call(request)?);
    Ok(())
}

fn read_password() -> Result<String> {
    print!("Password: ");
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    Ok(line.trim_end_matches(['\n', '\r']).to_string())
}

fn print_response(response: Response) {
    match response {
        Response::Ok => println!("ok"),
        Response::Sessions(sessions) => {
            if sessions.is_empty() {
                println!("no active sessions");
            }
            for s in sessions {
                println!(
                    "{}\t{}\tseat={}\t{:?}\t{}{}",
                    s.id,
                    s.user_name,
                    s.seat_id,
                    s.session_type,
                    s.state,
                    if s.locked { " [locked]" } else { "" }
                );
            }
        }
        Response::Session(s) => {
            println!("id:      {}", s.id);
            println!("user:    {} (uid {})", s.user_name, s.uid);
            println!("seat:    {}", s.seat_id);
            println!("type:    {:?}", s.session_type);
            println!(
                "state:   {}{}",
                s.state,
                if s.locked { " (locked)" } else { "" }
            );
        }
        Response::AuthResult(outcome) => println!("{outcome:?}"),
        Response::InhibitGranted { inhibit_id } => println!("inhibitor {inhibit_id} granted"),
        Response::Inhibitors(list) => {
            if list.is_empty() {
                println!("no active inhibitors");
            }
            for i in list {
                println!("{}\t{:?}\t{}\t{:?}\t{}", i.id, i.what, i.who, i.mode, i.why);
            }
        }
        Response::Error(e) => eprintln!("error: {e}"),
    }
}
