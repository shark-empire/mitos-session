//! Secure creation of per-user runtime directories (`XDG_RUNTIME_DIR`).
//!
//! Ownership and permission changes are made through an `O_NOFOLLOW`
//! directory file descriptor rather than by path, so a symlink swapped
//! in at the last moment cannot redirect them.

use crate::errors::{Result, SessionError};
use nix::errno::Errno;
use nix::fcntl::OFlag;
use nix::unistd::{Gid, Uid};
use std::fs;
use std::os::unix::fs::{fchown, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use tracing::{debug, info};

/// Prepare the standard user runtime directory, `/run/user/<uid>`.
///
/// The daemon itself places runtime directories under
/// `[session].runtime_dir` (see [`ensure_user_runtime_dir_in`]); this is
/// the same logic pointed at the conventional location.
pub fn ensure_user_runtime_dir(uid: Uid, gid: Gid) -> Result<PathBuf> {
    ensure_user_runtime_dir_in(Path::new("/run/user"), uid, gid)
}

/// Same as [`ensure_user_runtime_dir`], but with a configurable base.
///
/// `session::lifecycle::begin` reaches this (through
/// `Environment::ensure_runtime_dir`) with `[session].runtime_dir`, and
/// the integration tests exercise it against a temporary directory.
pub fn ensure_user_runtime_dir_in(base: &Path, uid: Uid, gid: Gid) -> Result<PathBuf> {
    ensure_named_runtime_dir(base, &uid.as_raw().to_string(), uid, gid)
}

/// Securely create `<base>/<name>`.
///
/// Requirements enforced:
///
/// - `base` must be a real directory, not a symlink
/// - `<base>/<name>` must be a real directory; a symlink there is refused
/// - ownership is locked to `(uid, gid)`
/// - permissions are locked to `0700`
///
/// The final ownership/permission changes and checks go through an
/// `O_NOFOLLOW` directory file descriptor, not through path-based
/// operations that could race with symlink replacement.
pub fn ensure_named_runtime_dir(base: &Path, name: &str, uid: Uid, gid: Gid) -> Result<PathBuf> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') {
        let message = format!("invalid runtime directory name: {name:?}");
        return Err(SessionError::Config(message));
    }

    prepare_base(base)?;

    let runtime_dir = base.join(name);

    // Best-effort create. If it already exists, it is still validated
    // through an O_NOFOLLOW descriptor below.
    match fs::create_dir(&runtime_dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(io_context("failed to create runtime directory")(e)),
    }

    // Open the directory itself, refusing symlinks.
    let flags = OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC;
    let dir = fs::OpenOptions::new()
        .read(true)
        .custom_flags(flags.bits())
        .open(&runtime_dir)
        .map_err(|e| open_error(&runtime_dir, e))?;

    let meta = dir
        .metadata()
        .map_err(io_context("failed to fstat runtime directory"))?;

    if !meta.is_dir() {
        let message = format!("{} is not a directory", runtime_dir.display());
        return Err(refuse(message));
    }

    // Lock ownership and permissions through the descriptor.
    fchown(&dir, Some(uid.as_raw()), Some(gid.as_raw()))
        .map_err(io_context("failed to fchown runtime directory"))?;
    dir.set_permissions(fs::Permissions::from_mode(0o700))
        .map_err(io_context("failed to fchmod runtime directory"))?;

    // Prove the final state through the same descriptor.
    let meta = dir
        .metadata()
        .map_err(io_context("failed to re-stat runtime directory"))?;

    let owned = meta.uid() == uid.as_raw() && meta.gid() == gid.as_raw();
    if !owned || meta.mode() & 0o777 != 0o700 {
        let message = format!(
            "{} ended up with an unexpected owner or mode",
            runtime_dir.display()
        );
        return Err(refuse(message));
    }

    info!(
        uid = uid.as_raw(),
        gid = gid.as_raw(),
        path = %runtime_dir.display(),
        "prepared secure user runtime directory"
    );

    Ok(runtime_dir)
}

/// Make sure `base` exists as a real (non-symlink) directory that
/// everyone can traverse but nobody but root can write to.
fn prepare_base(base: &Path) -> Result<()> {
    if !base.exists() {
        fs::create_dir_all(base).map_err(io_context("failed to create runtime base directory"))?;
    }

    let meta = fs::symlink_metadata(base)
        .map_err(io_context("failed to inspect runtime base directory"))?;

    if meta.file_type().is_symlink() {
        let message = format!("runtime base {} is a symlink", base.display());
        return Err(refuse(message));
    }

    if !meta.is_dir() {
        let message = format!("runtime base {} is not a directory", base.display());
        return Err(refuse(message));
    }

    fs::set_permissions(base, fs::Permissions::from_mode(0o755))
        .map_err(io_context("failed to set runtime base permissions"))?;

    debug!(path = %base.display(), "runtime base directory prepared");

    Ok(())
}

/// Removes stale IPC sockets and runtime files left behind by an unclean exit.
pub fn scrub_session_artifacts(runtime_dir: &Path) -> Result<()> {
    let mitos_dir = runtime_dir.join("mitos-session");
    if mitos_dir.exists() {
        let _ = fs::remove_dir_all(&mitos_dir);
    }

    // Standard Wayland/X11 sockets that might be left behind.
    let sockets = [
        "wayland-0",
        "wayland-0.lock",
        "X0",
        "X0-lock",
        "mitos-session.sock",
    ];
    for socket in sockets {
        let path = runtime_dir.join(socket);
        if path.exists() {
            let _ = fs::remove_file(path);
        }
    }

    info!(path = %runtime_dir.display(), "scrubbed stale session artifacts");
    Ok(())
}

/// Attach `context` to an I/O error, keeping its `ErrorKind` so callers
/// can still match on it.
fn io_context(context: &'static str) -> impl FnOnce(std::io::Error) -> SessionError {
    move |e| SessionError::Io(std::io::Error::new(e.kind(), format!("{context}: {e}")))
}

/// A refusal to touch something that isn't what it should be: a symlink
/// where a directory belongs, a directory with the wrong owner, ...
fn refuse(message: String) -> SessionError {
    SessionError::PermissionDenied(message)
}

/// Turn a failed `O_NOFOLLOW | O_DIRECTORY` open into the right error:
/// a symlink (or anything else that isn't a plain directory) is
/// refused, every other failure is just an I/O error.
fn open_error(path: &Path, e: std::io::Error) -> SessionError {
    let code = e.raw_os_error();
    if code == Some(Errno::ELOOP as i32) || code == Some(Errno::ENOTDIR as i32) {
        let message = format!("{} is a symlink or not a directory", path.display());
        return refuse(message);
    }
    io_context("failed to open runtime directory securely")(e)
}
