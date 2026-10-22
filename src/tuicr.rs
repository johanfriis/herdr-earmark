//! Running tuicr and collecting the review it produced.

use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Command, Stdio};

use serde::Deserialize;

/// The same intro tuicr's own markdown export opens with.
const INTRO: &str = "I reviewed your code and have the following comments. Please address them.";
const SESSION_PREFIX: &str = "tuicr-session: ";

pub struct Run {
    /// Markdown tuicr exported on `:wq` / `y` (empty when the user just quit).
    pub export: String,
    /// Session slug tuicr announces on stderr before entering the TUI.
    pub session: Option<String>,
}

/// Runs tuicr in `repo` on the pane terminal. `--stdout` makes tuicr draw on
/// `/dev/tty` and print its export to stdout, which is captured here.
pub fn run(repo: &Path, scope_args: &[String]) -> Result<Run, String> {
    let mut child = Command::new("tuicr")
        .args(["--stdout", "--no-update-check"])
        .args(scope_args)
        .current_dir(repo)
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => {
                "tuicr is not on herdr's PATH; install it (https://github.com/agavra/tuicr) and restart herdr".to_string()
            }
            _ => format!("cannot start tuicr: {e}"),
        })?;

    // Forward stderr live (errors must stay visible) while picking out the slug.
    let stderr = child.stderr.take().expect("stderr is piped");
    let stderr_reader = std::thread::spawn(move || {
        let mut session = None;
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            match line.strip_prefix(SESSION_PREFIX) {
                Some(slug) => session = Some(slug.trim().to_string()),
                None => eprintln!("{line}"),
            }
        }
        session
    });

    let mut export = String::new();
    child
        .stdout
        .take()
        .expect("stdout is piped")
        .read_to_string(&mut export)
        .map_err(|e| format!("cannot read tuicr output: {e}"))?;
    let status = child
        .wait()
        .map_err(|e| format!("tuicr wait failed: {e}"))?;
    let session = stderr_reader.join().unwrap_or(None);
    if !status.success() {
        return Err(format!("tuicr exited with {status}"));
    }
    Ok(Run {
        export: export.trim().to_string(),
        session,
    })
}

#[derive(Deserialize)]
struct Comment {
    location: String,
    #[serde(default)]
    comment_type: String,
    content: String,
}

#[derive(Deserialize)]
struct SessionSummary {
    slug: String,
    #[serde(default)]
    comment_count: usize,
}

/// The session's comments rendered like tuicr's export, for when the user
/// quit without exporting. `None` when the session holds no comments; tuicr
/// deletes such sessions on exit, so absence from the list means empty.
pub fn session_review(repo: &Path, session: &str) -> Result<Option<String>, String> {
    let list = review_cli(repo, &["list", "--repo", "."])?;
    let sessions: Vec<SessionSummary> = serde_json::from_str(&list)
        .map_err(|e| format!("unexpected `tuicr review list` output: {e}"))?;
    if !sessions
        .iter()
        .any(|s| s.slug == session && s.comment_count > 0)
    {
        return Ok(None);
    }
    let json = review_cli(repo, &["comments", "--session", session])?;
    let comments: Vec<Comment> = serde_json::from_str(&json)
        .map_err(|e| format!("unexpected `tuicr review comments` output: {e}"))?;
    Ok(format_comments(&comments))
}

/// Removes the session's comments, keeping its review marks.
pub fn clear_comments(repo: &Path, session: &str) -> Result<(), String> {
    review_cli(repo, &["clearc", "--session", session]).map(drop)
}

fn review_cli(repo: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("tuicr")
        .arg("review")
        .args(args)
        .current_dir(repo)
        .output()
        .map_err(|e| format!("cannot run tuicr review: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "tuicr review {} failed: {}",
            args[0],
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn format_comments(comments: &[Comment]) -> Option<String> {
    if comments.is_empty() {
        return None;
    }
    let mut md = format!("{INTRO}\n\n");
    for (i, c) in comments.iter().enumerate() {
        let marker = format!("{}.", i + 1);
        let indent = " ".repeat(marker.len() + 1);
        let kind = match c.comment_type.as_str() {
            "" | "none" => String::new(),
            t => format!("**[{}]** ", t.to_ascii_uppercase()),
        };
        let mut lines = c.content.lines();
        let first = lines.next().unwrap_or_default();
        md.push_str(&format!("{marker} {kind}`{}` - {first}\n", c.location));
        for line in lines {
            md.push_str(&format!("{indent}{line}\n"));
        }
    }
    Some(md.trim_end().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_like_tuicr_export() {
        let comments: Vec<Comment> = serde_json::from_str(
            r#"[
              {"location":"src/a.rs:42","comment_type":"issue","content":"Magic number"},
              {"location":"src/a.rs","comment_type":"none","content":"line one\nline two"}
            ]"#,
        )
        .unwrap();
        assert_eq!(
            format_comments(&comments).unwrap(),
            format!(
                "{INTRO}\n\n1. **[ISSUE]** `src/a.rs:42` - Magic number\n2. `src/a.rs` - line one\n   line two"
            )
        );
        assert_eq!(format_comments(&[]), None);
    }
}
