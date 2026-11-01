//! Autosave and crash recovery: when to write a recovery copy of unsaved work, where it goes, and which copies a
//! previous run left behind. The copies are written by a background thread; the frame loop only decides.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;

use crossbeam_channel::{Receiver, Sender};
use ondina_core::Project;
use serde_json::{json, Value};

/// 64-bit FNV-1a: stable across Rust versions, unlike `DefaultHasher`, so a copy keeps its name between runs.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// The readable part of a copy name: ASCII letters, digits, `-` and `_`, everything else `_`, at most 40 characters.
fn sanitize(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .take(40)
        .collect();
    if s.is_empty() { "project".into() } else { s }
}

/// File name (without extension) of the recovery copy of a document. `path` is where the document lives
/// (or the file it was recovered from); the instance id keeps two running Ondina windows from writing
/// over each other's copy.
pub fn copy_stem(path: Option<&Path>, instance: &str) -> String {
    let Some(p) = path else {
        return format!("untitled-{instance}");
    };
    let base = sanitize(&p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
    let key = p.to_string_lossy().to_lowercase().replace('/', "\\");
    format!("{base}-{:016x}-{instance}", fnv1a(key.as_bytes()))
}

/// Identity of this run of Ondina: the process id alone is not enough, because after a crash the next run can get
/// the same pid back and would take the dead run's copies for its own.
pub fn instance_id() -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    format!("{}-{millis}", std::process::id())
}

pub fn unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// What the recovery window and `leftover_copies` know about a copy, kept in `<stem>.json` next to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopyMeta {
    /// Project name shown in the recovery window.
    pub name: String,
    /// Where the document was saved, or the file it was recovered from; None for a project never saved.
    pub original: Option<String>,
    /// Unix seconds when the copy was written.
    pub saved_at: u64,
    /// Instance id of the Ondina that wrote it (see `instance_id`).
    pub instance: String,
}

pub fn meta_json(m: &CopyMeta) -> String {
    json!({
        "format": "ondina-autosave",
        "version": 1,
        "name": m.name,
        "original": m.original,
        "saved_at": m.saved_at,
        "instance": m.instance,
    })
    .to_string()
}

/// Reads the metadata of a copy. Missing fields take a default; None only when the text is not a JSON object.
pub fn parse_meta(s: &str) -> Option<CopyMeta> {
    let v: Value = serde_json::from_str(s).ok()?;
    let o = v.as_object()?;
    let text = |key: &str| o.get(key).and_then(Value::as_str);
    Some(CopyMeta {
        name: text("name").unwrap_or("Untitled").to_string(),
        original: text("original").filter(|s| !s.is_empty()).map(str::to_string),
        saved_at: o.get("saved_at").and_then(Value::as_u64).unwrap_or(0),
        instance: text("instance").unwrap_or("").to_string(),
    })
}

/// The metadata next to a copy (`<stem>.json`), if it is there and readable.
pub fn read_meta(copy: &Path) -> Option<CopyMeta> {
    std::fs::read_to_string(copy.with_extension("json")).ok().and_then(|s| parse_meta(&s))
}

/// What the frame loop knows, once per frame.
pub struct TickInput {
    /// egui time, seconds.
    pub now: f64,
    /// `Settings::autosave_minutes`.
    pub minutes: u32,
    pub dirty: bool,
    /// `View::document`.
    pub document: u64,
    /// `View::revision`.
    pub revision: u64,
    /// `copy_stem` of the current document.
    pub stem: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Todo {
    Write { stem: String },
    Delete { stem: String },
}

/// Decides, once per frame, when a copy is written and when it is removed.
pub struct Tracker {
    /// Stem of the copy this instance has on disk for the current document.
    written: Option<String>,
    written_revision: u64,
    document: u64,
    /// When the interval started counting (egui seconds).
    since: f64,
}

impl Tracker {
    pub fn new(document: u64) -> Tracker {
        Tracker { written: None, written_revision: 0, document, since: 0.0 }
    }

    pub fn tick(&mut self, input: &TickInput) -> Vec<Todo> {
        let mut todo = Vec::new();
        // New, Open or a recovery, from any source: the copy of the replaced document goes.
        if input.document != self.document {
            if let Some(stem) = self.written.take() {
                todo.push(Todo::Delete { stem });
            }
            self.document = input.document;
            self.written_revision = 0;
            self.since = input.now;
        }
        // Saved (from the GUI or a client) or autosave turned off: nothing to keep.
        if !input.dirty || input.minutes == 0 {
            if let Some(stem) = self.written.take() {
                todo.push(Todo::Delete { stem });
            }
            self.since = input.now;
            return todo;
        }
        // The copy on disk has another name: write the new one first, so there is never a moment without a copy.
        if let Some(old) = self.written.as_ref().filter(|w| **w != input.stem).cloned() {
            todo.push(Todo::Write { stem: input.stem.clone() });
            todo.push(Todo::Delete { stem: old });
            self.mark_written(input);
            return todo;
        }
        if input.revision != self.written_revision && input.now - self.since >= f64::from(input.minutes) * 60.0 {
            todo.push(Todo::Write { stem: input.stem.clone() });
            self.mark_written(input);
        }
        todo
    }

    fn mark_written(&mut self, input: &TickInput) {
        self.written = Some(input.stem.clone());
        self.written_revision = input.revision;
        self.since = input.now;
    }

    /// After a recovery: the copy at `stem` holds the current document, and is replaced by one of this instance.
    pub fn adopt(&mut self, document: u64, revision: u64, stem: String) {
        self.document = document;
        self.written_revision = revision;
        self.written = Some(stem);
    }

    pub fn take_written(&mut self) -> Option<String> {
        self.written.take()
    }
}

/// Writes `contents` to `path` with `fs_safe::write_replace`, so a crash in the middle never leaves a
/// half-written copy in place of the previous one.
pub fn write_atomic(path: &Path, contents: &str) -> Result<(), String> {
    ondina_studio::fs_safe::write_replace(path, contents.as_bytes())
}

/// Writes the copy `<stem>.ondina` and then its metadata `<stem>.json` in `dir`. Runs on the writer thread.
pub fn write_copy(dir: &Path, stem: &str, project: &Project, meta: &CopyMeta) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let text = ondina_core::save_project_string(project)?;
    let copy = dir.join(format!("{stem}.ondina"));
    write_atomic(&copy, &text)?;
    write_atomic(&dir.join(format!("{stem}.json")), &meta_json(meta))?;
    Ok(copy)
}

/// Removes `<stem>.ondina` and `<stem>.json`; a file already gone is not an error.
pub fn delete_copy(dir: &Path, stem: &str) {
    for ext in ["ondina", "json"] {
        let _ = std::fs::remove_file(dir.join(format!("{stem}.{ext}")));
    }
}

pub enum Job {
    Write { stem: String, project: Arc<Project>, meta: CopyMeta },
    Delete { stem: String },
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Wrote(String),
    Deleted(String),
    Failed(String),
}

/// The thread that serializes and writes the copies. One thread and one channel: jobs run in the order they were
/// sent, so a Delete never overtakes the Write sent before it.
pub struct Writer {
    tx: Option<Sender<Job>>,
    done: Receiver<Outcome>,
    handle: Option<JoinHandle<()>>,
}

impl Writer {
    pub fn spawn(dir: PathBuf) -> Writer {
        let (tx, rx) = crossbeam_channel::unbounded::<Job>();
        let (done_tx, done) = crossbeam_channel::unbounded();
        let handle = std::thread::Builder::new()
            .name("ondina-autosave".into())
            .spawn(move || {
                for job in rx {
                    let outcome = match job {
                        Job::Write { stem, project, meta } => match write_copy(&dir, &stem, &project, &meta) {
                            Ok(_) => Outcome::Wrote(stem),
                            Err(e) => Outcome::Failed(format!("{stem}: {e}")),
                        },
                        Job::Delete { stem } => {
                            delete_copy(&dir, &stem);
                            Outcome::Deleted(stem)
                        }
                    };
                    let _ = done_tx.send(outcome);
                }
            })
            .ok();
        Writer { tx: Some(tx), done, handle }
    }

    pub fn send(&self, job: Job) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(job);
        }
    }

    pub fn try_recv(&self) -> Option<Outcome> {
        self.done.try_recv().ok()
    }

    /// Closes the channel and waits for the thread: every job already sent runs first.
    pub fn finish(&mut self) {
        self.tx = None;
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn lock_path(dir: &Path, instance: &str) -> PathBuf {
    dir.join(format!("instance-{instance}.lock"))
}

/// Holds `instance-<id>.lock` open without sharing for the life of the app: another Ondina can then tell
/// that this instance is alive, because Windows refuses to delete a file that is open this way.
pub struct InstanceLock {
    file: Option<std::fs::File>,
    path: PathBuf,
}

impl InstanceLock {
    pub fn acquire(dir: &Path, instance: &str) -> Option<InstanceLock> {
        let path = lock_path(dir, instance);
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            // Rust's default share mode includes delete: without this the lock would not lock anything.
            opts.share_mode(0);
        }
        let file = opts.open(&path).ok()?;
        Some(InstanceLock { file: Some(file), path })
    }
}

impl Drop for InstanceLock {
    fn drop(&mut self) {
        drop(self.file.take());
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Whether the instance that wrote a copy is still running. A lock file that can be removed belongs to an
/// instance that is gone (and is removed on the way).
pub fn owner_alive(dir: &Path, instance: &str) -> bool {
    if instance.is_empty() {
        return false;
    }
    match std::fs::remove_file(lock_path(dir, instance)) {
        Ok(()) => false,
        Err(e) => e.kind() != std::io::ErrorKind::NotFound,
    }
}

/// A copy left by an instance that did not close cleanly.
#[derive(Clone, Debug)]
pub struct Leftover {
    pub copy: PathBuf,
    pub stem: String,
    pub meta: CopyMeta,
}

/// Copies in `dir` whose instance is neither `own` nor still running, newest first. Also removes lock files
/// of instances that are gone.
pub fn leftover_copies(dir: &Path, own: &str) -> Vec<Leftover> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut found = vec![];
    let mut locks = vec![];
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(id) = name.strip_prefix("instance-").and_then(|n| n.strip_suffix(".lock")) {
            if id != own {
                locks.push(path);
            }
            continue;
        }
        // `x.ondina.tmp` has the extension `tmp`: half-written files are never offered.
        if path.extension().is_some_and(|e| e == "ondina") {
            let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let meta = read_meta(&path).unwrap_or_else(|| CopyMeta { name: stem.clone(), original: None, saved_at: 0, instance: String::new() });
            found.push(Leftover { copy: path, stem, meta });
        }
    }
    found.retain(|l| l.meta.instance != own && !owner_alive(dir, &l.meta.instance));
    for lock in locks {
        let _ = std::fs::remove_file(lock);
    }
    found.sort_by_key(|l| std::cmp::Reverse(l.meta.saved_at));
    found
}

/// "3 minutes ago", for the recovery window.
pub fn age_label(now: u64, saved_at: u64) -> String {
    if saved_at == 0 {
        return "at an unknown time".into();
    }
    if saved_at > now {
        return "just now".into();
    }
    let d = now - saved_at;
    let unit = |n: u64, what: &str| format!("{n} {what}{} ago", if n == 1 { "" } else { "s" });
    match d {
        0..=59 => "less than a minute ago".into(),
        60..=3599 => unit(d / 60, "minute"),
        3600..=86_399 => unit(d / 3600, "hour"),
        _ => unit(d / 86_400, "day"),
    }
}

/// Folder and file name the Save As dialog proposes: the original file of recovered work, otherwise the project
/// name in the dialog's own folder.
pub fn save_dialog_default(name: &str, original: Option<&Path>) -> (Option<PathBuf>, String) {
    let fallback = format!("{name}.ondina");
    match original {
        Some(p) => (
            p.parent().filter(|d| !d.as_os_str().is_empty()).map(Path::to_path_buf),
            p.file_name().map_or(fallback, |f| f.to_string_lossy().into_owned()),
        ),
        None => (None, fallback),
    }
}

/// The status bar line after a recovery.
pub fn recovered_status(name: &str, result: &Value) -> String {
    let mut s = format!("Recovered '{name}' from an autosave copy: save it to keep it");
    let m = result["missing_audio_files"].as_array().map_or(0, Vec::len);
    if m > 0 {
        s += &format!(" · {m} audio {} missing", if m == 1 { "file" } else { "files" });
    }
    s
}

/// One frame of the document, as the autosave sees it.
pub struct Snapshot<'a> {
    pub now: f64,
    pub minutes: u32,
    pub dirty: bool,
    pub document: u64,
    pub revision: u64,
    pub project: &'a Arc<Project>,
    pub path: Option<&'a Path>,
    /// The original file of a recovered document that has not been saved yet.
    pub recovered: Option<&'a Path>,
}

/// Everything the app holds for autosave: the decision, the writer thread and the lock of this instance.
pub struct Autosave {
    instance: String,
    tracker: Tracker,
    writer: Writer,
    _lock: Option<InstanceLock>,
    last_error: Option<String>,
}

impl Autosave {
    /// Startup: lock for this instance, the copies left by others, and the writer thread.
    pub fn start(dir: PathBuf, document: u64) -> (Autosave, Vec<Leftover>) {
        let _ = std::fs::create_dir_all(&dir);
        let instance = instance_id();
        let lock = InstanceLock::acquire(&dir, &instance);
        let leftovers = leftover_copies(&dir, &instance);
        let writer = Writer::spawn(dir);
        let autosave = Autosave { instance, tracker: Tracker::new(document), writer, _lock: lock, last_error: None };
        (autosave, leftovers)
    }

    /// Decides for this frame and hands any work to the writer thread; never serializes or writes here.
    pub fn tick(&mut self, s: Snapshot) {
        let source = s.path.or(s.recovered);
        let input = TickInput {
            now: s.now,
            minutes: s.minutes,
            dirty: s.dirty,
            document: s.document,
            revision: s.revision,
            stem: copy_stem(source, &self.instance),
        };
        for todo in self.tracker.tick(&input) {
            match todo {
                Todo::Write { stem } => {
                    let meta = CopyMeta {
                        name: s.project.name.clone(),
                        original: source.map(|p| p.display().to_string()),
                        saved_at: unix_secs(),
                        instance: self.instance.clone(),
                    };
                    self.writer.send(Job::Write { stem, project: s.project.clone(), meta });
                }
                Todo::Delete { stem } => self.writer.send(Job::Delete { stem }),
            }
        }
    }

    /// Status line for the last failure, once per distinct error; None otherwise.
    pub fn poll(&mut self) -> Option<String> {
        let mut msg = None;
        while let Some(o) = self.writer.try_recv() {
            match o {
                Outcome::Failed(e) => {
                    if self.last_error.as_deref() != Some(e.as_str()) {
                        msg = Some(format!("⚠ Autosave failed: {e}"));
                        self.last_error = Some(e);
                    }
                }
                Outcome::Wrote(_) => self.last_error = None,
                Outcome::Deleted(_) => {}
            }
        }
        msg
    }

    /// Removes a copy left by an earlier run (Discard in the recovery window).
    pub fn discard(&self, stem: &str) {
        self.writer.send(Job::Delete { stem: stem.into() });
    }

    /// The recovered `copy` now holds the current document: it stays until this instance has written its own.
    pub fn adopt(&mut self, document: u64, revision: u64, copy: &Path) {
        let stem = copy.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        self.tracker.adopt(document, revision, stem);
    }

    /// Clean exit: deletes this instance's copy, waits for the writer, releases the lock.
    pub fn shutdown(&mut self) {
        if let Some(stem) = self.tracker.take_written() {
            self.writer.send(Job::Delete { stem });
        }
        self.writer.finish();
        self._lock.take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(now: f64, minutes: u32, dirty: bool, document: u64, revision: u64, stem: &str) -> TickInput {
        TickInput { now, minutes, dirty, document, revision, stem: stem.into() }
    }

    fn write(stem: &str) -> Todo {
        Todo::Write { stem: stem.into() }
    }

    fn delete(stem: &str) -> Todo {
        Todo::Delete { stem: stem.into() }
    }

    /// A tracker that has just written the copy "s" of document 1 at revision 1, at t = 180.
    fn tracker_with_copy() -> Tracker {
        let mut t = Tracker::new(1);
        assert!(t.tick(&input(0.0, 3, true, 1, 1, "s")).is_empty());
        assert_eq!(t.tick(&input(180.0, 3, true, 1, 1, "s")), vec![write("s")]);
        t
    }

    #[test]
    fn copy_stem_depends_on_the_path_not_on_its_spelling() {
        let a = copy_stem(Some(Path::new("C:\\Songs\\Night Drive.ondina")), "42-1000");
        let b = copy_stem(Some(Path::new("c:/songs/night drive.ondina")), "42-1000");
        // The readable part keeps the spelling of the file name; Windows file names ignore case, so both
        // spellings name the same copy on disk.
        assert!(a.eq_ignore_ascii_case(&b), "{a} / {b}");
        assert_eq!(a.split('-').nth(1), b.split('-').nth(1), "same hash");
        assert!(a.starts_with("Night_Drive-"), "{a}");
        assert!(a.ends_with("-42-1000"), "{a}");
    }

    #[test]
    fn copy_stem_tells_apart_files_with_the_same_name() {
        let a = copy_stem(Some(Path::new("C:\\A\\song.ondina")), "1-2");
        let b = copy_stem(Some(Path::new("C:\\B\\song.ondina")), "1-2");
        assert_ne!(a, b);
    }

    #[test]
    fn copy_stem_of_an_untitled_project_uses_the_instance() {
        assert_eq!(copy_stem(None, "42-1000"), "untitled-42-1000");
        assert_ne!(copy_stem(None, "42-1000"), copy_stem(None, "43-1000"));
    }

    #[test]
    fn copy_stem_keeps_only_safe_characters() {
        let s = copy_stem(Some(Path::new("C:\\x\\Canzone è:1?.ondina")), "7-8");
        let (base, rest) = s.split_at(s.len() - "-0123456789abcdef-7-8".len());
        assert_eq!(base, "Canzone___1_", "{s}");
        assert!(rest[1..17].chars().all(|c| c.is_ascii_hexdigit()), "{s}");
        let long = format!("C:\\x\\{}.ondina", "a".repeat(100));
        let s = copy_stem(Some(Path::new(&long)), "7-8");
        assert_eq!(s.len(), 40 + "-0123456789abcdef-7-8".len(), "{s}");
        assert!(s.starts_with(&format!("{}-", "a".repeat(40))), "{s}");
    }

    #[test]
    fn meta_round_trips_and_tolerates_missing_fields() {
        let mut m = CopyMeta { name: "Night Drive".into(), original: Some("C:\\Songs\\Night Drive.ondina".into()), saved_at: 1_700_000_000, instance: "42-1000".into() };
        assert_eq!(parse_meta(&meta_json(&m)), Some(m.clone()));
        m.original = None;
        assert_eq!(parse_meta(&meta_json(&m)), Some(m));
        assert_eq!(
            parse_meta(r#"{"saved_at": 5}"#),
            Some(CopyMeta { name: "Untitled".into(), original: None, saved_at: 5, instance: String::new() })
        );
        assert_eq!(parse_meta("not json"), None);
        assert_eq!(parse_meta("[1, 2]"), None);
        assert_eq!(parse_meta(r#"{"original": ""}"#).unwrap().original, None);
    }

    #[test]
    fn no_copy_while_clean_or_before_the_interval() {
        let mut t = Tracker::new(1);
        for i in 0..=10 {
            assert!(t.tick(&input(i as f64 * 60.0, 3, false, 1, 0, "s")).is_empty());
        }
        assert!(t.tick(&input(600.0, 3, true, 1, 1, "s")).is_empty());
        assert!(t.tick(&input(779.0, 3, true, 1, 1, "s")).is_empty());
        assert_eq!(t.tick(&input(780.0, 3, true, 1, 1, "s")), vec![write("s")]);
    }

    #[test]
    fn no_copy_when_autosave_is_off() {
        let mut t = Tracker::new(1);
        assert!(t.tick(&input(0.0, 0, true, 1, 1, "s")).is_empty());
        assert!(t.tick(&input(3600.0, 0, true, 1, 2, "s")).is_empty());
    }

    #[test]
    fn no_new_copy_without_new_changes() {
        let mut t = tracker_with_copy();
        assert!(t.tick(&input(780.0, 3, true, 1, 1, "s")).is_empty());
        assert_eq!(t.tick(&input(790.0, 3, true, 1, 2, "s")), vec![write("s")]);

        // A new change waits for the interval since the previous copy.
        let mut t = tracker_with_copy();
        assert!(t.tick(&input(200.0, 3, true, 1, 2, "s")).is_empty());
        assert!(t.tick(&input(359.0, 3, true, 1, 2, "s")).is_empty());
        assert_eq!(t.tick(&input(360.0, 3, true, 1, 2, "s")), vec![write("s")]);
    }

    #[test]
    fn copy_is_deleted_after_a_save() {
        let mut t = tracker_with_copy();
        assert_eq!(t.tick(&input(200.0, 3, false, 1, 2, "s")), vec![delete("s")]);
        assert!(t.tick(&input(201.0, 3, false, 1, 2, "s")).is_empty());
    }

    #[test]
    fn copy_is_deleted_when_the_document_changes() {
        let mut t = tracker_with_copy();
        assert_eq!(t.tick(&input(200.0, 3, false, 2, 2, "t")), vec![delete("s")]);

        let mut t = tracker_with_copy();
        assert_eq!(t.tick(&input(200.0, 3, true, 2, 2, "t")), vec![delete("s")]);
        assert!(t.tick(&input(379.0, 3, true, 2, 3, "t")).is_empty());
        assert_eq!(t.tick(&input(380.0, 3, true, 2, 3, "t")), vec![write("t")]);
    }

    #[test]
    fn turning_autosave_off_deletes_the_copy() {
        let mut t = tracker_with_copy();
        assert_eq!(t.tick(&input(200.0, 0, true, 1, 1, "s")), vec![delete("s")]);
        assert!(t.tick(&input(900.0, 0, true, 1, 2, "s")).is_empty());
    }

    #[test]
    fn new_stem_writes_before_deleting_the_old_copy() {
        let mut t = Tracker::new(1);
        t.tick(&input(0.0, 3, true, 1, 1, "a"));
        assert_eq!(t.tick(&input(180.0, 3, true, 1, 1, "a")), vec![write("a")]);
        assert_eq!(t.tick(&input(181.0, 3, true, 1, 1, "b")), vec![write("b"), delete("a")]);
        assert!(t.tick(&input(182.0, 3, true, 1, 1, "b")).is_empty());
    }

    #[test]
    fn adopted_copy_is_replaced_by_one_of_this_instance() {
        let mut t = Tracker::new(1);
        t.adopt(2, 5, "old".into());
        assert_eq!(t.tick(&input(10.0, 3, true, 2, 5, "new")), vec![write("new"), delete("old")]);

        let mut t = Tracker::new(1);
        t.adopt(2, 5, "old".into());
        assert_eq!(t.tick(&input(10.0, 0, true, 2, 5, "new")), vec![delete("old")]);
    }

    fn test_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join("ondina_autosave_tests").join(name);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn files_in(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        v.sort();
        v
    }

    fn lofi_beat() -> Project {
        use ondina_core::command::Command;
        use ondina_core::model::TrackKind;
        let mut p = Project::new("Lofi Beat");
        ondina_core::ops::apply(
            &mut p,
            &Command::AddTrack { kind: TrackKind::Instrument, name: None, patch: Some("Lush Pad".into()), instrument: None, preset: None, index: None },
        )
        .unwrap();
        p
    }

    fn meta(instance: &str, saved_at: u64) -> CopyMeta {
        CopyMeta { name: "Lofi Beat".into(), original: Some("C:\\Songs\\Lofi Beat.ondina".into()), saved_at, instance: instance.into() }
    }

    /// Waits (up to five seconds) for the writer thread.
    fn wait_for(what: impl Fn() -> bool) {
        for _ in 0..500 {
            if what() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("timed out");
    }

    #[test]
    fn copy_reads_back_as_the_same_project() {
        let dir = test_dir("read_back");
        let p = lofi_beat();
        let m = meta("1-2", 100);
        let copy = write_copy(&dir, "s", &p, &m).unwrap();
        assert_eq!(copy, dir.join("s.ondina"));
        let back = ondina_core::load_project_str(&std::fs::read_to_string(&copy).unwrap()).unwrap();
        assert_eq!(back.tracks.len(), 1);
        assert_eq!(ondina_core::save_project_string(&back).unwrap(), ondina_core::save_project_string(&p).unwrap());
        assert_eq!(parse_meta(&std::fs::read_to_string(dir.join("s.json")).unwrap()), Some(m.clone()));
        assert_eq!(read_meta(&copy), Some(m));
        assert!(files_in(&dir).iter().all(|f| !f.ends_with(".tmp")), "{:?}", files_in(&dir));
    }

    #[test]
    fn failed_temporary_write_keeps_the_previous_copy() {
        let dir = test_dir("failed_tmp");
        let path = dir.join("s.ondina");
        write_atomic(&path, "old").unwrap();
        // A folder where the temporary file should go: the write fails before touching the copy.
        std::fs::create_dir_all(dir.join("s.ondina.tmp")).unwrap();
        assert!(write_atomic(&path, "new").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old");
    }

    #[test]
    fn write_atomic_replaces_an_existing_copy() {
        let dir = test_dir("replace");
        let path = dir.join("s.ondina");
        write_atomic(&path, "first").unwrap();
        write_atomic(&path, "second").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "second");
        assert_eq!(files_in(&dir), vec!["s.ondina".to_string()]);
    }

    #[test]
    fn delete_copy_removes_project_and_meta() {
        let dir = test_dir("delete");
        write_copy(&dir, "s", &lofi_beat(), &meta("1-2", 100)).unwrap();
        delete_copy(&dir, "s");
        assert!(files_in(&dir).is_empty(), "{:?}", files_in(&dir));
        delete_copy(&dir, "s");
    }

    #[test]
    fn leftover_copies_skip_own_and_temporary_files() {
        let dir = test_dir("leftovers");
        let p = lofi_beat();
        write_copy(&dir, "one", &p, &meta("dead-1", 100)).unwrap();
        write_copy(&dir, "two", &p, &meta("dead-2", 200)).unwrap();
        write_copy(&dir, "mine", &p, &meta("me", 300)).unwrap();
        std::fs::write(dir.join("x.ondina.tmp"), "half").unwrap();
        std::fs::write(dir.join("bare.ondina"), ondina_core::save_project_string(&p).unwrap()).unwrap();
        std::fs::write(dir.join("instance-dead-9.lock"), "").unwrap();
        let found = leftover_copies(&dir, "me");
        let stems: Vec<&str> = found.iter().map(|l| l.stem.as_str()).collect();
        assert_eq!(stems, vec!["two", "one", "bare"]);
        assert_eq!(found[0].meta.instance, "dead-2");
        assert_eq!(found[0].copy, dir.join("two.ondina"));
        assert_eq!(found[2].meta, CopyMeta { name: "bare".into(), original: None, saved_at: 0, instance: String::new() });
        assert!(!dir.join("instance-dead-9.lock").exists(), "locks of instances that are gone are removed");
    }

    #[test]
    fn leftover_copies_of_a_missing_folder_are_empty() {
        let dir = test_dir("missing").join("not_there");
        assert!(leftover_copies(&dir, "me").is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn copies_of_a_running_instance_are_not_offered() {
        let dir = test_dir("running");
        let lock = InstanceLock::acquire(&dir, "alive").unwrap();
        write_copy(&dir, "s", &lofi_beat(), &meta("alive", 100)).unwrap();
        assert!(leftover_copies(&dir, "me").is_empty());
        assert!(owner_alive(&dir, "alive"));
        drop(lock);
        assert!(!dir.join("instance-alive.lock").exists());
        assert_eq!(leftover_copies(&dir, "me").len(), 1);
        assert!(!owner_alive(&dir, "alive"));
        assert!(!owner_alive(&dir, ""));
    }

    #[test]
    fn age_label_reads_like_a_person() {
        let now = 1_000_000;
        assert_eq!(age_label(now, now - 30), "less than a minute ago");
        assert_eq!(age_label(now, now - 60), "1 minute ago");
        assert_eq!(age_label(now, now - 180), "3 minutes ago");
        assert_eq!(age_label(now, now - 3600), "1 hour ago");
        assert_eq!(age_label(now, now - 7200), "2 hours ago");
        assert_eq!(age_label(now, now - 2 * 86_400), "2 days ago");
        assert_eq!(age_label(now, now + 10), "just now");
        assert_eq!(age_label(now, 0), "at an unknown time");
    }

    #[test]
    fn save_dialog_default_proposes_the_original_file() {
        assert_eq!(
            save_dialog_default("Whatever", Some(Path::new("C:\\Songs\\Night Drive.ondina"))),
            (Some(PathBuf::from("C:\\Songs")), "Night Drive.ondina".to_string())
        );
        assert_eq!(save_dialog_default("Lofi Beat", None), (None, "Lofi Beat.ondina".to_string()));
    }

    #[test]
    fn recovered_status_mentions_missing_audio() {
        assert_eq!(
            recovered_status("Night Drive", &json!({"missing_audio_files": []})),
            "Recovered 'Night Drive' from an autosave copy: save it to keep it"
        );
        assert_eq!(
            recovered_status("Night Drive", &json!({"missing_audio_files": ["a", "b"]})),
            "Recovered 'Night Drive' from an autosave copy: save it to keep it · 2 audio files missing"
        );
        assert_eq!(
            recovered_status("Night Drive", &json!({"missing_audio_files": ["a"]})),
            "Recovered 'Night Drive' from an autosave copy: save it to keep it · 1 audio file missing"
        );
    }

    #[test]
    fn writer_runs_jobs_in_order() {
        let dir = test_dir("writer_order");
        let mut w = Writer::spawn(dir.clone());
        w.send(Job::Write { stem: "s".into(), project: Arc::new(lofi_beat()), meta: meta("1-2", 100) });
        w.send(Job::Delete { stem: "s".into() });
        w.finish();
        assert!(files_in(&dir).is_empty(), "{:?}", files_in(&dir));
        assert_eq!(w.try_recv(), Some(Outcome::Wrote("s".into())));
        assert_eq!(w.try_recv(), Some(Outcome::Deleted("s".into())));
        assert_eq!(w.try_recv(), None);
    }

    #[test]
    fn writer_reports_a_failed_write() {
        let dir = test_dir("writer_fails");
        let not_a_folder = dir.join("file");
        std::fs::write(&not_a_folder, "x").unwrap();
        let mut w = Writer::spawn(not_a_folder);
        w.send(Job::Write { stem: "s".into(), project: Arc::new(lofi_beat()), meta: meta("1-2", 100) });
        w.finish();
        assert!(matches!(w.try_recv(), Some(Outcome::Failed(_))));
    }

    fn snapshot(now: f64, project: &Arc<Project>) -> Snapshot<'_> {
        Snapshot { now, minutes: 1, dirty: true, document: 1, revision: 1, project, path: None, recovered: None }
    }

    #[test]
    fn shutdown_deletes_this_instance_copy() {
        let dir = test_dir("shutdown");
        let (mut a, leftovers) = Autosave::start(dir.clone(), 1);
        assert!(leftovers.is_empty());
        let p = Arc::new(lofi_beat());
        a.tick(snapshot(0.0, &p));
        a.tick(snapshot(61.0, &p));
        let copy = dir.join(format!("untitled-{}.ondina", a.instance));
        wait_for(|| copy.exists() && copy.with_extension("json").exists());
        assert_eq!(a.poll(), None);
        a.shutdown();
        assert!(files_in(&dir).is_empty(), "{:?}", files_in(&dir));
        a.shutdown();
    }

    #[test]
    fn start_lists_copies_left_by_a_dead_instance() {
        let dir = test_dir("dead_instance");
        write_copy(&dir, "old", &lofi_beat(), &meta("dead", 100)).unwrap();
        let (mut a, leftovers) = Autosave::start(dir.clone(), 1);
        assert_eq!(leftovers.len(), 1);
        assert_eq!(leftovers[0].stem, "old");
        a.shutdown();
        assert!(dir.join("old.ondina").exists(), "a copy of another instance stays");
    }

    #[test]
    fn poll_reports_each_distinct_failure_once() {
        let dir = test_dir("poll_failure");
        let not_a_folder = dir.join("file");
        std::fs::write(&not_a_folder, "x").unwrap();
        let (mut a, _) = Autosave::start(dir.clone(), 1);
        a.writer.finish();
        a.writer = Writer::spawn(not_a_folder);
        let p = Arc::new(lofi_beat());
        a.tick(snapshot(0.0, &p));
        a.tick(snapshot(61.0, &p));
        a.writer.finish();
        assert!(a.poll().is_some_and(|m| m.starts_with("⚠ Autosave failed: ")));
        a.tracker.take_written();
        a.writer = Writer::spawn(dir.join("file"));
        a.tick(Snapshot { revision: 2, ..snapshot(122.0, &p) });
        a.writer.finish();
        assert_eq!(a.poll(), None, "the same error is not repeated");
        a.shutdown();
    }
}
