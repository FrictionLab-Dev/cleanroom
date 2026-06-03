use std::{fs, path::Path};

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

pub fn path_size_bytes(path: &Path, warnings: &mut Vec<String>) -> u64 {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => {
            warnings.push(format!("Could not inspect {}: {}", path.display(), error));
            return 0;
        }
    };

    if metadata.file_type().is_symlink() {
        warnings.push(format!("Skipped symlink {}", path.display()));
        return 0;
    }

    if metadata.is_file() {
        return metadata.len();
    }

    if !metadata.is_dir() {
        return 0;
    }

    let read_dir = match fs::read_dir(path) {
        Ok(read_dir) => read_dir,
        Err(error) => {
            warnings.push(format!("Could not read {}: {}", path.display(), error));
            return 0;
        }
    };

    let mut total = 0_u64;
    for child in read_dir {
        match child {
            Ok(entry) => total += path_size_bytes(&entry.path(), warnings),
            Err(error) => warnings.push(format!(
                "Could not read entry under {}: {}",
                path.display(),
                error
            )),
        }
    }

    total
}
