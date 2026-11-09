//! The transport: one GraphQL request -> `gh api graphql` -> the response's
//! `data`.
//!
//! The request goes to gh as JSON on stdin (`--input -`) rather than as
//! `-f`/`-F` flags: `-F` reads a value beginning with `@` as a file name,
//! which a comment such as "@kris can you look?" would trigger. gh owns
//! authentication; this app never sees a token.

use std::ffi::OsString;
use std::io::Write;
use std::process::{Command, Stdio};
use std::thread;

use serde_json::Value;

use crate::queries::Request;

/// Something that answers GraphQL requests. Blocking: callers run it on a
/// background thread.
pub trait Transport: Send + Sync + 'static {
    /// The response's `data`, or a message saying why there is none.
    fn graphql(&self, request: &Request) -> Result<Value, String>;
}

/// The real transport: the `gh` CLI.
#[derive(Clone, Debug)]
pub struct Gh {
    program: OsString,
}

impl Default for Gh {
    fn default() -> Self {
        Gh::new("gh")
    }
}

impl Gh {
    /// Use `program` in place of `gh` (for tests).
    pub fn new(program: impl Into<OsString>) -> Self {
        Gh {
            program: program.into(),
        }
    }
}

impl Transport for Gh {
    fn graphql(&self, request: &Request) -> Result<Value, String> {
        let body =
            serde_json::to_vec(request).map_err(|e| format!("could not encode request: {e}"))?;
        let mut child = Command::new(&self.program)
            .args(["api", "graphql", "--input", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("could not run gh: {e}"))?;
        // Write from another thread, so a large body cannot deadlock against
        // gh filling its stdout pipe.
        let mut stdin = child.stdin.take().expect("stdin is piped");
        let writer = thread::spawn(move || stdin.write_all(&body));
        let out = child
            .wait_with_output()
            .map_err(|e| format!("gh did not finish: {e}"))?;
        // A write error means gh exited early; its output says why.
        let _ = writer.join();
        parse(out.status.code(), &out.stdout, &out.stderr)
    }
}

/// Turn a finished `gh` process into the response's `data`.
///
/// GraphQL errors win over everything (gh exits non-zero for them too, with a
/// less useful message on stderr); then `data` if gh succeeded; then stderr;
/// then the exit code.
pub fn parse(code: Option<i32>, stdout: &[u8], stderr: &[u8]) -> Result<Value, String> {
    if let Ok(Value::Object(mut body)) = serde_json::from_slice::<Value>(stdout) {
        if let Some(Value::Array(errors)) = body.get("errors")
            && !errors.is_empty()
        {
            let messages: Vec<String> = errors
                .iter()
                .map(|e| match e.get("message").and_then(Value::as_str) {
                    Some(m) => m.to_string(),
                    None => e.to_string(),
                })
                .collect();
            return Err(messages.join("; "));
        }
        if code == Some(0)
            && let Some(data) = body.remove("data").filter(|d| !d.is_null())
        {
            return Ok(data);
        }
    }
    let stderr = String::from_utf8_lossy(stderr).trim().to_string();
    if !stderr.is_empty() {
        return Err(stderr);
    }
    Err(match code {
        Some(c) => format!("gh exited with code {c}"),
        None => "gh was killed by a signal".into(),
    })
}
