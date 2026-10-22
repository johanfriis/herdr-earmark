# herdr-earmark

herdr plugin that opens your repository's changes in [tuicr](https://github.com/agavra/tuicr) and, when you quit, pastes your review into the agent pane you launched it from, **without submitting it**. You earmark the lines; the agent gets the notes.

## Flow

1. The `herdr-earmark.open` action takes the focused pane's directory, resolves the git repository, and opens a new tab (named by `tab_name`) running tuicr.
2. The scope comes from read-only git probes:

   | Situation                               | tuicr shows                                           |
   | --------------------------------------- | ----------------------------------------------------- |
   | On the base branch (e.g. `main`)        | uncommitted changes (`-w`)                            |
   | On another branch / detached HEAD       | commits since the fork point from the base + uncommitted changes (`-r <merge-base>..HEAD -w`) |
   | Nothing in scope                        | `nothing to review`, the tab waits for Enter          |

   Uncommitted includes staged, unstaged, and untracked files. The fork point is computed against both the local base branch and `origin/<base>`, so a stale local `main` does not pull in already-merged commits.
3. When tuicr exits, the review is pasted into the calling pane as a bracketed paste (no Enter), focus returns to the calling tab, and the review tab closes.
   - `:wq` or `y` — tuicr's own markdown export is pasted.
   - `:q` with comments — the comments are read from the saved tuicr session and pasted in the same format.
   - `:q` without comments — nothing is pasted.

   After a successful paste, the delivered comments are cleared from the tuicr session (review marks are kept) so the next review does not paste them again. The latest review is also kept at `<plugin state dir>/last-review.md`.

## Install

Requires herdr >= 0.9.5, a Rust toolchain (1.88+, the plugin builds on install), and `tuicr` on herdr's `PATH`.

```sh
herdr plugin install johanfriis/herdr-earmark            # latest main
herdr plugin install johanfriis/herdr-earmark --ref v0.1.0  # a release tag
herdr plugin update herdr-earmark                         # refetch the installed ref
```

For local development, build and link a checkout instead:

```sh
cargo build --release
herdr plugin link /path/to/herdr-earmark
```

Bind a key in `~/.config/herdr/config.toml`:

```toml
[[keys.command]]
key = "prefix+r"
type = "plugin_action"
command = "herdr-earmark.open"
description = "review changes in tuicr"
```

## Configuration

`config.toml` in `herdr plugin config-dir herdr-earmark` (all keys optional):

```toml
tab_name = "review"        # label of the review tab
base_branch = "main"       # default: origin/HEAD, then main, then master
clear_after_send = true    # clear delivered comments from the tuicr session
```

A missing or invalid config falls back to the defaults (invalid files print a warning).

tuicr's own settings (theme, export intro, `session_header`, comment types) live in tuicr's config and apply to the export.

## Releasing

Versions live in both `Cargo.toml` and `herdr-plugin.toml`; CI fails if they differ.

```sh
make release VERSION=x.y.z
```

From a clean `main`, this bumps both files and `Cargo.lock`, runs `make check`, commits, tags `vx.y.z`, and pushes. The tag triggers `.github/workflows/release.yml`, which checks that the tag matches both versions, runs CI, and publishes a GitHub release with generated notes. Users get the release with `herdr plugin install ... --ref vx.y.z`; herdr builds it from source.
