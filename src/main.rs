//! herdr-earmark: review repository changes in tuicr in a new herdr tab, then
//! paste the review into the calling pane's input without submitting it.
//!
//! One binary, two roles picked from herdr's environment:
//! - action (`HERDR_PLUGIN_ACTION_ID`): resolve the repo and caller pane, open
//!   the `review` plugin pane in a new tab;
//! - pane (`HERDR_PLUGIN_ENTRYPOINT_ID=review`): run tuicr, deliver the review.

mod config;
mod git;
mod herdr;
mod tuicr;

use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::Value;

use config::Config;
use git::Repo;

const ENV_REPO: &str = "HERDR_EARMARK_REPO";
const ENV_CALLER_PANE: &str = "HERDR_EARMARK_CALLER_PANE";
const ENV_CALLER_TAB: &str = "HERDR_EARMARK_CALLER_TAB";

fn main() {
    if env("HERDR_PLUGIN_ENTRYPOINT_ID").as_deref() == Some("review") {
        review_pane();
    } else if env("HERDR_PLUGIN_ACTION_ID").is_some() {
        if let Err(e) = action() {
            eprintln!("herdr-earmark: {e}");
            herdr::notify("Review failed", &e);
            std::process::exit(1);
        }
    } else {
        eprintln!("herdr-earmark is a herdr plugin; bind or invoke `herdr-earmark.open`");
        std::process::exit(2);
    }
}

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.trim().is_empty())
}

/// Action: find the repo behind the focused pane and open the review tab.
fn action() -> Result<(), String> {
    let ctx: Value = env("HERDR_PLUGIN_CONTEXT_JSON")
        .map(|json| serde_json::from_str(&json))
        .transpose()
        .map_err(|e| format!("invalid HERDR_PLUGIN_CONTEXT_JSON: {e}"))?
        .unwrap_or(Value::Null);
    let field = |path: &[&str]| -> Option<String> {
        let mut v = &ctx;
        for key in path {
            v = v.get(key)?;
        }
        v.as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };

    let dir = field(&["focused_pane_cwd"])
        .or_else(|| field(&["worktree", "checkout_path"]))
        .or_else(|| field(&["workspace_cwd"]))
        .ok_or("no working directory in the plugin context")?;
    let repo = Repo::discover(Path::new(&dir))?;
    let root = repo.root().to_string_lossy().into_owned();

    let plugin = env("HERDR_PLUGIN_ID").unwrap_or_else(|| "herdr-earmark".into());
    let mut args = vec![
        "plugin".to_string(),
        "pane".into(),
        "open".into(),
        "--plugin".into(),
        plugin,
        "--entrypoint".into(),
        "review".into(),
        "--placement".into(),
        "tab".into(),
        "--cwd".into(),
        root.clone(),
        "--env".into(),
        format!("{ENV_REPO}={root}"),
        "--focus".into(),
    ];
    if let Some(ws) = field(&["workspace_id"]).or_else(|| env("HERDR_WORKSPACE_ID")) {
        args.extend(["--workspace".into(), ws]);
    }
    if let Some(pane) = field(&["focused_pane_id"]).or_else(|| env("HERDR_PANE_ID")) {
        args.extend(["--env".into(), format!("{ENV_CALLER_PANE}={pane}")]);
    }
    if let Some(tab) = field(&["tab_id"]).or_else(|| env("HERDR_TAB_ID")) {
        args.extend(["--env".into(), format!("{ENV_CALLER_TAB}={tab}")]);
    }
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    herdr::cli(&args).map(drop)
}

/// Review pane: run tuicr and hand the result to the caller. Errors are shown
/// in the pane until Enter, since the pane closes as soon as this exits.
fn review_pane() {
    let cfg = Config::load(env("HERDR_PLUGIN_CONFIG_DIR").as_deref().map(Path::new));
    if let Some(tab) = env("HERDR_TAB_ID")
        && let Err(e) = herdr::rename_tab(&tab, &cfg.tab_name)
    {
        eprintln!("herdr-earmark: {e}");
    }
    match review(&cfg) {
        Ok(Outcome::Delivered) | Ok(Outcome::NoComments) => return_to_caller(),
        Ok(Outcome::Hold(msg)) => hold(&msg),
        Err(e) => hold(&format!("error: {e}")),
    }
}

enum Outcome {
    Delivered,
    NoComments,
    /// Finished, but the message needs to be read before the pane closes.
    Hold(String),
}

fn review(cfg: &Config) -> Result<Outcome, String> {
    let dir = env(ENV_REPO)
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .ok_or("cannot determine the repository directory")?;
    let repo = Repo::discover(&dir)?;
    let scope = repo.scope(cfg.base_branch.as_deref());
    let Some(scope_args) = scope.tuicr_args() else {
        return Ok(Outcome::Hold(format!(
            "nothing to review: {}",
            scope.describe()
        )));
    };
    println!("herdr-earmark: reviewing {}", scope.describe());

    let run = tuicr::run(repo.root(), &scope_args)?;
    let review = match (&run.export, &run.session) {
        (export, _) if !export.is_empty() => Some(export.clone()),
        (_, Some(session)) => tuicr::session_review(repo.root(), session)?,
        (_, None) => None,
    };
    let Some(review) = review else {
        return Ok(Outcome::NoComments);
    };

    let saved = save_review(&review);
    let Some(caller) = env(ENV_CALLER_PANE) else {
        return Ok(Outcome::Hold(format!(
            "no calling pane to paste into; review saved to {}",
            saved.as_deref().unwrap_or("<not saved>")
        )));
    };
    if let Err(e) = herdr::paste_into_pane(&caller, &review) {
        return Err(format!(
            "{e}; review saved to {}",
            saved.as_deref().unwrap_or("<not saved>")
        ));
    }
    if cfg.clear_after_send
        && let Some(session) = &run.session
        && let Err(e) = tuicr::clear_comments(repo.root(), session)
    {
        eprintln!("herdr-earmark: {e}");
    }
    Ok(Outcome::Delivered)
}

/// Keeps the latest review at `<state dir>/last-review.md` so a failed or
/// accidentally discarded paste can be recovered.
fn save_review(review: &str) -> Option<String> {
    let path = PathBuf::from(env("HERDR_PLUGIN_STATE_DIR")?).join("last-review.md");
    match std::fs::write(&path, review) {
        Ok(()) => Some(path.to_string_lossy().into_owned()),
        Err(e) => {
            eprintln!("herdr-earmark: cannot save {}: {e}", path.display());
            None
        }
    }
}

fn return_to_caller() {
    if let Some(tab) = env(ENV_CALLER_TAB)
        && let Err(e) = herdr::focus_tab(&tab)
    {
        eprintln!("herdr-earmark: {e}");
    }
}

fn hold(message: &str) {
    println!("herdr-earmark: {message}");
    print!("press Enter to close");
    let _ = std::io::stdout().flush();
    let _ = std::io::stdin().read_line(&mut String::new());
}
