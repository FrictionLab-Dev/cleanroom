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
                        CleanupStatus::Moved,
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

fn escape_applescript(path: &Path) -> String {
    path.display()
        .to_string()
        .replace('\\', "\\\\")
        .replace('\"', "\\\"")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::sources::{CleanupPreviewItem, xcode};

    use super::validate_candidate;

    #[test]
    fn rejects_targets_outside_allowed_root() {
        let allowed_root = xcode::allowed_roots()
            .into_iter()
            .next()
            .expect("expected at least one allowed root");
        let item = CleanupPreviewItem {
            category_name: "DerivedData".to_string(),
            category_key: Some("xcode.derivedData".to_string()),
            entry_name: "outside".to_string(),
            size_bytes: 1,
            path: PathBuf::from("/tmp/outside"),
            allowed_root,
        };

        let allowed_roots = xcode::allowed_roots();
        let record =
            validate_candidate(&item, &allowed_roots).expect_err("candidate should be rejected");
        assert!(record.message.contains("outside allowed root"));
    }
}
