use std::{
    collections::HashMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
};

use anyhow::{Context, Result, anyhow};
use futures::channel::oneshot;
use gpui::{App, Global};
use serde::{Serialize, de::DeserializeOwned};
#[cfg(unix)]
use std::fs::File;

use super::tide::AiSettings;

#[derive(Clone, Copy)]
pub enum Format {
    Json,
    Toml,
}

pub struct Loaded<T> {
    pub value: T,
    pub recovered: bool,
}

fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".bak");
    name.into()
}

fn parse<T: DeserializeOwned>(bytes: &[u8], format: Format) -> Result<T> {
    // Do not include TOML source snippets in errors: they may contain an API key.
    let value = std::str::from_utf8(bytes).map_err(|_| anyhow!("Invalid file encoding"))?;
    match format {
        Format::Json => serde_json::from_str(value).map_err(|_| anyhow!("Invalid JSON data")),
        Format::Toml => toml::from_str(value).map_err(|_| anyhow!("Invalid TOML configuration")),
    }
}

fn atomic_replace(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("File has no parent directory")?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(())
}

fn preserve_corrupt_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("File has no parent directory")?;
    let prefix = format!(
        "{}.corrupt-",
        path.file_name()
            .context("File has no name")?
            .to_string_lossy()
    );
    let mut copy = tempfile::Builder::new()
        .prefix(&prefix)
        .tempfile_in(parent)?;
    copy.write_all(bytes)?;
    copy.as_file().sync_all()?;
    let (_, preserved_path) = copy.keep().map_err(|error| error.error)?;
    tracing::warn!(path = %path.display(), preserved = %preserved_path.display(), "Preserved damaged file");
    Ok(())
}

pub fn load_file<T: DeserializeOwned + Default>(path: &Path, format: Format) -> Result<Loaded<T>> {
    let backup = backup_path(path);
    if !path.try_exists()? && !backup.try_exists()? {
        return Ok(Loaded {
            value: T::default(),
            recovered: false,
        });
    }

    let primary = fs::read(path);
    if let Ok(bytes) = &primary
        && let Ok(value) = parse(bytes, format)
    {
        return Ok(Loaded {
            value,
            recovered: false,
        });
    }

    let bytes = fs::read(&backup).with_context(|| {
        format!(
            "Cannot load {} or its backup {}",
            path.display(),
            backup.display()
        )
    })?;
    let value = parse(&bytes, format).with_context(|| {
        format!(
            "Cannot recover {} from {}",
            path.display(),
            backup.display()
        )
    })?;
    match primary {
        Ok(damaged) => preserve_corrupt_file(path, &damaged)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).with_context(|| format!("Cannot read {}", path.display())),
    }
    atomic_replace(path, &bytes)?;
    Ok(Loaded {
        value,
        recovered: true,
    })
}

pub fn save_file<T: Serialize + DeserializeOwned>(
    path: &Path,
    value: &T,
    format: Format,
) -> Result<()> {
    let bytes = match format {
        Format::Json => serde_json::to_vec_pretty(value)?,
        Format::Toml => toml::to_string(value)?.into_bytes(),
    };
    match fs::read(path) {
        Ok(previous) => {
            if parse::<T>(&previous, format).is_ok() {
                atomic_replace(&backup_path(path), &previous)?;
            } else {
                preserve_corrupt_file(path, &previous)?;
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let backup = backup_path(path);
            if !backup.try_exists()? {
                atomic_replace(&backup, &bytes)?;
            }
        }
        Err(error) => return Err(error.into()),
    }
    atomic_replace(path, &bytes).with_context(|| format!("Cannot save {}", path.display()))
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SaveKind {
    Config,
    Tasks,
}

pub type SaveResult = std::result::Result<(), String>;
type Receipt = oneshot::Receiver<SaveResult>;
type Operation = Box<dyn Fn() -> Result<()> + Send>;

enum Command {
    Save(SaveKind, Operation, oneshot::Sender<SaveResult>),
    Flush(oneshot::Sender<SaveResult>),
}

#[derive(Clone)]
pub struct SaveQueue {
    sender: mpsc::Sender<Command>,
}

impl SaveQueue {
    pub fn new() -> Result<Self> {
        let (sender, receiver) = mpsc::channel();
        thread::Builder::new()
            .name("tide-save".into())
            .spawn(move || {
                let mut failed: HashMap<SaveKind, Operation> = HashMap::new();
                for command in receiver {
                    match command {
                        Command::Save(kind, operation, done) => {
                            let result = operation().map_err(|error| format!("{error:#}"));
                            if result.is_err() {
                                failed.insert(kind, operation);
                            } else {
                                failed.remove(&kind);
                            }
                            let _ = done.send(result);
                        }
                        Command::Flush(done) => {
                            let mut errors = Vec::new();
                            failed.retain(|_, operation| match operation() {
                                Ok(()) => false,
                                Err(error) => {
                                    errors.push(format!("{error:#}"));
                                    true
                                }
                            });
                            let _ = done.send(if errors.is_empty() {
                                Ok(())
                            } else {
                                Err(errors.join("\n"))
                            });
                        }
                    }
                }
            })?;
        Ok(Self { sender })
    }

    fn enqueue(&self, kind: SaveKind, operation: Operation) -> Receipt {
        let (sender, receiver) = oneshot::channel();
        let _ = self.sender.send(Command::Save(kind, operation, sender));
        receiver
    }

    pub fn flush(&self) -> Receipt {
        let (sender, receiver) = oneshot::channel();
        let _ = self.sender.send(Command::Flush(sender));
        receiver
    }
}

pub async fn wait_for_save(receipt: Receipt) -> SaveResult {
    receipt
        .await
        .unwrap_or_else(|_| Err("Save worker stopped".into()))
}

pub struct Persistence {
    pub queue: SaveQueue,
    pub recovered: bool,
    pub saved_ai: AiSettings,
    saved_ai_revision: u64,
    config_snapshots: HashMap<u64, AiSettings>,
    revision: u64,
    completed: HashMap<SaveKind, u64>,
    errors: HashMap<SaveKind, (u64, String)>,
}

impl Global for Persistence {}

impl Persistence {
    pub fn new(recovered: bool, saved_ai: AiSettings) -> Result<Self> {
        Ok(Self {
            queue: SaveQueue::new()?,
            recovered,
            saved_ai,
            saved_ai_revision: 0,
            config_snapshots: HashMap::new(),
            revision: 0,
            completed: HashMap::new(),
            errors: HashMap::new(),
        })
    }

    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    pub fn enqueue(
        &mut self,
        kind: SaveKind,
        operation: impl Fn() -> Result<()> + Send + 'static,
    ) -> (u64, Receipt) {
        self.revision += 1;
        (self.revision, self.queue.enqueue(kind, Box::new(operation)))
    }

    pub fn record(&mut self, kind: SaveKind, revision: u64, result: &SaveResult) {
        if self
            .completed
            .get(&kind)
            .is_some_and(|completed| *completed >= revision)
        {
            return;
        }
        self.completed.insert(kind, revision);
        if kind == SaveKind::Config {
            if result.is_ok() {
                if revision >= self.saved_ai_revision
                    && let Some(ai) = self.config_snapshots.remove(&revision)
                {
                    self.saved_ai = ai;
                    self.saved_ai_revision = revision;
                }
                self.config_snapshots
                    .retain(|pending, _| *pending > revision);
            } else {
                self.config_snapshots
                    .retain(|pending, _| *pending >= revision);
            }
        }
        match result {
            Ok(()) => {
                self.errors.remove(&kind);
            }
            Err(error) => {
                self.errors.insert(kind, (revision, error.clone()));
            }
        }
    }

    pub fn enqueue_config(
        &mut self,
        ai: AiSettings,
        operation: impl Fn() -> Result<()> + Send + 'static,
    ) -> (u64, Receipt) {
        let (revision, receipt) = self.enqueue(SaveKind::Config, operation);
        self.config_snapshots.insert(revision, ai);
        (revision, receipt)
    }

    fn mark_flushed(&mut self, revision: u64) {
        if let Some((&saved_revision, ai)) = self
            .config_snapshots
            .iter()
            .filter(|(pending, _)| **pending <= revision && **pending >= self.saved_ai_revision)
            .max_by_key(|(pending, _)| **pending)
        {
            self.saved_ai = ai.clone();
            self.saved_ai_revision = saved_revision;
        }
        self.config_snapshots
            .retain(|pending, _| *pending > revision);
        self.errors
            .retain(|_, (failed_revision, _)| *failed_revision > revision);
        for kind in [SaveKind::Config, SaveKind::Tasks] {
            let completed = self.completed.entry(kind).or_default();
            *completed = (*completed).max(revision);
        }
    }

    pub fn retry(cx: &mut App) {
        let persistence = cx.global::<Self>();
        let revision = persistence.revision;
        let receipt = persistence.queue.flush();
        cx.spawn(async move |cx| {
            let result = wait_for_save(receipt).await;
            cx.update(|cx| {
                if result.is_ok() {
                    cx.global_mut::<Self>().mark_flushed(revision);
                }
                cx.refresh_windows();
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::bail;
    use serde_json::{Value, json};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    #[test]
    fn restores_backup_and_preserves_damaged_primary() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("data.json");
        save_file(&path, &json!({"tasks": ["original"]}), Format::Json).unwrap();
        save_file(&path, &json!({"tasks": ["new"]}), Format::Json).unwrap();
        fs::write(&path, b"broken").unwrap();

        let loaded = load_file::<Value>(&path, Format::Json).unwrap();
        assert!(loaded.recovered);
        assert_eq!(loaded.value, json!({"tasks": ["original"]}));
        assert_eq!(
            serde_json::from_slice::<Value>(&fs::read(&path).unwrap()).unwrap(),
            loaded.value
        );
        let preserved = fs::read_dir(directory.path())
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("data.json.corrupt-")
            })
            .unwrap();
        assert_eq!(fs::read(preserved.path()).unwrap(), b"broken");
    }

    #[test]
    fn first_save_also_creates_a_recoverable_backup() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("data.json");
        let original = json!({"tasks": ["first task"]});
        save_file(&path, &original, Format::Json).unwrap();
        fs::write(&path, b"damaged").unwrap();

        let loaded = load_file::<Value>(&path, Format::Json).unwrap();
        assert!(loaded.recovered);
        assert_eq!(loaded.value, original);
    }

    #[test]
    fn refuses_to_replace_unrecoverable_data_with_defaults() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("data.json");
        fs::write(&path, b"broken primary").unwrap();
        fs::write(backup_path(&path), b"broken backup").unwrap();
        assert!(load_file::<Value>(&path, Format::Json).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"broken primary");
        assert_eq!(fs::read(backup_path(&path)).unwrap(), b"broken backup");
    }

    #[test]
    fn restores_backup_when_primary_is_missing() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("data.json");
        fs::write(backup_path(&path), br#"{"tasks":["kept"]}"#).unwrap();
        let loaded = load_file::<Value>(&path, Format::Json).unwrap();
        assert!(loaded.recovered);
        assert_eq!(loaded.value, json!({"tasks": ["kept"]}));
        assert!(path.exists());
    }

    #[test]
    fn damaged_primary_never_replaces_valid_backup() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("data.json");
        fs::write(&path, b"broken").unwrap();
        fs::write(backup_path(&path), br#"{"tasks":["backup"]}"#).unwrap();
        save_file(&path, &json!({"tasks": ["current"]}), Format::Json).unwrap();
        assert_eq!(
            fs::read(backup_path(&path)).unwrap(),
            br#"{"tasks":["backup"]}"#
        );
    }

    #[test]
    fn queued_snapshots_finish_in_submission_order() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("data.json");
        let queue = SaveQueue::new().unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let first_path = path.clone();
        let first = queue.enqueue(
            SaveKind::Tasks,
            Box::new(move || {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                save_file(&first_path, &json!({"revision": 1}), Format::Json)
            }),
        );
        started_rx.recv().unwrap();
        let second_path = path.clone();
        let second = queue.enqueue(
            SaveKind::Tasks,
            Box::new(move || save_file(&second_path, &json!({"revision": 2}), Format::Json)),
        );
        release_tx.send(()).unwrap();
        futures::executor::block_on(wait_for_save(queue.flush())).unwrap();
        futures::executor::block_on(wait_for_save(first)).unwrap();
        futures::executor::block_on(wait_for_save(second)).unwrap();
        assert_eq!(
            load_file::<Value>(&path, Format::Json).unwrap().value,
            json!({"revision": 2})
        );
    }

    #[test]
    fn failed_save_is_reported_and_can_be_retried() {
        let queue = SaveQueue::new().unwrap();
        let can_write = Arc::new(AtomicBool::new(false));
        let flag = can_write.clone();
        let receipt = queue.enqueue(
            SaveKind::Config,
            Box::new(move || {
                if !flag.load(Ordering::SeqCst) {
                    bail!("Disk unavailable");
                }
                Ok(())
            }),
        );
        assert!(futures::executor::block_on(wait_for_save(receipt)).is_err());
        assert!(futures::executor::block_on(wait_for_save(queue.flush())).is_err());
        can_write.store(true, Ordering::SeqCst);
        futures::executor::block_on(wait_for_save(queue.flush())).unwrap();
    }

    #[test]
    fn retries_only_the_latest_failed_snapshot_for_each_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("data.json");
        let queue = SaveQueue::new().unwrap();
        let can_write = Arc::new(AtomicBool::new(false));
        for revision in [1, 2] {
            let path = path.clone();
            let can_write = can_write.clone();
            let receipt = queue.enqueue(
                SaveKind::Tasks,
                Box::new(move || {
                    if !can_write.load(Ordering::SeqCst) {
                        bail!("Disk unavailable");
                    }
                    save_file(&path, &json!({"revision": revision}), Format::Json)
                }),
            );
            assert!(futures::executor::block_on(wait_for_save(receipt)).is_err());
        }
        can_write.store(true, Ordering::SeqCst);
        futures::executor::block_on(wait_for_save(queue.flush())).unwrap();
        assert_eq!(
            load_file::<Value>(&path, Format::Json).unwrap().value,
            json!({"revision": 2})
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&fs::read(backup_path(&path)).unwrap()).unwrap(),
            json!({"revision": 2}),
            "The obsolete snapshot must not be retried first"
        );
    }

    #[test]
    fn backup_failure_leaves_the_primary_file_untouched() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("data.json");
        fs::write(&path, br#"{"revision":1}"#).unwrap();
        fs::create_dir(backup_path(&path)).unwrap();

        assert!(save_file(&path, &json!({"revision": 2}), Format::Json).is_err());

        assert_eq!(fs::read(&path).unwrap(), br#"{"revision":1}"#);
    }

    #[test]
    fn stale_completions_cannot_revert_saved_settings_or_errors() {
        let mut persistence = Persistence::new(false, AiSettings::default()).unwrap();
        let older = AiSettings {
            model: "older".into(),
            ..Default::default()
        };
        let newer = AiSettings {
            model: "newer".into(),
            ..Default::default()
        };
        let (old_revision, old_done) = persistence.enqueue_config(older, || Ok(()));
        let (new_revision, new_done) = persistence.enqueue_config(newer.clone(), || Ok(()));
        futures::executor::block_on(wait_for_save(old_done)).unwrap();
        futures::executor::block_on(wait_for_save(new_done)).unwrap();

        persistence.record(SaveKind::Config, new_revision, &Ok(()));
        persistence.record(SaveKind::Config, old_revision, &Err("Stale failure".into()));

        assert_eq!(persistence.saved_ai, newer);
        assert!(!persistence.has_errors());
    }

    #[test]
    fn successful_retry_updates_the_saved_baseline_and_ignores_late_failure() {
        let original = AiSettings::default();
        let mut persistence = Persistence::new(false, original.clone()).unwrap();
        let edited = AiSettings {
            model: "edited".into(),
            ..original.clone()
        };
        let can_write = Arc::new(AtomicBool::new(false));
        let flag = can_write.clone();
        let (revision, receipt) = persistence.enqueue_config(edited.clone(), move || {
            if !flag.load(Ordering::SeqCst) {
                bail!("Disk unavailable");
            }
            Ok(())
        });
        let result = futures::executor::block_on(wait_for_save(receipt));
        persistence.record(SaveKind::Config, revision, &result);
        assert_eq!(persistence.saved_ai, original);
        assert!(persistence.has_errors());

        can_write.store(true, Ordering::SeqCst);
        futures::executor::block_on(wait_for_save(persistence.queue.flush())).unwrap();
        persistence.mark_flushed(revision);
        persistence.record(SaveKind::Config, revision, &Err("Late failure".into()));

        assert_eq!(persistence.saved_ai, edited);
        assert!(!persistence.has_errors());
    }
}
