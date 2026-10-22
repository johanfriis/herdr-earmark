//! Calls back into herdr: the CLI via `HERDR_BIN_PATH`, and the raw socket for
//! `pane.send_input`, which the CLI does not expose.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::Command;
use std::time::Duration;

use serde_json::{Value, json};

fn bin() -> String {
    std::env::var("HERDR_BIN_PATH")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "herdr".to_string())
}

/// Runs `herdr <args>` and returns stdout.
pub fn cli(args: &[&str]) -> Result<String, String> {
    let bin = bin();
    let out = Command::new(&bin)
        .args(args)
        .output()
        .map_err(|e| format!("cannot run {bin}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "herdr {} failed: {}",
            args.first().copied().unwrap_or_default(),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn notify(title: &str, body: &str) {
    let _ = cli(&["notification", "show", title, "--body", body]);
}

pub fn rename_tab(tab_id: &str, label: &str) -> Result<(), String> {
    cli(&["tab", "rename", tab_id, label]).map(drop)
}

pub fn focus_tab(tab_id: &str) -> Result<(), String> {
    cli(&["tab", "focus", tab_id]).map(drop)
}

/// Types `text` into a pane's input without pressing Enter. Unlike
/// `pane.send_text`, `pane.send_input` wraps the text in a bracketed paste
/// when the target application enabled that mode, so embedded newlines do not
/// submit the prompt.
pub fn paste_into_pane(pane_id: &str, text: &str) -> Result<(), String> {
    let request = json!({
        "id": format!("herdr-earmark:{}", std::process::id()),
        "method": "pane.send_input",
        "params": { "pane_id": pane_id, "text": text, "keys": [] },
    });
    let reply = socket_request(&request)?;
    match reply.get("error") {
        Some(err) => Err(format!(
            "herdr rejected the paste into {pane_id}: {}",
            err.get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown error")
        )),
        None => Ok(()),
    }
}

fn socket_request(request: &Value) -> Result<Value, String> {
    let path = std::env::var("HERDR_SOCKET_PATH")
        .map_err(|_| "HERDR_SOCKET_PATH is not set".to_string())?;
    let mut stream =
        UnixStream::connect(&path).map_err(|e| format!("cannot connect to {path}: {e}"))?;
    let timeout = Some(Duration::from_secs(10));
    stream
        .set_read_timeout(timeout)
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(timeout)
        .map_err(|e| e.to_string())?;
    let mut line = request.to_string();
    line.push('\n');
    stream
        .write_all(line.as_bytes())
        .map_err(|e| format!("herdr socket write failed: {e}"))?;
    let mut reply = String::new();
    BufReader::new(stream)
        .read_line(&mut reply)
        .map_err(|e| format!("herdr socket read failed: {e}"))?;
    serde_json::from_str(&reply).map_err(|e| format!("invalid herdr reply {reply:?}: {e}"))
}
