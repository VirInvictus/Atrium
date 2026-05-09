// SPDX-License-Identifier: MIT
//! Self-write filter for the vault sync loop (Phase 17, v0.10.0).
//!
//! atrium-org's [`VaultWriter`](crate::vault_writer::VaultWriter)
//! and [`VaultWatcher`](crate::vault_watcher::VaultWatcher) talk to
//! the same files. Without coordination, every DB write would echo
//! back through inotify and trigger a redundant read/diff cycle.
//!
//! [`RecentWrites`] is a small ring-buffer of `(path, mtime)`
//! entries that the writer pushes to after every successful flush
//! and the watcher checks before processing an event. The match is
//! **exact tuple equality** on `(path, mtime)` — not a TTL window
//! on path alone — because external edits within hundreds of
//! milliseconds of an Atrium write are real and must not be
//! suppressed.
//!
//! Why mtime-based instead of TTL-on-path?
//! - A path-only TTL filter swallows external edits that happen
//!   inside the TTL window after an Atrium write. (Empirically:
//!   integration test seeds the file ~150 ms before "external"
//!   edit; with a 500 ms TTL on path alone, the watcher never
//!   processed the edit.)
//! - mtime is set by the OS on every write. Two distinct writes
//!   produce distinct mtimes (Linux ext4 stores nanosecond
//!   precision). Atrium-from-Atrium echoes match exactly; a real
//!   external edit produces a different mtime and falls through.
//!
//! Why still keep a TTL?
//! - Bounds memory under sustained writer activity. Old entries
//!   that no longer correspond to any file mtime are useless.
//! - 2 second TTL is wide enough to cover any plausible
//!   notify-debounce path; entries are also evicted by capacity.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// A bounded TTL set of `(path, mtime_just_written)` pairs.
/// Cheaply cloneable via `Arc<RwLock<RecentWrites>>` from both the
/// writer and the watcher.
#[derive(Debug)]
pub struct RecentWrites {
    entries: VecDeque<Entry>,
    capacity: usize,
    ttl: Duration,
}

#[derive(Debug)]
struct Entry {
    path: PathBuf,
    mtime: SystemTime,
    recorded_at: Instant,
}

impl RecentWrites {
    /// Default capacity (32 entries) and TTL (2 seconds). The
    /// match is by `(path, mtime)`, not TTL alone — the TTL is
    /// just a memory bound.
    pub fn new() -> Self {
        Self::with_capacity_and_ttl(32, Duration::from_secs(2))
    }

    pub fn with_capacity_and_ttl(capacity: usize, ttl: Duration) -> Self {
        Self {
            entries: VecDeque::with_capacity(capacity),
            capacity,
            ttl,
        }
    }

    /// Record that Atrium itself wrote `path`, picking up the
    /// file's current mtime via `fs::metadata`. The writer's
    /// flush path calls this on every successful emit. Returns
    /// `false` if the metadata call failed (path doesn't exist,
    /// permission denied) — the writer logs but otherwise
    /// proceeds; missing entries just mean the next inotify
    /// event won't be suppressed.
    pub fn record(&mut self, path: PathBuf) -> bool {
        let mtime = match std::fs::metadata(&path).and_then(|m| m.modified()) {
            Ok(m) => m,
            Err(_) => return false,
        };
        self.record_with_mtime(path, mtime);
        true
    }

    /// Variant for tests / hot paths that already have the mtime
    /// in hand. Production code should prefer [`record`].
    pub fn record_with_mtime(&mut self, path: PathBuf, mtime: SystemTime) {
        let now = Instant::now();
        self.evict_expired(now);
        if self.entries.len() >= self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back(Entry {
            path,
            mtime,
            recorded_at: now,
        });
    }

    /// True if Atrium recently wrote `path` at exactly `mtime`.
    /// External edits produce a different mtime and fall through
    /// even if Atrium wrote the same file moments earlier.
    pub fn is_self_write(&self, path: &Path, mtime: SystemTime) -> bool {
        let now = Instant::now();
        self.entries.iter().any(|e| {
            e.path == path && e.mtime == mtime && now.duration_since(e.recorded_at) <= self.ttl
        })
    }

    fn evict_expired(&mut self, now: Instant) {
        while let Some(front) = self.entries.front() {
            if now.duration_since(front.recorded_at) > self.ttl {
                self.entries.pop_front();
            } else {
                break;
            }
        }
    }
}

impl Default for RecentWrites {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    fn epoch_plus(ms: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_millis(ms)
    }

    #[test]
    fn matches_path_and_mtime() {
        let mut r = RecentWrites::new();
        let p = PathBuf::from("/tmp/atrium-test/Project.org");
        let m = epoch_plus(1_000);
        assert!(!r.is_self_write(&p, m));
        r.record_with_mtime(p.clone(), m);
        assert!(r.is_self_write(&p, m));
    }

    #[test]
    fn different_mtime_falls_through() {
        // The whole point: an external edit moments after Atrium's
        // own write produces a fresh mtime and must NOT be
        // classified as a self-write.
        let mut r = RecentWrites::new();
        let p = PathBuf::from("/tmp/atrium-test/Project.org");
        r.record_with_mtime(p.clone(), epoch_plus(1_000));
        assert!(!r.is_self_write(&p, epoch_plus(1_500)));
    }

    #[test]
    fn entries_expire_after_ttl() {
        let mut r = RecentWrites::with_capacity_and_ttl(8, Duration::from_millis(50));
        let p = PathBuf::from("/tmp/atrium-test/Project.org");
        let m = epoch_plus(1_000);
        r.record_with_mtime(p.clone(), m);
        assert!(r.is_self_write(&p, m));
        sleep(Duration::from_millis(80));
        assert!(!r.is_self_write(&p, m));
    }

    #[test]
    fn evicts_oldest_when_capacity_reached() {
        let mut r = RecentWrites::with_capacity_and_ttl(2, Duration::from_secs(60));
        r.record_with_mtime(PathBuf::from("/tmp/a.org"), epoch_plus(1));
        r.record_with_mtime(PathBuf::from("/tmp/b.org"), epoch_plus(2));
        r.record_with_mtime(PathBuf::from("/tmp/c.org"), epoch_plus(3));
        assert!(!r.is_self_write(Path::new("/tmp/a.org"), epoch_plus(1)));
        assert!(r.is_self_write(Path::new("/tmp/b.org"), epoch_plus(2)));
        assert!(r.is_self_write(Path::new("/tmp/c.org"), epoch_plus(3)));
    }

    #[test]
    fn unrelated_paths_are_not_self_writes() {
        let mut r = RecentWrites::new();
        let m = epoch_plus(1_000);
        r.record_with_mtime(PathBuf::from("/tmp/written.org"), m);
        assert!(!r.is_self_write(Path::new("/tmp/other.org"), m));
    }
}
