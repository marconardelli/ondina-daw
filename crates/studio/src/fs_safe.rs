//! Writing a file so that a failure never destroys the previous version: the new contents go to a new temporary
//! file next to it (never one that already exists), are forced to disk and only then take its place.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

const RENAME_ATTEMPTS: u32 = 5;
const RENAME_PAUSE_MS: u64 = 40;
const READ_ATTEMPTS: u32 = 10;
const READ_PAUSE_MS: u64 = 50;
const TEMP_ATTEMPTS: u32 = 64;
const STALE_TEMP_AGE: Duration = Duration::from_secs(600);

static TEMP_SEQ: AtomicU32 = AtomicU32::new(0);

thread_local! {
    static FAIL_NEXT_TEMP: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Makes the next temporary file this thread tries to create fail with "access denied", before anything is
/// written. For tests of the callers (Session::save, autosave): with unique temporary names nothing in the
/// folder can make that step fail any more.
#[doc(hidden)]
pub fn fail_next_temp_file_for_test() {
    FAIL_NEXT_TEMP.with(|f| f.set(true));
}

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

/// `Song.ondina` -> `Song.ondina.<pid>-<n>.tmp`.
fn temp_name(path: &Path, pid: u32, n: u32) -> PathBuf {
    sibling(path, &format!(".{pid}-{n}.tmp"))
}

/// Whether `file_name` is one of the temporary files `temp_name` makes for the file called `target_name`.
fn is_temp_of(file_name: &str, target_name: &str) -> bool {
    let Some(rest) = file_name.strip_prefix(target_name).and_then(|r| r.strip_prefix('.')).and_then(|r| r.strip_suffix(".tmp")) else {
        return false;
    };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    rest.split_once('-').is_some_and(|(pid, n)| digits(pid) && digits(n))
}

/// Creates the first name in `candidates` that does not exist yet. `create_new` never opens, truncates or follows
/// an existing file or link; a name already taken is skipped. On failure, the name tried last and the error.
fn create_first_free(candidates: impl IntoIterator<Item = PathBuf>) -> Result<(std::fs::File, PathBuf), (PathBuf, std::io::Error)> {
    let mut last = None;
    for c in candidates {
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&c) {
            Ok(f) => return Ok((f, c)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = Some((c, e)),
            Err(e) => return Err((c, e)),
        }
    }
    Err(last.unwrap_or_else(|| (PathBuf::new(), std::io::Error::from(std::io::ErrorKind::AlreadyExists))))
}

/// Writes `bytes` to a new temporary file in the same folder (never one that already exists), forces them to disk,
/// then renames the temporary file over `path`. On any failure the temporary file is removed and `path` is left as
/// it was.
pub fn write_replace(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = stage(path, bytes)?;
    commit(&tmp, path)?;
    sweep_stale_temps(path, STALE_TEMP_AGE);
    Ok(())
}

/// Like `write_replace`, but when `keep_backup` is set and `path` already is a file, its previous contents are
/// written to `<path>.bak` (one copy, replaced at every save) before the new file takes its place. An existing
/// `<path>.bak` is replaced only when it is a regular file that `is_ours` recognises as an earlier save; a folder,
/// a link or any other file is left as it is, the backup is skipped and `backup_error` says why. The file itself
/// is saved in every case.
pub fn replace_with_checked_backup(path: &Path, bytes: &[u8], keep_backup: bool, is_ours: impl Fn(&Path) -> Result<bool, String>) -> Result<Replaced, String> {
    let tmp = stage(path, bytes)?;
    let mut done = Replaced::default();
    if keep_backup && path.is_file() {
        let bak = sibling(path, ".bak");
        let copied = backup_allowed(&bak, &is_ours)
            .and_then(|()| std::fs::read(path).map_err(|e| format!("{}: {e}", path.display())))
            .and_then(|old| write_replace(&bak, &old));
        match copied {
            Ok(()) => done.backup = Some(bak),
            Err(e) => done.backup_error = Some(e),
        }
    }
    commit(&tmp, path)?;
    sweep_stale_temps(path, STALE_TEMP_AGE);
    Ok(done)
}

/// `replace_with_checked_backup` for files whose `.bak` holds plain bytes: any regular file there counts as an
/// earlier save (a folder or a link still does not).
pub fn replace_with_backup(path: &Path, bytes: &[u8], keep_backup: bool) -> Result<Replaced, String> {
    replace_with_checked_backup(path, bytes, keep_backup, |_| Ok(true))
}

/// Whether `bak` may take the previous version: the name is free, or it holds a regular file that `is_ours`
/// accepts. Otherwise the reason, which becomes `backup_error`.
fn backup_allowed(bak: &Path, is_ours: &impl Fn(&Path) -> Result<bool, String>) -> Result<(), String> {
    let meta = match std::fs::symlink_metadata(bak) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("cannot check {}: {e}", bak.display())),
    };
    if !meta.file_type().is_file() {
        return Err(format!("{} already exists and is a folder or a link, so it was left as it is", bak.display()));
    }
    if !is_ours(bak)? {
        return Err(format!("{} already exists and is not an earlier save, so it was left as it is", bak.display()));
    }
    Ok(())
}

/// `std::fs::read`, retried for about half a second on the errors another program causes for a moment
/// (see `should_retry`): an antivirus or a cloud sync holding the file when Ondina starts.
pub fn read_retrying(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut attempt = 1;
    loop {
        match std::fs::read(path) {
            Err(e) if attempt < READ_ATTEMPTS && should_retry(&e) => {
                attempt += 1;
                std::thread::sleep(Duration::from_millis(READ_PAUSE_MS));
            }
            other => return other,
        }
    }
}

/// Removes temporary files of `path` left by a save that never finished (a crash, a power cut): only regular files
/// named by `temp_name` for this very file and older than `older_than`, so a save still running in another
/// Ondina window is never disturbed. Best effort: any error is ignored.
fn sweep_stale_temps(path: &Path, older_than: Duration) {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return;
    };
    let folder = match path.parent() {
        Some(d) if !d.as_os_str().is_empty() => d,
        _ => Path::new("."),
    };
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        if !entry.file_name().to_str().is_some_and(|n| is_temp_of(n, name)) {
            continue;
        }
        let entry_path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&entry_path) else {
            continue;
        };
        let stale = meta.modified().ok().and_then(|m| m.elapsed().ok()).is_some_and(|age| age > older_than);
        if meta.file_type().is_file() && stale {
            let _ = std::fs::remove_file(&entry_path);
        }
    }
}

/// Step 1: a new temporary file written and synced. On failure a file we created is removed; a name we could not
/// create is never touched.
fn stage(path: &Path, bytes: &[u8]) -> Result<PathBuf, String> {
    let pid = std::process::id();
    if FAIL_NEXT_TEMP.with(|f| f.replace(false)) {
        return Err(failed(path, &temp_name(path, pid, 0), &std::io::Error::from_raw_os_error(5)));
    }
    let (mut file, tmp) = create_first_free((0..TEMP_ATTEMPTS).map(|_| temp_name(path, pid, TEMP_SEQ.fetch_add(1, Ordering::Relaxed)))).map_err(|(at, e)| failed(path, &at, &e))?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    match written {
        Ok(()) => Ok(tmp),
        Err(e) => {
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
        fail_next_temp_file_for_test();
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
        fail_next_temp_file_for_test();
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

    #[test]
    fn temp_names_belong_to_their_file() {
        assert_eq!(temp_name(Path::new("C:\\S\\a.ondina"), 12, 3), PathBuf::from("C:\\S\\a.ondina.12-3.tmp"));
        assert!(is_temp_of("a.ondina.12-3.tmp", "a.ondina"));
        for other in ["a.ondina.tmp", "a.ondina.12.tmp", "a.ondina.-3.tmp", "a.ondina.12-.tmp", "a.ondina.x-3.tmp", "a.ondina.bak.12-3.tmp", "b.ondina.12-3.tmp", "a.ondina.12-3.tmp.old"] {
            assert!(!is_temp_of(other, "a.ondina"), "{other}");
        }
    }

    #[test]
    fn create_first_free_skips_names_already_taken() {
        let dir = test_dir("first_free");
        let taken1 = dir.join("taken1");
        let taken2 = dir.join("taken2");
        let free = dir.join("free");
        std::fs::write(&taken1, b"keep").unwrap();
        std::fs::write(&taken2, b"keep").unwrap();
        let (f, at) = create_first_free([taken1.clone(), taken2.clone(), free.clone()]).unwrap();
        drop(f);
        assert_eq!(at, free);
        assert_eq!(std::fs::read(&taken1).unwrap(), b"keep");
        assert_eq!(std::fs::read(&taken2).unwrap(), b"keep");
        let (last, e) = create_first_free([taken1.clone()]).unwrap_err();
        assert_eq!(last, taken1);
        assert_eq!(e.kind(), io::ErrorKind::AlreadyExists);
    }

    #[test]
    fn existing_files_named_like_the_temporary_file_are_left_alone() {
        let dir = test_dir("named_like_tmp");
        let p = dir.join("a.ondina");
        let victim = dir.join("victim.txt");
        std::fs::write(&victim, b"keep me").unwrap();
        std::fs::hard_link(&victim, dir.join("a.ondina.tmp")).unwrap();
        std::fs::write(dir.join("a.ondina.bak.tmp"), b"keep me too").unwrap();
        write_replace(&p, b"v1").unwrap();
        replace_with_backup(&p, b"v2", true).unwrap();
        replace_with_backup(&p, b"v3", true).unwrap();
        assert_eq!(std::fs::read(&victim).unwrap(), b"keep me");
        assert_eq!(std::fs::read(dir.join("a.ondina.tmp")).unwrap(), b"keep me");
        assert_eq!(std::fs::read(dir.join("a.ondina.bak.tmp")).unwrap(), b"keep me too");
        assert_eq!(std::fs::read(&p).unwrap(), b"v3");
        assert_eq!(std::fs::read(dir.join("a.ondina.bak")).unwrap(), b"v2");
        assert_eq!(names(&dir), vec!["a.ondina", "a.ondina.bak", "a.ondina.bak.tmp", "a.ondina.tmp", "victim.txt"]);
    }

    #[cfg(windows)]
    #[test]
    fn link_named_like_the_temporary_file_is_left_alone() {
        let dir = test_dir("link_like_tmp");
        let p = dir.join("a.ondina");
        let victim = dir.join("victim.txt");
        std::fs::write(&victim, b"keep me").unwrap();
        let link = dir.join("a.ondina.tmp");
        if let Err(e) = std::os::windows::fs::symlink_file(&victim, &link) {
            eprintln!("symbolic links cannot be created here ({e}): nothing to check");
            return;
        }
        write_replace(&p, b"new").unwrap();
        assert_eq!(std::fs::read(&victim).unwrap(), b"keep me");
        assert_eq!(std::fs::read(&p).unwrap(), b"new");
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
    }

    #[test]
    fn folder_named_like_the_temporary_file_no_longer_blocks_saving() {
        let dir = test_dir("folder_like_tmp");
        let p = dir.join("a.ondina");
        std::fs::create_dir_all(dir.join("a.ondina.tmp")).unwrap();
        write_replace(&p, b"new").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"new");
        assert!(dir.join("a.ondina.tmp").is_dir());
    }

    #[test]
    fn failed_rename_leaves_no_temporary_file_of_any_name() {
        let dir = test_dir("failed_rename_any");
        let p = dir.join("target.ondina");
        std::fs::create_dir_all(&p).unwrap();
        assert!(write_replace(&p, b"new").is_err());
        assert_eq!(names(&dir), vec!["target.ondina"]);
    }

    #[test]
    fn hooked_failure_writes_nothing() {
        let dir = test_dir("hooked");
        let p = dir.join("a.ondina");
        write_replace(&p, b"old").unwrap();
        fail_next_temp_file_for_test();
        let e = write_replace(&p, b"new").unwrap_err();
        assert!(e.contains("cannot save"), "{e}");
        assert!(e.contains("nothing was overwritten"), "{e}");
        assert_eq!(std::fs::read(&p).unwrap(), b"old");
        assert_eq!(names(&dir), vec!["a.ondina"]);
        write_replace(&p, b"new").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"new");
    }

    fn age(p: &Path) {
        let f = std::fs::File::options().write(true).open(p).unwrap();
        f.set_modified(std::time::SystemTime::now() - Duration::from_secs(3600)).unwrap();
    }

    #[test]
    fn stale_temporary_files_of_the_same_file_are_swept() {
        let dir = test_dir("sweep");
        let p = dir.join("a.ondina");
        for (name, old) in [("a.ondina.999-1.tmp", true), ("a.ondina.999-2.tmp", false), ("a.ondina.tmp", true), ("b.ondina.999-1.tmp", true), ("a.ondina.bak.999-1.tmp", true)] {
            let f = dir.join(name);
            std::fs::write(&f, b"left").unwrap();
            if old {
                age(&f);
            }
        }
        std::fs::create_dir_all(dir.join("a.ondina.999-3.tmp")).unwrap();
        write_replace(&p, b"x").unwrap();
        assert_eq!(names(&dir), vec!["a.ondina", "a.ondina.999-2.tmp", "a.ondina.999-3.tmp", "a.ondina.bak.999-1.tmp", "a.ondina.tmp", "b.ondina.999-1.tmp"]);
        replace_with_backup(&p, b"y", true).unwrap();
        replace_with_backup(&p, b"z", true).unwrap();
        assert_eq!(names(&dir), vec!["a.ondina", "a.ondina.999-2.tmp", "a.ondina.999-3.tmp", "a.ondina.bak", "a.ondina.tmp", "b.ondina.999-1.tmp"]);
    }

    #[test]
    fn failed_save_sweeps_nothing() {
        let dir = test_dir("sweep_failed");
        let p = dir.join("a.ondina");
        let left = dir.join("a.ondina.999-1.tmp");
        std::fs::write(&left, b"left").unwrap();
        age(&left);
        fail_next_temp_file_for_test();
        assert!(write_replace(&p, b"x").is_err());
        assert!(left.is_file());
    }

    #[test]
    fn bak_that_is_not_an_earlier_save_is_left_alone() {
        let dir = test_dir("bak_not_ours");
        let p = dir.join("a.ondina");
        let bak = dir.join("a.ondina.bak");
        let is_ours = |b: &Path| Ok(std::fs::read(b).map(|x| x.starts_with(b"proj")).unwrap_or(false));
        replace_with_checked_backup(&p, b"proj-1", true, is_ours).unwrap();
        std::fs::write(&bak, b"my notes").unwrap();
        let r = replace_with_checked_backup(&p, b"proj-2", true, is_ours).unwrap();
        assert_eq!(r.backup, None);
        let e = r.backup_error.expect("the backup is skipped");
        assert!(e.contains("a.ondina.bak"), "{e}");
        assert!(e.contains("not an earlier save"), "{e}");
        assert_eq!(std::fs::read(&bak).unwrap(), b"my notes");
        assert_eq!(std::fs::read(&p).unwrap(), b"proj-2");
        std::fs::write(&bak, b"proj-0").unwrap();
        let r = replace_with_checked_backup(&p, b"proj-3", true, is_ours).unwrap();
        assert_eq!(r.backup, Some(bak.clone()));
        assert_eq!(r.backup_error, None);
        assert_eq!(std::fs::read(&bak).unwrap(), b"proj-2");
        assert_eq!(std::fs::read(&p).unwrap(), b"proj-3");
    }

    #[test]
    fn bak_that_is_a_folder_is_left_alone() {
        let dir = test_dir("bak_folder");
        let p = dir.join("a.ondina");
        let bak = dir.join("a.ondina.bak");
        std::fs::create_dir_all(&bak).unwrap();
        replace_with_backup(&p, b"v1", true).unwrap();
        let r = replace_with_backup(&p, b"v2", true).unwrap();
        assert_eq!(r.backup, None);
        let e = r.backup_error.expect("the backup is skipped");
        assert!(e.contains("folder or a link"), "{e}");
        assert!(bak.is_dir());
        assert_eq!(std::fs::read(&p).unwrap(), b"v2");
        assert_eq!(names(&dir), vec!["a.ondina", "a.ondina.bak"]);
    }

    #[test]
    fn bak_check_error_skips_only_the_backup() {
        let dir = test_dir("bak_check_error");
        let p = dir.join("a.ondina");
        let bak = dir.join("a.ondina.bak");
        write_replace(&p, b"v1").unwrap();
        std::fs::write(&bak, b"whatever").unwrap();
        let r = replace_with_checked_backup(&p, b"v2", true, |_| Err("cannot read it".to_string())).unwrap();
        assert_eq!(r.backup, None);
        assert_eq!(r.backup_error.as_deref(), Some("cannot read it"));
        assert_eq!(std::fs::read(&bak).unwrap(), b"whatever");
        assert_eq!(std::fs::read(&p).unwrap(), b"v2");
    }

    #[test]
    fn read_retrying_reads_and_reports_missing_files_at_once() {
        let dir = test_dir("read_retrying");
        let p = dir.join("s.json");
        std::fs::write(&p, b"abc").unwrap();
        assert_eq!(read_retrying(&p).unwrap(), b"abc");
        let started = std::time::Instant::now();
        assert_eq!(read_retrying(&dir.join("missing.json")).unwrap_err().kind(), io::ErrorKind::NotFound);
        assert!(started.elapsed() < Duration::from_millis(READ_PAUSE_MS));
    }
}
