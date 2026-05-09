// SPDX-License-Identifier: MIT
//! Atomic file writes for the Org vault projection (Phase 16).
//!
//! Spec §7.3.3 rule 6: "Every Atrium-side vault write is
//! `write-temp + fsync + rename`, never partial. Crash mid-write
//! leaves the previous version intact."
//!
//! [`write_atomic`] is the single helper every vault writer uses.
//! The temp file lives in the same directory as the destination
//! so the final `rename` is a same-filesystem atomic operation
//! (POSIX guarantees atomic rename within a filesystem; across
//! filesystems it falls back to copy-and-delete which isn't
//! atomic and would defeat the purpose).
//!
//! The temp file is named `<dest>.atrium.tmp` so concurrent
//! writers (Atrium + an unrelated tool that happens to write the
//! same file) don't collide. If two Atrium processes write the
//! same file at the same time the second `rename` clobbers the
//! first, which is the OS guarantee — last-writer-wins. Atrium's
//! single-writer worker discipline ensures this doesn't happen
//! within one process.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;

/// Atomically replace the file at `path` with `contents`.
///
/// Strategy:
/// 1. Open `<path>.atrium.tmp` for write (truncate any leftover).
/// 2. Write the full `contents`.
/// 3. `fsync` the file so the data hits disk before rename.
/// 4. Rename the temp file over `path`. This is atomic on POSIX
///    within a filesystem.
///
/// If any step fails, the temp file is removed (best-effort)
/// and the destination at `path` is untouched. Callers see an
/// `io::Error` and can retry.
///
/// The destination's parent directory must exist; `write_atomic`
/// does not create it. This matches the spec's expectation that
/// the vault layout (Area directories, etc.) is provisioned by
/// the caller before writing project files.
pub fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "write_atomic: path has no parent directory",
        )
    })?;
    let file_name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "write_atomic: path has no file name",
        )
    })?;

    // Same-directory temp file so the final rename is a same-FS
    // atomic operation. The `.atrium.tmp` suffix makes the temp
    // file recognisable if a crash leaves one around.
    let mut temp_name = file_name.to_os_string();
    temp_name.push(".atrium.tmp");
    let temp_path = parent.join(&temp_name);

    let result = (|| -> io::Result<()> {
        let mut file = File::create(&temp_path)?;
        file.write_all(contents)?;
        file.sync_all()?;
        // Drop happens at end-of-scope; we want it explicit here
        // so the OS handle is released before the rename.
        drop(file);
        fs::rename(&temp_path, path)?;
        Ok(())
    })();

    if result.is_err() {
        // Best-effort cleanup. If this fails we still surface the
        // original error; the user can clean up the temp file
        // manually if it persists.
        let _ = fs::remove_file(&temp_path);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "atrium-atomic-test-{}-{}",
            std::process::id(),
            name
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn write_atomic_creates_file_with_contents() {
        let dir = tmp_dir("create");
        let path = dir.join("a.org");
        write_atomic(&path, b"* TODO Hello\n").unwrap();
        let read = fs::read(&path).unwrap();
        assert_eq!(read, b"* TODO Hello\n");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn write_atomic_overwrites_existing_file() {
        let dir = tmp_dir("overwrite");
        let path = dir.join("a.org");
        fs::write(&path, b"OLD CONTENTS").unwrap();
        write_atomic(&path, b"NEW CONTENTS").unwrap();
        let read = fs::read(&path).unwrap();
        assert_eq!(read, b"NEW CONTENTS");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn write_atomic_leaves_no_temp_file_on_success() {
        let dir = tmp_dir("no-temp");
        let path = dir.join("a.org");
        write_atomic(&path, b"ok").unwrap();
        // Walk the directory; nothing besides the destination
        // should remain.
        let entries: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(entries, vec![path.file_name().unwrap().to_os_string()]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn write_atomic_preserves_destination_when_parent_missing() {
        // No tmp_dir setup — the parent doesn't exist. The helper
        // should error rather than silently create directories.
        let path = std::env::temp_dir()
            .join(format!(
                "atrium-atomic-missing-parent-{}",
                std::process::id()
            ))
            .join("nested")
            .join("a.org");
        let result = write_atomic(&path, b"contents");
        assert!(result.is_err(), "missing parent should error");
    }

    #[test]
    fn write_atomic_rejects_path_without_filename() {
        let path = std::path::PathBuf::from("/");
        let result = write_atomic(&path, b"contents");
        assert!(result.is_err(), "root path should error");
    }
}
