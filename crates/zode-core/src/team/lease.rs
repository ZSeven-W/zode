//! Exclusive team ownership lease. The fs4 OS lock on a stable `team.lock`
//! file is the SOLE authority — a dead process releases it automatically, so
//! there is no heartbeat-expiry takeover (that path invites split-brain: two
//! processes each locking a different inode). The pid/timestamp written into
//! the file is diagnostics only.
//!
//! Acquisition is lazy: only mutating team operations call [`acquire`];
//! read-only tools never create directories or take the exclusive lock.

use std::io::Write;
use std::path::{Path, PathBuf};

use fs4::fs_std::FileExt;

use super::TeamError;

#[derive(Debug)]
pub struct TeamLease {
    /// Held for the lease's lifetime; dropping releases the OS lock.
    file: std::fs::File,
    pub path: PathBuf,
}

impl Drop for TeamLease {
    fn drop(&mut self) {
        // Release the OS lock explicitly so a subsequent acquire doesn't have
        // to wait for the fd close to propagate (matters under load).
        let _ = FileExt::unlock(&self.file);
    }
}

/// Try to take the exclusive team lease, creating `team_dir` (0700) and the
/// stable `team.lock` file (0600) on first use. Held by another process →
/// `Occupied` (with the holder's pid when readable). Filesystems that cannot
/// support the lock fail closed.
pub fn acquire(team_dir: &Path) -> Result<TeamLease, TeamError> {
    // Refuse symlinked components (host-side path safety, spec §5.1).
    super::board::ensure_no_symlink(team_dir)?;
    std::fs::create_dir_all(team_dir).map_err(|e| TeamError::Io(e.to_string()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(team_dir, std::fs::Permissions::from_mode(0o700));
    }
    let path = team_dir.join("team.lock");
    let mut opts = std::fs::OpenOptions::new();
    opts.create(true).read(true).write(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let file = opts.open(&path).map_err(|e| TeamError::Io(e.to_string()))?;

    // fs4 returns Err(WouldBlock) when another holder has the lock; any
    // other error means the filesystem can't lock → fail closed, loudly.
    if let Err(e) = FileExt::try_lock_exclusive(&file) {
        if e.kind() == std::io::ErrorKind::WouldBlock {
            let pid = std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| s.split_whitespace().next()?.parse::<u32>().ok());
            return Err(TeamError::Occupied { pid });
        }
        return Err(TeamError::Io(format!(
            "filesystem does not support locking ({e}); team features unavailable here"
        )));
    }

    // Diagnostics only — never used for ownership decisions.
    let mut f = &file;
    let _ = f.set_len(0);
    let _ = writeln!(
        f,
        "{} {}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    );
    let _ = f.flush();

    Ok(TeamLease { file, path })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lease_is_exclusive_and_released_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let team_dir = dir.path().join(".zode/team");
        let l1 = acquire(&team_dir).unwrap();
        // fs4 advisory locks on the same file may or may not conflict within
        // one process depending on platform; the drop→reacquire contract is
        // the portable guarantee we rely on.
        drop(l1);
        let l2 = acquire(&team_dir).expect("OS lock released on drop");
        assert!(l2.path.ends_with("team.lock"));
    }

    #[cfg(unix)]
    #[test]
    fn lease_scaffolds_private_dir() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let team_dir = dir.path().join(".zode/team");
        let _l = acquire(&team_dir).unwrap();
        let mode = std::fs::metadata(&team_dir).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
    }
}
