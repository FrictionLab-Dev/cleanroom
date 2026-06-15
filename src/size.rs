use std::{fs, path::Path, time::UNIX_EPOCH};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PathMetrics {
    pub size_bytes: u64,
    pub file_count: u64,
    pub last_modified_unix_seconds: Option<u64>,
}

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];

    if bytes < 1024 {
        return format!("{} B", bytes);
    }

    let mut value = bytes as f64;
    let mut unit_index = 0_usize;
    while value >= 1024.0 && unit_index + 1 < UNITS.len() {
        value /= 1024.0;
        unit_index += 1;
    }

    format!("{value:.1} {}", UNITS[unit_index])
}

pub fn path_metrics(path: &Path, warnings: &mut Vec<String>) -> PathMetrics {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => {
            warnings.push(format!("Could not inspect {}: {}", path.display(), error));
            return PathMetrics::default();
        }
    };

    if metadata.file_type().is_symlink() {
        warnings.push(format!("Skipped symlink {}", path.display()));
        return PathMetrics::default();
    }

    let path_modified = modified_unix_seconds(path, &metadata, warnings);

    if metadata.is_file() {
        return PathMetrics {
            size_bytes: metadata.len(),
            file_count: 1,
            last_modified_unix_seconds: path_modified,
        };
    }

    if !metadata.is_dir() {
        return PathMetrics::default();
    }

    let read_dir = match fs::read_dir(path) {
        Ok(read_dir) => read_dir,
        Err(error) => {
            warnings.push(format!("Could not read {}: {}", path.display(), error));
            return PathMetrics {
                last_modified_unix_seconds: path_modified,
                ..PathMetrics::default()
            };
        }
    };

    let mut total = PathMetrics {
        last_modified_unix_seconds: path_modified,
        ..PathMetrics::default()
    };
    for child in read_dir {
        match child {
            Ok(entry) => {
                let child_metrics = path_metrics(&entry.path(), warnings);
                total.size_bytes += child_metrics.size_bytes;
                total.file_count += child_metrics.file_count;
                total.last_modified_unix_seconds = latest_modified(
                    total.last_modified_unix_seconds,
                    child_metrics.last_modified_unix_seconds,
                );
            }
            Err(error) => warnings.push(format!(
                "Could not read entry under {}: {}",
                path.display(),
                error
            )),
        }
    }

    total
}

fn modified_unix_seconds(
    path: &Path,
    metadata: &fs::Metadata,
    warnings: &mut Vec<String>,
) -> Option<u64> {
    let modified = match metadata.modified() {
        Ok(modified) => modified,
        Err(error) => {
            warnings.push(format!(
                "Could not read modified time for {}: {}",
                path.display(),
                error
            ));
            return None;
        }
    };

    modified
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
}

fn latest_modified(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}
