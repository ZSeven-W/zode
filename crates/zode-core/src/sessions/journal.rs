use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use fs4::fs_std::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::CoreError;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalEntry {
    pub schema_version: u32,
    pub seq: u64,
    pub timestamp_ms: u64,
    /// Logical predecessor. Unlike `seq`, this follows rewinds and lets
    /// readers reconstruct the active branch without deleting audit history.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_seq: Option<u64>,
    pub kind: String,
    pub payload: Value,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalHead {
    pub next_seq: u64,
    pub logical_head: Option<u64>,
    /// Byte length of the valid `events.jsonl` prefix when this head was
    /// written. Lets `append` trust the head without re-parsing the journal;
    /// any mismatch (torn tail, external edit, pre-field head) forces repair.
    #[serde(default)]
    pub journal_len: u64,
}

pub struct SessionJournal {
    dir: PathBuf,
}

impl SessionJournal {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn append(
        &self,
        kind: impl Into<String>,
        payload: Value,
    ) -> Result<JournalEntry, CoreError> {
        std::fs::create_dir_all(&self.dir)?;
        let lock = open_lock(&self.dir.join("session.lock"))?;
        FileExt::lock_exclusive(&lock)?;
        let mut head = self.load_or_repair_head_unlocked()?;
        let entry = JournalEntry {
            schema_version: 1,
            seq: head.next_seq,
            timestamp_ms: now_ms(),
            parent_seq: head.logical_head,
            kind: kind.into(),
            payload,
        };
        let mut journal = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.join("events.jsonl"))?;
        let mut line = serde_json::to_vec(&entry)?;
        line.push(b'\n');
        journal.write_all(&line)?;
        journal.sync_all()?;
        head.next_seq = head.next_seq.saturating_add(1);
        head.logical_head = Some(entry.seq);
        head.journal_len = head.journal_len.saturating_add(line.len() as u64);
        write_json_atomic(&self.dir.join("head.json"), &head)?;
        Ok(entry)
    }

    pub fn set_logical_head(&self, seq: u64) -> Result<(), CoreError> {
        std::fs::create_dir_all(&self.dir)?;
        let lock = open_lock(&self.dir.join("session.lock"))?;
        FileExt::lock_exclusive(&lock)?;
        let mut head = self.load_or_repair_head_unlocked()?;
        if seq >= head.next_seq {
            return Err(CoreError::Other(format!(
                "logical head {seq} is past journal tail {}",
                head.next_seq.saturating_sub(1)
            )));
        }
        head.logical_head = Some(seq);
        write_json_atomic(&self.dir.join("head.json"), &head)?;
        Ok(())
    }

    pub fn head(&self) -> Result<JournalHead, CoreError> {
        std::fs::create_dir_all(&self.dir)?;
        let lock = open_lock(&self.dir.join("session.lock"))?;
        FileExt::lock_shared(&lock)?;
        self.load_or_repair_head_unlocked()
    }

    pub fn read_all(&self) -> Result<Vec<JournalEntry>, CoreError> {
        Ok(scan_entries(&self.dir.join("events.jsonl"))?.0)
    }

    /// Load `head.json`, trusting it when its recorded byte length matches
    /// `events.jsonl` (the fast path — no journal re-parse). Any mismatch
    /// rebuilds the head from the valid journal prefix and truncates a torn
    /// tail so the next append cannot fuse with a half-written line.
    fn load_or_repair_head_unlocked(&self) -> Result<JournalHead, CoreError> {
        let path = self.dir.join("head.json");
        let events = self.dir.join("events.jsonl");
        let file_len = std::fs::metadata(&events)
            .map(|meta| meta.len())
            .unwrap_or(0);
        if let Ok(raw) = std::fs::read(&path) {
            if let Ok(head) = serde_json::from_slice::<JournalHead>(&raw) {
                if head.journal_len == file_len
                    && (head.next_seq == 0) == (head.journal_len == 0)
                    && head
                        .logical_head
                        .is_none_or(|logical| logical < head.next_seq)
                {
                    return Ok(head);
                }
            }
        }
        let (entries, valid_len) = scan_entries(&events)?;
        if file_len > valid_len {
            let file = OpenOptions::new().write(true).open(&events)?;
            file.set_len(valid_len)?;
            file.sync_all()?;
        }
        let next_seq = entries
            .last()
            .map(|entry| entry.seq.saturating_add(1))
            .unwrap_or(0);
        let head = JournalHead {
            next_seq,
            logical_head: entries.last().map(|entry| entry.seq),
            journal_len: valid_len,
        };
        write_json_atomic(&path, &head)?;
        Ok(head)
    }
}

/// Parse the journal, returning the entries of the valid prefix and that
/// prefix's byte length. An unterminated or unparsable tail is excluded (the
/// repair path truncates it).
fn scan_entries(path: &Path) -> Result<(Vec<JournalEntry>, u64), CoreError> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok((Vec::new(), 0)),
        Err(error) => return Err(error.into()),
    };
    let mut reader = BufReader::new(file);
    let mut entries = Vec::new();
    let mut valid_len: u64 = 0;
    let mut buf = Vec::new();
    loop {
        buf.clear();
        let read = reader.read_until(b'\n', &mut buf)?;
        if read == 0 {
            break;
        }
        if buf.last() != Some(&b'\n') {
            tracing::warn!(path = %path.display(), "ignoring unterminated journal tail");
            break;
        }
        let line = String::from_utf8_lossy(&buf);
        let trimmed = line.trim();
        if trimmed.is_empty() {
            valid_len += read as u64;
            continue;
        }
        match serde_json::from_str(trimmed) {
            Ok(entry) => {
                entries.push(entry);
                valid_len += read as u64;
            }
            Err(error) => {
                tracing::warn!(path = %path.display(), "ignoring incomplete journal tail: {error}");
                break;
            }
        }
    }
    Ok((entries, valid_len))
}

fn open_lock(path: &Path) -> Result<File, CoreError> {
    Ok(OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?)
}

pub(crate) fn write_json_atomic(path: &Path, value: &impl Serialize) -> Result<(), CoreError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4().simple()));
    let bytes = serde_json::to_vec_pretty(value)?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    if let Err(error) = replace_file(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(error.into());
    }
    Ok(())
}

fn replace_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    if destination.exists() {
        // `std::fs::rename` cannot replace an existing file on Windows. The
        // staged file is already durable, so fall back to remove + rename.
        std::fs::remove_file(destination)?;
    }
    std::fs::rename(source, destination)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_monotonic_records_and_repairs_head() {
        let dir = tempfile::tempdir().unwrap();
        let journal = SessionJournal::new(dir.path().to_path_buf());
        assert_eq!(journal.append("one", serde_json::json!({})).unwrap().seq, 0);
        let second = journal.append("two", serde_json::json!({})).unwrap();
        assert_eq!(second.seq, 1);
        assert_eq!(second.parent_seq, Some(0));
        std::fs::write(dir.path().join("head.json"), b"{broken").unwrap();
        let head = journal.head().unwrap();
        assert_eq!(head.next_seq, 2);
        assert_eq!(head.logical_head, Some(1));
    }

    #[test]
    fn ignores_an_incomplete_tail_without_losing_prior_records() {
        let dir = tempfile::tempdir().unwrap();
        let journal = SessionJournal::new(dir.path().to_path_buf());
        journal.append("one", serde_json::json!({})).unwrap();
        let mut file = OpenOptions::new()
            .append(true)
            .open(dir.path().join("events.jsonl"))
            .unwrap();
        file.write_all(b"{partial").unwrap();
        let entries = journal.read_all().unwrap();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn append_after_a_torn_tail_truncates_it_instead_of_corrupting_the_next_entry() {
        let dir = tempfile::tempdir().unwrap();
        let journal = SessionJournal::new(dir.path().to_path_buf());
        journal.append("one", serde_json::json!({})).unwrap();
        journal.append("two", serde_json::json!({})).unwrap();
        let mut file = OpenOptions::new()
            .append(true)
            .open(dir.path().join("events.jsonl"))
            .unwrap();
        file.write_all(b"{partial").unwrap();
        drop(file);
        let third = journal.append("three", serde_json::json!({})).unwrap();
        assert_eq!(third.seq, 2);
        let entries = journal.read_all().unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[2].kind, "three");
    }

    #[test]
    fn a_valid_head_is_trusted_without_reparsing_the_journal() {
        let dir = tempfile::tempdir().unwrap();
        let journal = SessionJournal::new(dir.path().to_path_buf());
        journal.append("one", serde_json::json!({})).unwrap();
        journal.append("two", serde_json::json!({})).unwrap();
        // If append trusts the recorded head, breaking a middle entry's JSON
        // (same byte length) is invisible to it: the next seq must come from
        // head.json alone, not from re-parsing the journal.
        let events = dir.path().join("events.jsonl");
        let mut raw = std::fs::read(&events).unwrap();
        raw[0] = b'[';
        std::fs::write(&events, raw).unwrap();
        let third = journal.append("three", serde_json::json!({})).unwrap();
        assert_eq!(third.seq, 2);
    }

    #[test]
    fn appending_after_rewind_creates_a_new_logical_branch() {
        let dir = tempfile::tempdir().unwrap();
        let journal = SessionJournal::new(dir.path().to_path_buf());
        journal.append("zero", serde_json::json!({})).unwrap();
        journal.append("one", serde_json::json!({})).unwrap();
        journal.set_logical_head(0).unwrap();
        let branch = journal.append("branch", serde_json::json!({})).unwrap();
        assert_eq!(branch.seq, 2);
        assert_eq!(branch.parent_seq, Some(0));
        assert_eq!(journal.head().unwrap().logical_head, Some(2));
    }

    #[test]
    fn empty_journal_does_not_accept_a_stale_head() {
        let dir = tempfile::tempdir().unwrap();
        let journal = SessionJournal::new(dir.path().to_path_buf());
        write_json_atomic(
            &dir.path().join("head.json"),
            &JournalHead {
                next_seq: 1,
                logical_head: Some(0),
                journal_len: 0,
            },
        )
        .unwrap();
        assert_eq!(journal.head().unwrap(), JournalHead::default());
    }
}
