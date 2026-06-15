use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use crate::{
    logging,
    sources::{
        CleanupExecutionResult, CleanupMode, CleanupPlan, CleanupPreviewItem, CleanupRecord,
        CleanupStatus, xcode,
    },
};

pub fn execute_cleanup(plan: &CleanupPlan, mode: CleanupMode) -> CleanupExecutionResult {
    let mut result = CleanupExecutionResult::new(
        plan.source_name.clone(),
        plan.profile_key.clone(),
        mode,
        logging::default_log_path(),
    );
    let allowed_roots = xcode::allowed_roots();

    for item in &plan.preview_items {
        match validate_candidate(item, &allowed_roots) {
            Ok(()) => {
                if matches!(mode, CleanupMode::DryRun) {
                    result.record(
                        CleanupStatus::DryRunEligible,
                        CleanupRecord::new_from_item(item, "Dry run: candidate is eligible."),
                    );
                    continue;
                }

                match move_path_to_trash(&item.path) {
                    Ok(()) => result.record(
                        CleanupStatus::Moved,
                        CleanupRecord::new_from_item(item, "Moved to Trash."),
                    ),
                    Err(error) => result.record(
                        CleanupStatus::Failed,
                        CleanupRecord::new_from_item(item, format!("Trash move failed: {error}")),
                    ),
                }
            }
            Err(record) => result.record(CleanupStatus::Skipped, *record),
        }
    }

    if matches!(mode, CleanupMode::MoveToTrash)
        && let Err(error) = logging::append_cleanup_log(&result)
    {
        result
            .warnings
            .push(format!("Could not write cleanup log: {error}"));
    }

    result
}

fn validate_candidate(
    item: &CleanupPreviewItem,
    allowed_roots: &[PathBuf],
) -> Result<(), Box<CleanupRecord>> {
    if !allowed_roots.iter().any(|root| root == &item.allowed_root) {
        return Err(Box::new(CleanupRecord::new_from_item(
            item,
            format!(
                "Skipped: allowed root is not configured ({})",
                item.allowed_root.display()
            ),
        )));
    }

    // PathBuf::starts_with is component-wise and does not resolve `..`. Paths here
    // come from fs::read_dir DirEntry::path(), which joins a known root with an OS-
    // provided filename. OS filenames never contain path separator components, so
    // `..` traversal is not possible through this path.
    if !item.path.starts_with(&item.allowed_root) {
        return Err(Box::new(CleanupRecord::new_from_item(
            item,
            format!(
                "Skipped: target is outside allowed root ({})",
                item.allowed_root.display()
            ),
        )));
    }

    let metadata = match fs::symlink_metadata(&item.path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(Box::new(CleanupRecord::new_from_item(
                item,
                "Skipped: target no longer exists.",
            )));
        }
        Err(error) => {
            return Err(Box::new(CleanupRecord::new_from_item(
                item,
                format!("Skipped: could not inspect target: {error}"),
            )));
        }
    };

    if metadata.file_type().is_symlink() {
        return Err(Box::new(CleanupRecord::new_from_item(
            item,
            "Skipped: symlink targets are never moved automatically.",
        )));
    }

    Ok(())
}

fn move_path_to_trash(path: &Path) -> Result<(), String> {
    let script = format!(
        "tell application \"Finder\" to delete POSIX file \"{}\"",
        escape_applescript(path)
    );
    let output = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|error| error.to_string())?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        Err(format!("osascript exited with {}", output.status))
    } else {
        Err(stderr)
    }
}

// Escape a path for embedding in an AppleScript double-quoted string.
//
// Backslashes are escaped first (to avoid double-processing), then quotes.
// This is sufficient because:
// - `"` injection is blocked by the `\"` escape
// - Newlines inside an AppleScript string are a syntax error that makes
//   osascript exit non-zero; they do not allow injecting new statements
// - The path is already validated to be within an allowed root before this
//   function is called
fn escape_applescript(path: &Path) -> String {
    path.display()
        .to_string()
        .replace('\\', "\\\\")
        .replace('\"', "\\\"")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::sources::CleanupPreviewItem;
    use crate::sources::{
        CleanCategoryId, CleanupExecutionResult, CleanupMode, CleanupRecord, CleanupStatus,
        EntryAge, xcode,
    };

    use super::validate_candidate;

    fn dummy_item(path: PathBuf, allowed_root: PathBuf) -> CleanupPreviewItem {
        CleanupPreviewItem {
            category_id: CleanCategoryId::DerivedData,
            category_name: "Test".to_string(),
            category_key: None,
            entry_name: "test-entry".to_string(),
            size_bytes: 100,
            file_count: 1,
            age: EntryAge::default(),
            high_caution: false,
            path,
            allowed_root,
        }
    }

    #[test]
    fn rejects_targets_outside_allowed_root() {
        let allowed_root = xcode::allowed_roots()
            .into_iter()
            .next()
            .expect("expected at least one allowed root");
        let item = dummy_item(PathBuf::from("/tmp/outside"), allowed_root);

        let allowed_roots = xcode::allowed_roots();
        let record =
            validate_candidate(&item, &allowed_roots).expect_err("candidate should be rejected");
        assert!(record.message.contains("outside allowed root"));
    }

    #[test]
    fn rejects_candidate_whose_root_is_not_in_allowed_list() {
        let item = dummy_item(
            PathBuf::from("/tmp/not-a-real-root/entry"),
            PathBuf::from("/tmp/not-a-real-root"),
        );

        let allowed_roots = xcode::allowed_roots();
        let record =
            validate_candidate(&item, &allowed_roots).expect_err("candidate should be rejected");
        assert!(record.message.contains("allowed root is not configured"));
    }

    #[test]
    fn nonexistent_candidate_is_skipped_gracefully() {
        let allowed_root = xcode::allowed_roots()
            .into_iter()
            .next()
            .expect("expected at least one allowed root");
        // Path is inside the allowed root but does not exist on disk.
        let path = allowed_root.join("cleanroom-test-definitely-nonexistent-99999");
        let item = dummy_item(path, allowed_root);

        let allowed_roots = xcode::allowed_roots();
        let record =
            validate_candidate(&item, &allowed_roots).expect_err("nonexistent path should skip");
        assert!(
            record.message.contains("no longer exists")
                || record.message.contains("could not inspect"),
            "unexpected message: {}",
            record.message
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_candidate_is_rejected() {
        use std::{fs, os::unix::fs as unix_fs};

        let ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be valid")
            .subsec_nanos();
        let root = std::env::temp_dir().join(format!("cleanroom-sym-{}-{ns}", std::process::id()));
        let link_path = root.join("sym-entry");

        fs::create_dir_all(&root).expect("fixture dir should be created");
        unix_fs::symlink("/tmp", &link_path).expect("symlink fixture should be created");

        let item = dummy_item(link_path.clone(), root.clone());
        let allowed_roots = vec![root.clone()];
        let record =
            validate_candidate(&item, &allowed_roots).expect_err("symlink should be rejected");
        assert!(
            record.message.contains("symlink"),
            "unexpected message: {}",
            record.message
        );

        let _ = fs::remove_file(&link_path);
        let _ = fs::remove_dir(&root);
    }

    #[test]
    fn dry_run_eligible_does_not_increment_moved_count() {
        let mut result = CleanupExecutionResult::new(
            "Test".to_string(),
            "test".to_string(),
            CleanupMode::DryRun,
            PathBuf::from("/tmp/test.log"),
        );
        let record = CleanupRecord {
            category_name: "Test".to_string(),
            category_key: None,
            entry_name: "test-entry".to_string(),
            path: PathBuf::from("/tmp/test-entry"),
            size_bytes: 200,
            message: "Dry run: candidate is eligible.".to_string(),
        };

        result.record(CleanupStatus::DryRunEligible, record);

        assert_eq!(
            result.moved_count, 0,
            "dry run must not increment moved_count"
        );
        assert_eq!(
            result.cleaned_size_bytes, 0,
            "dry run must not increment cleaned_size_bytes"
        );
        assert_eq!(
            result.dry_run_eligible_count, 1,
            "eligible count should be tracked"
        );
        assert_eq!(result.dry_run_eligible_items.len(), 1);
        assert_eq!(result.skipped_count, 0);
        assert_eq!(result.failed_count, 0);
    }

    #[test]
    fn moved_status_increments_moved_count_and_cleaned_bytes() {
        let mut result = CleanupExecutionResult::new(
            "Test".to_string(),
            "test".to_string(),
            CleanupMode::MoveToTrash,
            PathBuf::from("/tmp/test.log"),
        );
        let record = CleanupRecord {
            category_name: "Test".to_string(),
            category_key: None,
            entry_name: "test-entry".to_string(),
            path: PathBuf::from("/tmp/test-entry"),
            size_bytes: 500,
            message: "Moved to Trash.".to_string(),
        };

        result.record(CleanupStatus::Moved, record);

        assert_eq!(result.moved_count, 1);
        assert_eq!(result.cleaned_size_bytes, 500);
        assert_eq!(result.dry_run_eligible_count, 0);
    }
}
