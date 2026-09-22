use std::fs;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use nix::unistd::{Gid, Uid};
use tracing::{debug, info};

/// Prepare the canonical user runtime directory:
///
/// `/run/user/<uid>`
///
/// This is the production entrypoint used by the daemon.
pub fn ensure_user_runtime_dir(uid: Uid, gid: Gid) -> Result<PathBuf> {
    ensure_user_runtime_dir_in(Path::new("/run/user"), uid, gid)
}

/// Same as [`ensure_user_runtime_dir`], but with a configurable base.
///
/// This exists primarily so integration tests can exercise the exact
/// same logic against a temporary directory instead of `/run/user`.
pub fn ensure_user_runtime_dir_in(base: &Path, uid: Uid, gid: Gid) -> Result<PathBuf> {
    ensure_named_runtime_dir(base, &uid.as_raw().to_string(), uid, gid)
}

/// Securely create:
///
/// `<base>/<name>`
///
/// Requirements enforced:
///
/// - base directory must exist as a real directory, not a symlink
/// - `<base>/<name>` must be a real directory
/// - symlinks at `<base>/<name>` are refused
/// - ownership is locked to `(uid, gid)`
/// - permissions are locked to `0700`
///
/// The final ownership/permission checks are done through an
/// `O_NOFOLLOW` directory file descriptor, not through path-based
/// operations that could race with symlink replacement.
pub fn ensure_named_runtime_dir(
    base: &Path,
    name: &str,
    uid: Uid,
    gid: Gid,
) -> Result<PathBuf> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') {
        bail!("invalid runtime directory name: {name:?}");
    }

    prepare_base(base)?;

    let runtime_dir = base.join(name);

    // Best-effort create. If it already exists, we still validate it
    // through an O_NOFOLLOW descriptor below.
    match fs::create_dir(&runtime_dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => {
            return Err(e).context("failed to create runtime directory");
        }
    }

    // Open the directory itself, refusing symlinks.
    let dir = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&runtime_dir)
        .map_err(|e| {
            if e.raw_os_error() == Some(libc::ELOOP) {
                anyhow::anyhow!(
                    "runtime directory path {} is a symlink; refusing",
                    runtime_dir.display()
                )
            } else {
                anyhow::Error::from(e)
            }
        })
        .context("failed to open runtime directory securely")?;

    let meta = dir
        .metadata()
        .context("failed to fstat runtime directory")?;

    if !meta.is_dir() {
        bail!(
            "runtime path {} exists but is not a directory",
            runtime_dir.display()
        );
    }

    // Lock ownership and permissions through the descriptor.
    unsafe {
        if libc::fchown(dir.as_raw_fd(), uid.as_raw(), gid.as_raw()) != 0 {
            return Err(std::io::Error::last_os_error())
                .context("failed to fchown runtime directory");
        }

        if libc::fchmod(dir.as_raw_fd(), 0o700 as libc::mode_t) != 0 {
            return Err(std::io::Error::last_os_error())
                .context("failed to fchmod runtime directory");
        }
    }

    // Prove final state through the same descriptor.
    let meta = dir
        .metadata()
        .context("failed to re-stat runtime directory after permission fix")?;

    if meta.uid() != uid.as_raw() {
        bail!(
            "runtime directory {} has uid {}, expected {}",
            runtime_dir.display(),
            meta.uid(),
            uid.as_raw()
        );
    }

    if meta.gid() != gid.as_raw() {
        bail!(
            "runtime directory {} has gid {}, expected {}",
            runtime_dir.display(),
            meta.gid(),
            gid.as_raw()
        );
    }

    if meta.mode() & 0o777 != 0o700 {
        bail!(
            "runtime directory {} has mode {:#o}, expected 0700",
            runtime_dir.display(),
            meta.mode() & 0o777
        );
    }

    info!(
        uid = uid.as_raw(),
        gid = gid.as_raw(),
        path = %runtime_dir.display(),
        "prepared secure user runtime directory"
    );

    Ok(runtime_dir)
}

fn prepare_base(base: &Path) -> Result<()> {
    if !base.exists() {
        fs::create_dir_all(base).context("failed to create runtime base directory")?;
    }

    let meta = fs::symlink_metadata(base).context("failed to inspect runtime base directory")?;

    if meta.file_type().is_symlink() {
        bail!(
            "runtime base {} is a symlink; refusing",
            base.display()
        );
    }

    if !meta.is_dir() {
        bail!(
            "runtime base {} is not a directory",
            base.display()
        );
    }

    // The base should be traversable but not writable by normal users.
    fs::set_permissions(base, fs::Permissions::from_mode(0o755))
        .context("failed to set permissions on runtime base directory")?;

    debug!(path = %base.display(), "runtime base directory prepared");

    Ok(())
}
