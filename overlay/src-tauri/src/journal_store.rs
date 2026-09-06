//! Serialized, coalesced persistence for the local decision journal.
use recall_core::journal::Journal;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::watch;

fn next_file_nonce() -> u64 {
    static NONCE: AtomicU64 = AtomicU64::new(1);
    NONCE.fetch_add(1, Ordering::Relaxed)
}

/// At most one pending bounded Journal clone; newer snapshots replace older pending ones.
pub struct JournalSink {
    sender: watch::Sender<Option<Journal>>,
    pub writable: bool,
    pub warning: Option<String>,
}

impl JournalSink {
    pub fn queue(&self, journal: Journal) {
        if self.writable {
            self.sender.send_replace(Some(journal));
        }
    }
}

pub struct JournalWriter {
    receiver: watch::Receiver<Option<Journal>>,
    path: PathBuf,
    warning: Option<String>,
}

fn preserve_unreadable(path: &Path) -> std::io::Result<PathBuf> {
    if !fs::symlink_metadata(path)?.file_type().is_file() {
        return Err(std::io::Error::other("journal path is not a regular file"));
    }
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    for _ in 0..32 {
        let backup = path.with_file_name(format!(
            "{name}.unreadable-{}-{}.json",
            std::process::id(),
            next_file_nonce()
        ));
        // Reserve an exact unused target first; never replace an earlier backup.
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&backup)
        {
            Ok(file) => {
                drop(file);
                if let Err(error) = fs::rename(path, &backup) {
                    let _ = fs::remove_file(&backup); // Only the empty file created just above.
                    return Err(error);
                }
                return Ok(backup);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::other(
        "could not reserve a unique journal backup",
    ))
}

/// Startup-only I/O. A failed load can never silently overwrite the unreadable original.
pub fn open(path: PathBuf) -> (Journal, JournalSink, JournalWriter) {
    let (journal, writable, warning) = match Journal::load(&path) {
        Ok(journal) => (journal, true, None),
        Err(error) => match preserve_unreadable(&path) {
            Ok(backup) => (Journal::default(), true, Some(format!("Previous journal was unreadable ({error}); preserved at {}", backup.display()))),
            Err(backup_error) => (Journal::default(), false, Some(format!("Journal could not be loaded ({error}) or preserved ({backup_error}). Saving is disabled; the original was not replaced."))),
        },
    };
    let (sender, receiver) = watch::channel(None);
    let sink = JournalSink {
        sender,
        writable,
        warning: warning.clone(),
    };
    let writer = JournalWriter {
        receiver,
        path,
        warning,
    };
    (journal, sink, writer)
}

/// Demo data must never modify or recover the user's real journal.
pub fn disabled() -> (Journal, JournalSink, JournalWriter) {
    let (sender, receiver) = watch::channel(None);
    (
        Journal::default(),
        JournalSink {
            sender,
            writable: false,
            warning: None,
        },
        JournalWriter {
            receiver,
            path: PathBuf::new(),
            warning: None,
        },
    )
}

impl JournalWriter {
    /// One writer awaits each disk commit, so an older slow write cannot overwrite a newer one.
    /// Failures retain the latest snapshot and retry outside the polling and UI threads.
    pub async fn run(mut self, report: impl Fn(Option<String>) + Send + 'static) {
        while self.receiver.changed().await.is_ok() {
            let Some(mut latest) = self.receiver.borrow_and_update().clone() else {
                continue;
            };
            loop {
                let path = self.path.clone();
                let snapshot = latest.clone();
                let saved = tokio::task::spawn_blocking(move || {
                    snapshot.save(&path).map_err(|error| error.to_string())
                })
                .await;
                match saved {
                    Ok(Ok(())) => {
                        report(self.warning.clone());
                        break;
                    }
                    result => {
                        let error = match result {
                            Ok(Err(error)) => error,
                            Err(error) => error.to_string(),
                            Ok(Ok(())) => unreachable!(),
                        };
                        let warning = self
                            .warning
                            .as_ref()
                            .map(|warning| format!("{warning}. "))
                            .unwrap_or_default();
                        report(Some(format!(
                            "{warning}Journal is not saved yet: {error}. Retrying locally."
                        )));
                        tokio::select! {
                            changed = self.receiver.changed() => {
                                if changed.is_err() { return; }
                                if let Some(newer) = self.receiver.borrow_and_update().clone() { latest = newer; }
                            }
                            _ = tokio::time::sleep(Duration::from_secs(5)) => {}
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use recall_core::journal::Journal;
    use std::io::Write;

    fn test_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "recall-journal-store-{}-{}",
            std::process::id(),
            next_file_nonce()
        ));
        std::fs::create_dir(&dir).unwrap();
        dir
    }

    #[test]
    fn a_corrupt_journal_is_preserved_before_new_writes_are_allowed() {
        let dir = test_dir();
        let path = dir.join("decisions.json");
        std::fs::File::create(&path)
            .unwrap()
            .write_all(b"not a journal")
            .unwrap();
        let (_, sink, _) = open(path.clone());
        assert!(sink.writable);
        assert!(sink
            .warning
            .as_ref()
            .is_some_and(|message| message.contains("preserved")));
        assert!(!path.exists());
        let backups: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(std::fs::read(&backups[0]).unwrap(), b"not a journal");
        std::fs::remove_file(&backups[0]).unwrap();
        std::fs::remove_dir(&dir).unwrap();
    }

    #[test]
    fn a_non_file_target_disables_persistence_without_moving_user_data() {
        let dir = test_dir();
        let path = dir.join("decisions.json");
        std::fs::create_dir(&path).unwrap();
        let (_, sink, _) = open(path.clone());
        assert!(!sink.writable);
        assert!(sink.warning.is_some());
        assert!(path.is_dir());
        std::fs::remove_dir(&path).unwrap();
        std::fs::remove_dir(&dir).unwrap();
    }

    #[tokio::test]
    async fn burst_updates_coalesce_and_the_writer_commits_the_latest_journal() {
        let dir = test_dir();
        let path = dir.join("decisions.json");
        let (_, sink, worker) = open(path.clone());
        let mut journal = Journal::default();
        journal.begin("first".into(), "Xayah".into(), None, "16.17".into(), None);
        journal.finish();
        sink.queue(journal.clone());
        journal.begin("second".into(), "Lux".into(), None, "16.17".into(), None);
        journal.finish();
        sink.queue(journal);
        let (done, mut received) = tokio::sync::mpsc::unbounded_channel();
        let task = tokio::spawn(worker.run(move |error| {
            let _ = done.send(error);
        }));
        let status = tokio::time::timeout(std::time::Duration::from_secs(2), received.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(status.is_none());
        assert_eq!(
            Journal::load(&path).unwrap().recap().unwrap().session_id,
            "second"
        );
        drop(sink);
        task.await.unwrap();
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&dir).unwrap();
    }
}
