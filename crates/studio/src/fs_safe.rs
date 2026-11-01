//! Writing a file so that a failure never destroys the previous version: the new contents go to a temporary file
//! next to it, are forced to disk and only then take its place.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

const RENAME_ATTEMPTS: u32 = 5;
const RENAME_PAUSE_MS: u64 = 40;

/// What `replace_with_backup` did besides writing the file.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Replaced {
    /// The copy of the previous file, when one was written.
    pub backup: Option<PathBuf>,
    /// Why the copy of the previous file could not be written; the file itself was saved anyway.
    pub backup_error: Option<String>,
}

/// `path` with `suffix` appended to the whole file name: `Song.ondina` + ".bak" -> `Song.ondina.bak`.
pub fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Writes `bytes` to `<path>.tmp` in the same folder, forces them to disk, then renames the temporary file over
/// `path`. On any failure the temporary file is removed and `path` is left as it was.
pub fn write_replace(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = stage(path, bytes)?;
    commit(&tmp, path)
}

/// Like `write_replace`, but when `keep_backup` is set and `path` already is a file, its previous contents are
/// written to `<path>.bak` (one copy, replaced at every save) before the new file takes its place.
pub fn replace_with_backup(path: &Path, bytes: &[u8], keep_backup: bool) -> Result<Replaced, String> {
    let tmp = stage(path, bytes)?;
    let mut done = Replaced::default();
    if keep_backup && path.is_file() {
        let bak = sibling(path, ".bak");
        let copied = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display())).and_then(|old| write_replace(&bak, &old));
        match copied {
            Ok(()) => done.backup = Some(bak),
            Err(e) => done.backup_error = Some(e),
        }
    }
    commit(&tmp, path)?;
    Ok(done)
}

/// Step 1: `<path>.tmp` written and synced. On failure the temporary file is removed.
fn stage(path: &Path, bytes: &[u8]) -> Result<PathBuf, String> {
    let tmp = sibling(path, ".tmp");
    let written = std::fs::File::create(&tmp).and_then(|mut f| {
        f.write_all(bytes)?;
        f.sync_all()
    });
    match written {
        Ok(()) => Ok(tmp),
        Err(e) => {
            // A folder in the way makes `remove_file` fail and stays where it is: it is not ours to delete.
            let _ = std::fs::remove_file(&tmp);
            Err(failed(path, &tmp, &e))
        }
    }
}

/// Step 3: the rename, retried briefly; on failure the temporary file is removed.
fn commit(tmp: &Path, path: &Path) -> Result<(), String> {
    let mut attempt = 1;
    loop {
        match std::fs::rename(tmp, path) {
            Ok(()) => return Ok(()),
            Err(e) if attempt < RENAME_ATTEMPTS && should_retry(&e) => {
                attempt += 1;
                std::thread::sleep(Duration::from_millis(RENAME_PAUSE_MS));
            }
            Err(e) => {
                let _ = std::fs::remove_file(tmp);
                return Err(failed(path, path, &e));
            }
        }
    }
}

/// Errors another program causes for a moment (antivirus, cloud sync, an indexer holding the file):
/// ERROR_ACCESS_DENIED (5), ERROR_SHARING_VIOLATION (32), ERROR_LOCK_VIOLATION (33).
fn should_retry(e: &std::io::Error) -> bool {
    matches!(e.raw_os_error(), Some(5 | 32 | 33))
}

/// The message a person reads: "cannot save <target>: <reason> (nothing was overwritten)".
fn failed(target: &Path, at: &Path, e: &std::io::Error) -> String {
    let reason = if e.kind() == std::io::ErrorKind::StorageFull || matches!(e.raw_os_error(), Some(39 | 112)) {
        "not enough disk space".to_string()
    } else if matches!(e.raw_os_error(), Some(32 | 33)) {
        format!("{} is in use by another program", at.display())
    } else {
        format!("{}: {e}", at.display())
    };
    format!("cannot save {}: {reason} (nothing was overwritten)", target.display())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    fn test_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join("ondina_fs_safe_tests").join(name);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        v.sort();
        v
    }

    #[test]
    fn write_replace_creates_then_replaces_and_leaves_no_temporary_file() {
        let dir = test_dir("create_replace");
        let p = dir.join("a.ondina");
        write_replace(&p, b"first").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"first");
        write_replace(&p, b"second").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"second");
        assert_eq!(names(&dir), vec!["a.ondina"]);
    }

    #[test]
    fn failed_temporary_write_keeps_the_previous_file() {
        let dir = test_dir("failed_tmp");
        let p = dir.join("a.ondina");
        write_replace(&p, b"old").unwrap();
        std::fs::create_dir_all(dir.join("a.ondina.tmp")).unwrap();
        let e = write_replace(&p, b"new").unwrap_err();
        assert!(e.contains("cannot save"), "{e}");
        assert!(e.contains("nothing was overwritten"), "{e}");
        assert_eq!(std::fs::read(&p).unwrap(), b"old");
        assert!(dir.join("a.ondina.tmp").is_dir());
    }

    #[test]
    fn failed_rename_removes_the_temporary_file() {
        let dir = test_dir("failed_rename");
        let p = dir.join("target.ondina");
        std::fs::create_dir_all(&p).unwrap();
        assert!(write_replace(&p, b"new").is_err());
        assert!(!dir.join("target.ondina.tmp").exists());
    }

    #[test]
    fn backup_holds_the_exact_previous_bytes() {
        let dir = test_dir("backup");
        let p = dir.join("a.ondina");
        let bak = dir.join("a.ondina.bak");
        assert_eq!(replace_with_backup(&p, b"v1", true).unwrap(), Replaced::default());
        assert!(!bak.exists());
        let r = replace_with_backup(&p, b"v2", true).unwrap();
        assert_eq!(r.backup, Some(bak.clone()));
        assert_eq!(r.backup_error, None);
        assert_eq!(std::fs::read(&bak).unwrap(), b"v1");
        assert_eq!(std::fs::read(&p).unwrap(), b"v2");
        replace_with_backup(&p, b"v3", true).unwrap();
        assert_eq!(std::fs::read(&bak).unwrap(), b"v2");
        assert_eq!(std::fs::read(&p).unwrap(), b"v3");
        assert_eq!(names(&dir), vec!["a.ondina", "a.ondina.bak"]);
    }

    #[test]
    fn no_backup_when_turned_off() {
        let dir = test_dir("no_backup");
        let p = dir.join("a.ondina");
        assert_eq!(replace_with_backup(&p, b"v1", false).unwrap(), Replaced::default());
        assert_eq!(replace_with_backup(&p, b"v2", false).unwrap(), Replaced::default());
        assert!(!dir.join("a.ondina.bak").exists());
        assert_eq!(names(&dir), vec!["a.ondina"]);
    }

    #[test]
    fn failed_save_does_not_touch_the_backup() {
        let dir = test_dir("failed_backup");
        let p = dir.join("a.ondina");
        let bak = dir.join("a.ondina.bak");
        replace_with_backup(&p, b"v1", true).unwrap();
        replace_with_backup(&p, b"v2", true).unwrap();
        assert_eq!(std::fs::read(&bak).unwrap(), b"v1");
        std::fs::create_dir_all(dir.join("a.ondina.tmp")).unwrap();
        assert!(replace_with_backup(&p, b"v3", true).is_err());
        assert_eq!(std::fs::read(&p).unwrap(), b"v2");
        assert_eq!(std::fs::read(&bak).unwrap(), b"v1");
    }

    #[test]
    fn sharing_and_access_errors_are_retried_disk_full_is_not() {
        assert!(should_retry(&io::Error::from_raw_os_error(32)));
        assert!(should_retry(&io::Error::from_raw_os_error(33)));
        assert!(should_retry(&io::Error::from_raw_os_error(5)));
        assert!(!should_retry(&io::Error::from_raw_os_error(112)));
        assert!(!should_retry(&io::Error::from(io::ErrorKind::NotFound)));
    }

    #[test]
    fn disk_full_reads_as_not_enough_disk_space() {
        let target = Path::new("C:\\S\\a.ondina");
        let at = Path::new("C:\\S\\a.ondina.tmp");
        let full = failed(target, at, &io::Error::from_raw_os_error(112));
        assert!(full.contains("not enough disk space"), "{full}");
        assert!(full.contains("nothing was overwritten"), "{full}");
        let busy = failed(target, at, &io::Error::from_raw_os_error(32));
        assert!(busy.contains("in use by another program"), "{busy}");
    }

    #[cfg(windows)]
    #[test]
    fn locked_target_is_left_untouched() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = test_dir("locked");
        let p = dir.join("a.ondina");
        write_replace(&p, b"old").unwrap();
        let handle = std::fs::OpenOptions::new().read(true).share_mode(0).open(&p).unwrap();
        assert!(write_replace(&p, b"new").is_err());
        drop(handle);
        assert_eq!(std::fs::read(&p).unwrap(), b"old");
        assert!(!dir.join("a.ondina.tmp").exists());
    }
}
