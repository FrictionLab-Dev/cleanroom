# Cleanroom

Cleanroom is the Friction Lab tool for safe developer-cache cleanup.

The preferred public command is `clean`.

Cleanroom uses bundled cleanup profiles to describe known artifacts, categories, safety levels, and recommendations. Profiles are descriptive:

- Profiles explain what an artifact is
- Profiles explain why it may be safe or unsafe to clean
- Profiles explain expected impact and recommendations
- Profiles do not execute commands
- Profiles do not delete files
- Profiles do not move files
- Profiles do not bypass confirmation
- Profiles do not override allowed-root validation

Current bundled profile:
- Xcode

## Setup

Install the binary:

```sh
cargo install --path . --force --bin clean
```

Verify the command:

```sh
which clean
clean --help 2>/dev/null || true
```

If Cargo's bin directory is not on your `PATH` yet:

```sh
echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.zshrc
source ~/.zshrc
```

Check required macOS tools:

```sh
command -v osascript
command -v open
```

Check the current Xcode cleanup roots:

```sh
ls -ld "$HOME/Library/Developer/Xcode/DerivedData" 2>/dev/null || echo "DerivedData not found"
ls -ld "$HOME/Library/Developer/Xcode/iOS DeviceSupport" 2>/dev/null || echo "iOS DeviceSupport not found"
ls -ld "$HOME/Library/Developer/Xcode/Archives" 2>/dev/null || echo "Archives not found"
ls -ld "$HOME/Library/Developer/CoreSimulator/Caches" 2>/dev/null || echo "CoreSimulator Caches not found"
```

Check or create the log directory:

```sh
mkdir -p "$HOME/Library/Logs/Friction Lab/Cleanroom"
ls -ld "$HOME/Library/Logs/Friction Lab/Cleanroom"
```

Notes:

- `clean` does not need a shell wrapper. You can run it directly as `clean`.
- Bundled profiles are loaded from the crate and used to annotate scan results in the TUI.
- Cleanup uses macOS Finder via `osascript` to move selected items to Trash.
- Permanent delete is not implemented.
- Cleanup still requires explicit confirmation inside the TUI before anything is moved.
- If macOS prompts for folder access, allow Terminal, iTerm, or your terminal app to access the relevant folders.
- If scanning fails because of permissions, check System Settings -> Privacy & Security -> Files and Folders or Full Disk Access.

Current scope:
- Xcode cleanup only
- `DerivedData`
- `iOS DeviceSupport`
- `Archives`
- `CoreSimulator/Caches`

Current behavior:
- Scans known Xcode cache roots and summarizes size
- Shows one level at a time in the TUI
- Uses profile metadata to explain categories and artifacts in the right-side summary panels
- Users choose what to keep
- Anything not kept becomes a cleanup candidate
- Cleanup requires explicit confirmation
- Default cleanup action moves items to macOS Trash

Safety levels:
- `recommended`: usually a good cleanup candidate when stale
- `rebuildable`: generated caches or outputs that tools can recreate
- `caution`: review before cleaning because the artifact may still be useful
- `protected`: generally avoid cleaning
- `unknown`: unmatched artifact, inspect before cleaning

Profile metadata includes:
- What an artifact or category represents
- Why it may be safe or unsafe to clean
- Expected impact after cleaning
- A recommendation for review or cleanup

Safety notes:
- Permanent delete is not implemented
- Cleanroom does not use `rm -rf`
- Entries are validated against known Xcode roots before cleanup
- Symlinks are skipped instead of followed blindly
- Cleanup profiles are descriptive only and cannot execute cleanup actions
- Cleanup logs are written to `~/Library/Logs/Friction Lab/Cleanroom/cleanroom.log`

Current development paths:
- Log path: `~/Library/Logs/Friction Lab/Cleanroom/cleanroom.log`
- Stats path: `~/Library/Application Support/Friction Lab/Cleanroom/stats.json`
- If legacy aggregate stats exist under the older PathPilot path, Cleanroom loads them and then writes future updates to the new Friction Lab stats path.

Aggregate stats:
- Stats path: `~/Library/Application Support/Friction Lab/Cleanroom/stats.json`
- Stats are aggregate-only
- Stats track cleanup counts, cleaned bytes, cleaned entries, and aggregate buckets such as `xcode` or `xcode.derivedData`
- Stats do not store full private paths
- Stats are updated only after confirmed cleanup runs with successful Trash moves
- Stats write failures do not block cleanup completion

Optional testing helpers:
- `CLEANROOM_HOME_OVERRIDE` points Xcode scanning at a fake home directory with the same `Library/...` layout
- `CLEANROOM_LOG_PATH` overrides the default log path
- `CLEANROOM_STATS_PATH` overrides the aggregate stats path
