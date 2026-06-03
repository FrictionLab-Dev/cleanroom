use std::{
    env, fs,
    path::{Path, PathBuf},
};

use crate::{
    profile::{CleanerProfile, ProfileCategory, ProfileLoadError, ProfileRule, SafetyLevel},
    size::path_size_bytes,
    sources::{
        CleanCategory, CleanCategoryId, CleanCategoryMetadata, CleanEntry, CleanEntryMetadata,
        CleanSourceId, ScannedSource,
    },
};

pub const HOME_OVERRIDE_ENV: &str = "CLEANROOM_HOME_OVERRIDE";
pub const LEGACY_HOME_OVERRIDE_ENV: &str = "PCLEAN_HOME_OVERRIDE";

#[derive(Clone)]
struct LoadedProfile {
    category_specs: Vec<CategorySpec>,
    profile: CleanerProfile,
}

#[derive(Clone)]
struct CategorySpec {
    id: CleanCategoryId,
    name: String,
    path: String,
    metadata: Option<CleanCategoryMetadata>,
}

struct FallbackCategorySpec {
    id: CleanCategoryId,
    name: &'static str,
    path: &'static str,
}

const FALLBACK_CATEGORY_SPECS: [FallbackCategorySpec; 4] = [
    FallbackCategorySpec {
        id: CleanCategoryId::DerivedData,
        name: "DerivedData",
        path: "~/Library/Developer/Xcode/DerivedData",
    },
    FallbackCategorySpec {
        id: CleanCategoryId::DeviceSupport,
        name: "iOS DeviceSupport",
        path: "~/Library/Developer/Xcode/iOS DeviceSupport",
    },
    FallbackCategorySpec {
        id: CleanCategoryId::Archives,
        name: "Archives",
        path: "~/Library/Developer/Xcode/Archives",
    },
    FallbackCategorySpec {
        id: CleanCategoryId::SimulatorCaches,
        name: "CoreSimulator Caches",
        path: "~/Library/Developer/CoreSimulator/Caches",
    },
];

pub fn scan() -> ScannedSource {
    let home_dir = configured_home_dir();
    let mut warnings = Vec::new();
    // Profile load failure is recoverable: keep the Xcode source available and
    // fall back to the built-in roots instead of failing the TUI startup path.
    let loaded_profile = match load_xcode_profile() {
        Ok(profile) => Some(profile),
        Err(error) => {
            warnings.push(format!(
                "Could not load bundled Xcode profile; using built-in fallback roots: {error}"
            ));
            None
        }
    };
    let category_specs = loaded_profile
        .as_ref()
        .map(|profile| profile.category_specs.clone())
        .unwrap_or_else(fallback_category_specs);
    let categories = category_specs
        .iter()
        .map(|spec| scan_category(spec, &home_dir, loaded_profile.as_ref(), &mut warnings))
        .collect();

    ScannedSource {
        source_id: CleanSourceId::Xcode,
        source_name: "Xcode",
        profile_key: "xcode".to_string(),
        root_hint: home_dir.join("Library/Developer"),
        categories,
        warnings,
    }
}

pub fn configured_home_dir() -> PathBuf {
    env::var_os(HOME_OVERRIDE_ENV)
        .or_else(|| env::var_os(LEGACY_HOME_OVERRIDE_ENV))
        .or_else(|| env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn allowed_roots() -> Vec<PathBuf> {
    let home_dir = configured_home_dir();
    let category_specs = load_xcode_profile()
        .map(|profile| profile.category_specs)
        .unwrap_or_else(|_| fallback_category_specs());

    category_specs
        .iter()
        .map(|spec| expand_profile_path(&spec.path, &home_dir))
        .collect()
}

fn load_xcode_profile() -> Result<LoadedProfile, ProfileLoadError> {
    let profile = CleanerProfile::bundled_xcode()?;
    let category_specs = profile
        .categories
        .iter()
        .map(category_spec_from_profile)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(LoadedProfile {
        category_specs,
        profile,
    })
}

fn category_spec_from_profile(
    category: &ProfileCategory,
) -> Result<CategorySpec, ProfileLoadError> {
    Ok(CategorySpec {
        id: category_id_from_profile(category.id.as_str())?,
        name: category.name.clone(),
        path: category.path.clone(),
        metadata: Some(CleanCategoryMetadata {
            description: category.description.clone(),
            safety: category.safety,
            recommendation: category.recommendation.clone(),
            impact: category.impact.clone(),
        }),
    })
}

fn category_id_from_profile(category_id: &str) -> Result<CleanCategoryId, ProfileLoadError> {
    match category_id {
        "derived-data" => Ok(CleanCategoryId::DerivedData),
        "device-support" => Ok(CleanCategoryId::DeviceSupport),
        "archives" => Ok(CleanCategoryId::Archives),
        "core-simulator-caches" => Ok(CleanCategoryId::SimulatorCaches),
        other => Err(ProfileLoadError::new(format!(
            "Unknown Xcode profile category id '{other}'"
        ))),
    }
}

fn fallback_category_specs() -> Vec<CategorySpec> {
    FALLBACK_CATEGORY_SPECS
        .iter()
        .map(|spec| CategorySpec {
            id: spec.id,
            name: spec.name.to_string(),
            path: spec.path.to_string(),
            metadata: None,
        })
        .collect()
}

fn expand_profile_path(profile_path: &str, home_dir: &Path) -> PathBuf {
    if profile_path == "~" {
        return home_dir.to_path_buf();
    }

    if let Some(relative_path) = profile_path.strip_prefix("~/") {
        return home_dir.join(relative_path);
    }

    PathBuf::from(profile_path)
}

fn scan_category(
    spec: &CategorySpec,
    home_dir: &Path,
    loaded_profile: Option<&LoadedProfile>,
    warnings: &mut Vec<String>,
) -> CleanCategory {
    let path = expand_profile_path(&spec.path, home_dir);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return CleanCategory {
                id: spec.id,
                name: spec.name.clone(),
                stats_key: Some(category_stats_key(spec.id)),
                path,
                exists: false,
                note: Some("path missing".to_string()),
                warnings: Vec::new(),
                entries: Vec::new(),
                total_size_bytes: 0,
                metadata: spec.metadata.clone(),
            };
        }
        Err(error) => {
            let message = format!("Could not inspect {}: {}", path.display(), error);
            warnings.push(message.clone());
            return CleanCategory {
                id: spec.id,
                name: spec.name.clone(),
                stats_key: Some(category_stats_key(spec.id)),
                path,
                exists: true,
                note: Some("inspect warning".to_string()),
                warnings: vec![message],
                entries: Vec::new(),
                total_size_bytes: 0,
                metadata: spec.metadata.clone(),
            };
        }
    };

    if metadata.file_type().is_symlink() {
        let message = format!("Skipped symlink category root {}", path.display());
        warnings.push(message.clone());
        return CleanCategory {
            id: spec.id,
            name: spec.name.clone(),
            stats_key: Some(category_stats_key(spec.id)),
            path,
            exists: true,
            note: Some("symlink skipped".to_string()),
            warnings: vec![message],
            entries: Vec::new(),
            total_size_bytes: 0,
            metadata: spec.metadata.clone(),
        };
    }

    if !metadata.is_dir() {
        let message = format!("Expected directory at {}", path.display());
        warnings.push(message.clone());
        return CleanCategory {
            id: spec.id,
            name: spec.name.clone(),
            stats_key: Some(category_stats_key(spec.id)),
            path,
            exists: true,
            note: Some("not a directory".to_string()),
            warnings: vec![message],
            entries: Vec::new(),
            total_size_bytes: 0,
            metadata: spec.metadata.clone(),
        };
    }

    let mut category_warnings = Vec::new();
    let read_dir = match fs::read_dir(&path) {
        Ok(read_dir) => read_dir,
        Err(error) => {
            let message = format!("Could not read {}: {}", path.display(), error);
            warnings.push(message.clone());
            return CleanCategory {
                id: spec.id,
                name: spec.name.clone(),
                stats_key: Some(category_stats_key(spec.id)),
                path,
                exists: true,
                note: Some("read warning".to_string()),
                warnings: vec![message],
                entries: Vec::new(),
                total_size_bytes: 0,
                metadata: spec.metadata.clone(),
            };
        }
    };

    let mut entries = Vec::new();
    for child in read_dir {
        let entry = match child {
            Ok(entry) => entry,
            Err(error) => {
                category_warnings.push(format!(
                    "Could not read child in {}: {}",
                    path.display(),
                    error
                ));
                continue;
            }
        };

        let entry_path = entry.path();
        let metadata = match fs::symlink_metadata(&entry_path) {
            Ok(metadata) => metadata,
            Err(error) => {
                category_warnings.push(format!(
                    "Could not inspect {}: {}",
                    entry_path.display(),
                    error
                ));
                continue;
            }
        };

        if metadata.file_type().is_symlink() {
            category_warnings.push(format!("Skipped symlink {}", entry_path.display()));
            continue;
        }

        let entry_name = entry.file_name().to_string_lossy().into_owned();
        let size_bytes = path_size_bytes(&entry_path, &mut category_warnings);
        entries.push(CleanEntry {
            name: entry_name.clone(),
            path: entry_path,
            size_bytes,
            keep: true,
            metadata: entry_metadata(&entry_name, loaded_profile.map(|profile| &profile.profile)),
        });
    }

    entries.sort_by(|left, right| {
        right
            .size_bytes
            .cmp(&left.size_bytes)
            .then_with(|| left.name.cmp(&right.name))
    });

    let total_size_bytes = entries.iter().map(|entry| entry.size_bytes).sum();
    warnings.extend(category_warnings.iter().cloned());
    let note = if category_warnings.is_empty() {
        None
    } else {
        Some(format!("{} warning(s)", category_warnings.len()))
    };

    CleanCategory {
        id: spec.id,
        name: spec.name.clone(),
        stats_key: Some(category_stats_key(spec.id)),
        path,
        exists: true,
        note,
        warnings: category_warnings,
        entries,
        total_size_bytes,
        metadata: spec.metadata.clone(),
    }
}

fn entry_metadata(entry_name: &str, profile: Option<&CleanerProfile>) -> CleanEntryMetadata {
    if let Some(rule) = profile.and_then(|profile| profile.match_rule(entry_name)) {
        return metadata_from_rule(rule);
    }

    CleanEntryMetadata {
        matched_rule: None,
        description: "Generated artifact not recognized by the bundled profile.".to_string(),
        safety: SafetyLevel::Unknown,
        recommendation: "Inspect before cleaning.".to_string(),
        impact: Some("Impact varies by project state.".to_string()),
    }
}

fn metadata_from_rule(rule: &ProfileRule) -> CleanEntryMetadata {
    CleanEntryMetadata {
        matched_rule: Some(rule.pattern.clone()),
        description: rule.description.clone(),
        safety: rule.safety,
        recommendation: rule.recommendation.clone(),
        impact: rule.impact.clone(),
    }
}

fn category_stats_key(category_id: CleanCategoryId) -> String {
    let suffix = match category_id {
        CleanCategoryId::DerivedData => "derivedData",
        CleanCategoryId::DeviceSupport => "deviceSupport",
        CleanCategoryId::Archives => "archives",
        CleanCategoryId::SimulatorCaches => "coreSimulatorCaches",
    };

    format!("xcode.{suffix}")
}

#[cfg(test)]
mod tests {
    use crate::profile::SafetyLevel;

    use super::{entry_metadata, load_xcode_profile};

    #[test]
    fn category_metadata_lookup_uses_bundled_profile() {
        let loaded = load_xcode_profile().expect("xcode profile should load");
        let derived_data = loaded
            .category_specs
            .iter()
            .find(|category| category.name == "DerivedData")
            .expect("derived data category should exist");

        let metadata = derived_data
            .metadata
            .as_ref()
            .expect("profile metadata should exist");
        assert_eq!(metadata.safety, SafetyLevel::Rebuildable);
        assert!(metadata.description.contains("Build products"));
    }

    #[test]
    fn entry_rule_metadata_lookup_matches_known_artifacts() {
        let loaded = load_xcode_profile().expect("xcode profile should load");
        let metadata = entry_metadata("ModuleCache.noindex", Some(&loaded.profile));

        assert_eq!(metadata.safety, SafetyLevel::Rebuildable);
        assert_eq!(
            metadata.matched_rule.as_deref(),
            Some("ModuleCache.noindex")
        );
    }

    #[test]
    fn unmatched_entries_fall_back_to_unknown_metadata() {
        let loaded = load_xcode_profile().expect("xcode profile should load");
        let metadata = entry_metadata("NotAKnownArtifact", Some(&loaded.profile));

        assert_eq!(metadata.safety, SafetyLevel::Unknown);
        assert!(metadata.matched_rule.is_none());
        assert_eq!(metadata.recommendation, "Inspect before cleaning.");
    }
}
