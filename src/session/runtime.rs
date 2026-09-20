use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use nix::sys::stat::Mode;
use nix::unistd::{chown, Gid, Uid};
use tracing::{debug, info};

pub fn ensure_user_runtime_dir(uid: Uid, gid: Gid) -> Result<PathBuf> {
    let base = Path::new("/run/user");

    if !base.exists() {
        fs::create_dir_all(base)
            .context("failed to create /run/user")?;

        fs::set_permissions(base, fs::Permissions::from_mode(0o755))
            .context("failed to set permissions on /run/user")?;
    }

    let runtime_dir = base.join(uid.as_raw().to_string());

    match fs::symlink_metadata(&runtime_dir) {
        Ok(meta) if meta.is_dir() => {
            debug!(
                path = %runtime_dir.display(),
                "runtime directory already exists"
            );
        }
        Ok(_) => {
            // Something exists but is not a directory.
            fs::remove_file(&runtime_dir)
                .context("failed to remove non-directory runtime path")?;

            fs::create_dir_all(&runtime_dir)
                .context("failed to create runtime directory")?;
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(&runtime_dir)
                .context("failed to create runtime directory")?;
        }
        Err(err) => {
            return Err(err)
                .context("failed to inspect runtime directory");
        }
    }

    chown(&runtime_dir, Some(uid), Some(gid))
        .context("failed to chown runtime directory")?;

    fs::set_permissions(&runtime_dir, fs::Permissions::from_mode(0o700))
        .context("failed to set runtime directory permissions")?;

    info!(
        uid = uid.as_raw(),
        gid = gid.as_raw(),
        path = %runtime_dir.display(),
        "prepared user runtime directory"
    );

    Ok(runtime_dir)
}
