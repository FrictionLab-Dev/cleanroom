use std::{
    env,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::sources::CleanupExecutionResult;

const LOG_PATH_ENV: &str = "CLEANROOM_LOG_PATH";
const LEGACY_LOG_PATH_ENV: &str = "PCLEAN_LOG_PATH";

pub fn default_log_path() -> PathBuf {
    if let Some(override_path) =
        env::var_os(LOG_PATH_ENV).or_else(|| env::var_os(LEGACY_LOG_PATH_ENV))
    {
        return PathBuf::from(override_path);
    }

    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Library/Logs/Friction Lab/Cleanroom/cleanroom.log")
}

pub fn append_cleanup_log(result: &CleanupExecutionResult) -> io::Result<()> {
    let log_path = &result.log_path;
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;

    writeln!(
        file,
        "[{}] source={} mode={} moved={} skipped={} failed={} cleaned_size_bytes={}",
        unix_timestamp(),
        result.source_name,
        result.mode.label(),
        result.moved_count,
        result.skipped_count,
        result.failed_count,
        result.cleaned_size_bytes
    )?;

    for item in &result.moved_items {
        writeln!(
            file,
            "  moved category={} path={} size_bytes={} note={}",
            item.category_name,
            item.path.display(),
            item.size_bytes,
            item.message
        )?;
    }

    for item in &result.skipped_items {
        writeln!(
            file,
            "  skipped category={} path={} size_bytes={} note={}",
            item.category_name,
            item.path.display(),
            item.size_bytes,
            item.message
        )?;
    }

    for item in &result.failed_items {
        writeln!(
            file,
            "  failed category={} path={} size_bytes={} note={}",
            item.category_name,
            item.path.display(),
            item.size_bytes,
            item.message
        )?;
    }

    for warning in &result.warnings {
        writeln!(file, "  warning {}", warning)?;
    }

    Ok(())
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}
