//! Run an editor on a temporary `.md` file and read back what was saved.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// What the editor left: whether it exited successfully, and the file's text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edited {
    pub success: bool,
    pub text: String,
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A fresh file in the temp dir holding `text`. Created exclusively, so an
/// existing file (or a planted symlink) is never written through.
fn temp_file(text: &str) -> io::Result<PathBuf> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("gh-board-{}-{nanos}-{n}.md", std::process::id()));
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?
        .write_all(text.as_bytes())?;
    Ok(path)
}

/// Write `initial` to a temp file, run `command` (program and arguments; the
/// file name is appended) on it with the terminal's stdio, and read the file
/// back. The file is removed afterwards, whatever happens.
pub fn edit(command: &[String], initial: &str) -> io::Result<Edited> {
    let (program, args) = command
        .split_first()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no editor command"))?;
    let path = temp_file(initial)?;
    let result = Command::new(program)
        .args(args)
        .arg(&path)
        .status()
        .and_then(|status| {
            Ok(Edited {
                success: status.success(),
                text: fs::read_to_string(&path)?,
            })
        });
    let _ = fs::remove_file(&path);
    result
}
