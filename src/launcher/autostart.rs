use std::fs;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};

use crate::errors::{Result, SessionError};
use crate::session::Environment;
use crate::user::User;

#[derive(Debug, Clone)]
pub struct AutostartEntry {
    pub name: String,
    pub exec: String,
    pub hidden: bool,
    pub only_show_in: Vec<String>,
    pub not_show_in: Vec<String>,
}

/// Parses both system-wide and user-specific autostart directories.
pub fn parse_autostart_dirs(user: &User) -> Vec<AutostartEntry> {
    let mut entries = Vec::new();
    
    let system_dir = PathBuf::from("/etc/xdg/autostart");
    if system_dir.exists() {
        entries.extend(parse_dir(&system_dir));
    }
    
    let user_dir = user.home.join(".config").join("autostart");
    if user_dir.exists() {
        entries.extend(parse_dir(&user_dir));
    }
    
    entries
}

fn parse_dir(dir: &Path) -> Vec<AutostartEntry> {
    let mut entries = Vec::new();
    if let Ok(read_dir) = fs::read_dir(dir) {
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("desktop") {
                if let Some(parsed) = parse_desktop_file(&path) {
                    entries.push(parsed);
                }
            }
        }
    }
    entries
}

fn parse_desktop_file(path: &Path) -> Option<AutostartEntry> {
    let content = fs::read_to_string(path).ok()?;
    let mut name = String::new();
    let mut exec = String::new();
    let mut hidden = false;
    let mut only_show_in = Vec::new();
    let mut not_show_in = Vec::new();
    let mut in_desktop_entry = false;

    for line in content.lines() {
        let line = line.trim();
        if line == "[Desktop Entry]" {
            in_desktop_entry = true;
            continue;
        }
        if line.starts_with('[') {
            in_desktop_entry = false;
            continue;
        }
        if !in_desktop_entry || line.is_empty() || line.starts_with('#') { continue; }

        if let Some((k, v)) = line.split_once('=') {
            match k.trim() {
                "Name" => name = v.trim().to_string(),
                "Exec" => exec = v.trim().to_string(),
                "Hidden" => hidden = v.trim().eq_ignore_ascii_case("true"),
                "OnlyShowIn" => only_show_in = v.split(';').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
                "NotShowIn" => not_show_in = v.split(';').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
                _ => {}
            }
        }
    }

    if exec.is_empty() { return None; }
    Some(AutostartEntry { name, exec, hidden, only_show_in, not_show_in })
}

/// Filters and launches autostart apps for a given session.
pub fn launch_autostart_apps(user: &User, env: &Environment) {
    let entries = parse_autostart_dirs(user);
    
    for entry in entries {
        if entry.hidden { continue; }
        
        // XDG Desktop Entry spec filtering
        if !entry.only_show_in.is_empty() && !entry.only_show_in.iter().any(|s| s == "MITOS" || s == "X-MITOS") {
            continue;
        }
        if entry.not_show_in.iter().any(|s| s == "MITOS" || s == "X-MITOS") {
            continue;
        }

        // Strip standard desktop file field codes (%f, %F, %u, %U)
        let cmd_str = entry.exec
            .replace("%f", "").replace("%F", "")
            .replace("%u", "").replace("%U", "")
            .replace("%i", "").replace("%c", "").replace("%k", "");
            
        let mut parts = cmd_str.split_whitespace();
        if let Some(bin) = parts.next() {
            let args: Vec<&str> = parts.collect();
            if let Err(e) = spawn_autostart_app(user, env, bin, &args) {
                tracing::warn!(app = %entry.name, error = %e, "failed to launch autostart app");
            } else {
                tracing::info!(app = %entry.name, "launched autostart app");
            }
        }
    }
}

fn spawn_autostart_app(user: &User, env: &Environment, bin: &str, args: &[&str]) -> Result<std::process::Child> {
    let mut cmd = std::process::Command::new(bin);
    cmd.args(args);
    
    for (k, v) in env.iter() {
        cmd.env(k, v);
    }

    let uid = user.uid.as_raw();
    let gid = user.gid.as_raw();
    let groups: Vec<libc::gid_t> = user.groups.iter().map(|g| g.as_raw()).collect();

    unsafe {
        cmd.pre_exec(move || {
            if libc::setsid() == -1 { return Err(std::io::Error::last_os_error()); }
            if libc::setgroups(groups.len(), groups.as_ptr()) == -1 { return Err(std::io::Error::last_os_error()); }
            if libc::setresgid(gid, gid, gid) == -1 { return Err(std::io::Error::last_os_error()); }
            if libc::setresuid(uid, uid, uid) == -1 { return Err(std::io::Error::last_os_error()); }
            Ok(())
        });
    }

    cmd.spawn().map_err(SessionError::Io)
}
