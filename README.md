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
ls -ld "$HOME/Library/Developer/Xcode/Archives" 2>/dev/null || echo "Archives not found"
ls -ld "$HOME/Library/Developer/Xcode/iOS DeviceSupport" 2>/dev/null || echo "iOS DeviceSupport not found"
ls -ld "$HOME/Library/Developer/Xcode/watchOS DeviceSupport" 2>/dev/null || echo "watchOS DeviceSupport not found"
ls -ld "$HOME/Library/Developer/Xcode/tvOS DeviceSupport" 2>/dev/null || echo "tvOS DeviceSupport not found"
ls -ld "$HOME/Library/Developer/Xcode/UserData/Previews" 2>/dev/null || echo "Previews not found"
ls -ld "$HOME/Library/Developer/Xcode/Products" 2>/dev/null || echo "Products not found"
ls -ld "$HOME/Library/Developer/Xcode/DocumentationCache" 2>/dev/null || echo "DocumentationCache not found"
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
- `Derived Data`
- `Archives`
- `Device Support` (`iOS`, `watchOS`, `tvOS`)
- `SwiftUI Previews`
- `Products`
- `Documentation Cache`
- `Test Logs`
- `Result Bundles`
- bounded `/private/tmp` Xcode build artifacts only (`xcodebuild-*`, matching `TemporaryItems` entries)

Current behavior:
- Scans bounded Xcode developer storage roots and summarizes size plus file counts by category
- Keeps the pass scan-first and review-first: categories are explained before cleanup
- Shows category-level safety, cleanup recommendation, caution text, default cleanup stance, and stale-first review hints in the TUI
- Uses profile metadata to explain categories and artifacts in the right-side summary panels
- High-confidence categories such as `Derived Data`, `SwiftUI Previews`, `Documentation Cache`, `Test Logs`, and `Result Bundles` can be pre-marked as cleanup candidates
- `Archives` and `Device Support` stay keep-by-default and high-caution
- Entry review is stale-first and size-aware so older and heavier artifacts surface first
- Age-aware bulk review helpers can safely select `very stale`, `>30 day`, or `>90 day` entries before cleanup
- Entry details include age labels and last-modified date labels when metadata is available
- Users can still review each category and choose what to keep
- Cleanup requires explicit confirmation
- Cleanup plans that include `Archives` or `Device Support` require a typed high-caution confirmation before the final confirm screen
- Default cleanup action moves items to macOS Trash

Category safety levels:
- `high confidence`: generally safe generated developer storage
- `medium confidence`: generated artifacts that still deserve review
- `high caution`: keep-by-default storage that may still matter for releases or device debugging

Cleanup recommendation badges:
- `Safe cleanup candidate`
- `Review carefully`
- `Keep by default`

Profile metadata includes:
- What an artifact or category represents
- Why it may be safe or unsafe to clean
- Whether it is selected by default for cleanup
- Whether cleanup is reversible and should move to Trash
- Caution text for review-first categories
- Expected impact after cleaning
- A recommendation for review or cleanup

Safety notes:
- Permanent delete is not implemented
- Cleanroom does not use `rm -rf`
- Entries are validated against known Xcode roots before cleanup
- Symlinks are skipped instead of followed blindly
- Missing folders produce zero-size findings instead of scan failures
- `/private/tmp` scanning stays tightly bounded to clearly Xcode-related patterns
- Archives and Device Support are not default cleanup targets in this pass
- Archives and Device Support still require an extra typed confirmation even if you manually mark them for cleanup
- Bulk age actions skip `Archives` and `Device Support` by default
- Cleanup profiles are descriptive only and cannot execute cleanup actions
- Cleanup logs are written to `~/Library/Logs/Friction Lab/Cleanroom/cleanroom.log`

Review behavior:
- Categories keep a scan-first posture: totals, selected size, file count, safety, and stale signals are shown before cleanup
- Entry ordering favors very stale items first, then stale items, then size, then stable alphabetical order
- Category details and preview summaries include stale counts, very stale counts, and stale selected size
- Preview ordering also surfaces high-caution selections first so risky cleanup plans are obvious
- Missing modified-time metadata falls back to `Unknown` without blocking scanning or cleanup preview
- Bulk-generated selections can be cleared without undoing manual review choices

TUI review helpers:
- From the Xcode category summary action mode: `v` selects very stale safe entries, `3` selects safe entries older than 30 days, `9` selects safe entries older than 90 days, and `u` clears generated bulk selections
- From an entry checklist action mode: the same `v`, `3`, `9`, and `u` helpers apply only to the current safe category
- `Archives` and `Device Support` are still excluded from those bulk helpers unless you manually review and select them yourself
- Cleanup remains explicit, Trash-only, and confirmation-gated after any bulk review action

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
