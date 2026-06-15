use std::{
    collections::BTreeMap,
    env, fs, io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::sources::CleanupExecutionResult;

const STATS_PATH_ENV: &str = "CLEANROOM_STATS_PATH";
const LEGACY_STATS_PATH_ENV: &str = "PCLEAN_STATS_PATH";

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupStats {
    pub total_cleanups: u64,
    pub total_bytes_cleaned: u64,
    pub entries_cleaned: u64,
    #[serde(default)]
    pub by_profile: BTreeMap<String, AggregateBucket>,
    #[serde(default)]
    pub by_category: BTreeMap<String, AggregateBucket>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregateBucket {
    pub bytes_cleaned: u64,
    pub entries_cleaned: u64,
}

pub fn default_stats_path() -> PathBuf {
    if let Some(override_path) =
        env::var_os(STATS_PATH_ENV).or_else(|| env::var_os(LEGACY_STATS_PATH_ENV))
    {
        return PathBuf::from(override_path);
    }

    home_dir().join("Library/Application Support/Friction Lab/Cleanroom/stats.json")
}

pub fn load_stats_from_path(path: &Path) -> CleanupStats {
    // Stats are optional UX metadata. Missing or malformed files should never
    // interrupt scanning or cleanup flows.
    let Ok(contents) = fs::read_to_string(path) else {
        return CleanupStats::default();
    };

    serde_json::from_str(&contents).unwrap_or_default()
}

pub fn record_cleanup(result: &CleanupExecutionResult) -> io::Result<CleanupStats> {
    let stats_path = default_stats_path();
    let mut stats = load_stats_for_recording(&stats_path);
    stats.total_cleanups += 1;
    stats.total_bytes_cleaned += result.cleaned_size_bytes;
    stats.entries_cleaned += result.moved_count as u64;

    increment_bucket(
        stats
            .by_profile
            .entry(result.profile_key.clone())
            .or_default(),
        result.cleaned_size_bytes,
        result.moved_count as u64,
    );

    for record in &result.moved_items {
        if let Some(category_key) = &record.category_key {
            increment_bucket(
                stats.by_category.entry(category_key.clone()).or_default(),
                record.size_bytes,
                1,
            );
        }
    }

    save_stats(&stats_path, &stats)?;
    Ok(stats)
}

#[cfg(test)]
pub fn record_cleanup_to_path(
    path: &Path,
    result: &CleanupExecutionResult,
) -> io::Result<CleanupStats> {
    let mut stats = load_stats_from_path(path);
    stats.total_cleanups += 1;
    stats.total_bytes_cleaned += result.cleaned_size_bytes;
    stats.entries_cleaned += result.moved_count as u64;

    increment_bucket(
        stats
            .by_profile
            .entry(result.profile_key.clone())
            .or_default(),
        result.cleaned_size_bytes,
        result.moved_count as u64,
    );

    for record in &result.moved_items {
        if let Some(category_key) = &record.category_key {
            increment_bucket(
                stats.by_category.entry(category_key.clone()).or_default(),
                record.size_bytes,
                1,
            );
        }
    }

    save_stats(path, &stats)?;
    Ok(stats)
}

fn save_stats(path: &Path, stats: &CleanupStats) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Persist aggregate-only counters. Full cleaned paths stay in the session
    // result/log flow and are not written into long-term stats.
    let contents =
        serde_json::to_vec_pretty(stats).map_err(|error| io::Error::other(error.to_string()))?;
    fs::write(path, contents)
}

fn home_dir() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn legacy_stats_path() -> PathBuf {
    home_dir().join("Library/Application Support/PathPilot/pclean/stats.json")
}

fn load_stats_for_recording(path: &Path) -> CleanupStats {
    if path.exists() {
        return load_stats_from_path(path);
    }

    let legacy_path = match env::var_os(STATS_PATH_ENV) {
        Some(_) => return CleanupStats::default(),
        None => env::var_os(LEGACY_STATS_PATH_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(legacy_stats_path),
    };

    if legacy_path == path {
        return CleanupStats::default();
    }

    load_stats_from_path(&legacy_path)
}

fn increment_bucket(bucket: &mut AggregateBucket, bytes_cleaned: u64, entries_cleaned: u64) {
    bucket.bytes_cleaned += bytes_cleaned;
    bucket.entries_cleaned += entries_cleaned;
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use crate::sources::{CleanupExecutionResult, CleanupMode, CleanupRecord};

    use super::{
        AggregateBucket, CleanupStats, increment_bucket, load_stats_from_path,
        record_cleanup_to_path,
    };

    fn temp_path(name: &str) -> PathBuf {
        let unique = format!(
            "cleanroom-stats-test-{name}-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock should be valid")
                .as_nanos()
        );
        std::env::temp_dir().join(unique)
    }

    #[test]
    fn missing_stats_file_loads_defaults() {
        let path = temp_path("missing");
        let stats = load_stats_from_path(&path);

        assert_eq!(stats, CleanupStats::default());
    }

    #[test]
    fn malformed_stats_file_does_not_crash() {
        let path = temp_path("malformed");
        fs::write(&path, "{ definitely not json").expect("fixture should write");

        let stats = load_stats_from_path(&path);

        assert_eq!(stats, CleanupStats::default());
    }

    #[test]
    fn stats_update_increments_totals_and_aggregates() {
        let path = temp_path("record");

        let result = CleanupExecutionResult {
            source_name: "Xcode".to_string(),
            profile_key: "xcode".to_string(),
            mode: CleanupMode::MoveToTrash,
            log_path: PathBuf::from("/tmp/cleanroom.log"),
            moved_count: 2,
            dry_run_eligible_count: 0,
            skipped_count: 0,
            failed_count: 0,
            cleaned_size_bytes: 300,
            moved_items: vec![
                CleanupRecord {
                    category_name: "DerivedData".to_string(),
                    category_key: Some("xcode.derivedData".to_string()),
                    entry_name: "ModuleCache.noindex".to_string(),
                    path: PathBuf::from("/private/tmp/one"),
                    size_bytes: 100,
                    message: "Moved to Trash.".to_string(),
                },
                CleanupRecord {
                    category_name: "Archives".to_string(),
                    category_key: Some("xcode.archives".to_string()),
                    entry_name: "Archive.xcarchive".to_string(),
                    path: PathBuf::from("/private/tmp/two"),
                    size_bytes: 200,
                    message: "Moved to Trash.".to_string(),
                },
            ],
            dry_run_eligible_items: Vec::new(),
            skipped_items: Vec::new(),
            failed_items: Vec::new(),
            warnings: Vec::new(),
        };

        let stats = record_cleanup_to_path(&path, &result).expect("stats should record");

        assert_eq!(stats.total_cleanups, 1);
        assert_eq!(stats.total_bytes_cleaned, 300);
        assert_eq!(stats.entries_cleaned, 2);
        assert_eq!(
            stats.by_profile.get("xcode"),
            Some(&AggregateBucket {
                bytes_cleaned: 300,
                entries_cleaned: 2
            })
        );
        assert_eq!(
            stats.by_category.get("xcode.derivedData"),
            Some(&AggregateBucket {
                bytes_cleaned: 100,
                entries_cleaned: 1
            })
        );
    }

    #[test]
    fn record_cleanup_uses_the_requested_stats_file_path() {
        let path = temp_path("specific-path");
        let other_path = temp_path("other-path");
        fs::write(
            &path,
            r#"{"totalCleanups":2,"totalBytesCleaned":10,"entriesCleaned":1}"#,
        )
        .expect("fixture should write");

        let result = CleanupExecutionResult {
            source_name: "Xcode".to_string(),
            profile_key: "xcode".to_string(),
            mode: CleanupMode::MoveToTrash,
            log_path: PathBuf::from("/tmp/cleanroom.log"),
            moved_count: 1,
            dry_run_eligible_count: 0,
            skipped_count: 0,
            failed_count: 0,
            cleaned_size_bytes: 5,
            moved_items: vec![CleanupRecord {
                category_name: "DerivedData".to_string(),
                category_key: Some("xcode.derivedData".to_string()),
                entry_name: "ModuleCache.noindex".to_string(),
                path: PathBuf::from("/private/tmp/one"),
                size_bytes: 5,
                message: "Moved to Trash.".to_string(),
            }],
            dry_run_eligible_items: Vec::new(),
            skipped_items: Vec::new(),
            failed_items: Vec::new(),
            warnings: Vec::new(),
        };

        let stats = record_cleanup_to_path(&path, &result).expect("stats should record");

        assert_eq!(stats.total_cleanups, 3);
        assert!(!other_path.exists());
    }

    #[test]
    fn serialized_stats_do_not_include_cleaned_paths() {
        let mut stats = CleanupStats::default();
        stats.total_cleanups = 1;
        increment_bucket(
            stats.by_profile.entry("xcode".to_string()).or_default(),
            42,
            1,
        );

        let serialized = serde_json::to_string(&stats).expect("stats should serialize");

        assert!(!serialized.contains("/Users/"));
        assert!(!serialized.contains("/private/"));
        assert!(!serialized.contains("path"));
    }
}
