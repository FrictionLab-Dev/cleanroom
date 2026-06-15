use std::{
    env, fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::{
    profile::{
        CategorySafetyLevel, CleanerProfile, CleanupRecommendationKind, ProfileCategory,
        ProfileLoadError, ProfileRule, SafetyLevel,
    },
    size::path_metrics,
    sources::{
        CleanCategory, CleanCategoryId, CleanCategoryMetadata, CleanEntry, CleanEntryMetadata,
        CleanSourceId, EntryAge, ScannedSource, StaleBucket,
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
    display_path: String,
    scan_roots: Vec<String>,
    scan_kind: ScanKind,
    metadata: CleanCategoryMetadata,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScanKind {
    ImmediateChildren,
    DerivedDataTestLogs,
    DerivedDataResultBundles,
    TemporaryXcodeBuildFolders,
}

pub fn scan() -> ScannedSource {
    scan_for_home(&configured_home_dir())
}

pub(crate) fn scan_for_home(home_dir: &Path) -> ScannedSource {
    scan_for_home_with_tmp_root(home_dir, None)
}

fn scan_for_home_with_tmp_root(home_dir: &Path, tmp_root_override: Option<&Path>) -> ScannedSource {
    scan_for_home_with_tmp_root_and_now(home_dir, tmp_root_override, SystemTime::now())
}

fn scan_for_home_with_tmp_root_and_now(
    home_dir: &Path,
    tmp_root_override: Option<&Path>,
    now: SystemTime,
) -> ScannedSource {
    let mut warnings = Vec::new();
    let loaded_profile = match load_xcode_profile() {
        Ok(profile) => Some(profile),
        Err(error) => {
            warnings.push(format!(
                "Could not load bundled Xcode profile; using built-in fallback metadata: {error}"
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
        .map(|spec| {
            scan_category(
                spec,
                home_dir,
                tmp_root_override,
                now,
                loaded_profile.as_ref(),
                &mut warnings,
            )
        })
        .collect();

    ScannedSource {
        source_id: CleanSourceId::Xcode,
        source_name: "Xcode",
        profile_key: "xcode".to_string(),
        root_hint: home_dir.join("Library/Developer/Xcode"),
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
    allowed_roots_for_home(&configured_home_dir())
}

pub(crate) fn allowed_roots_for_home(home_dir: &Path) -> Vec<PathBuf> {
    allowed_roots_for_home_with_tmp_root(home_dir, None)
}

fn allowed_roots_for_home_with_tmp_root(
    home_dir: &Path,
    tmp_root_override: Option<&Path>,
) -> Vec<PathBuf> {
    let category_specs = load_xcode_profile()
        .map(|profile| profile.category_specs)
        .unwrap_or_else(|_| fallback_category_specs());
    let mut roots = Vec::new();

    for spec in category_specs {
        for root in spec
            .scan_roots
            .iter()
            .map(|root| resolve_scan_root(root, home_dir, tmp_root_override))
        {
            if !roots.contains(&root) {
                roots.push(root);
            }
        }
    }

    roots
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
    let id = category_id_from_profile(category.id.as_str())?;

    Ok(CategorySpec {
        id,
        name: category.name.clone(),
        display_path: category.path.clone(),
        scan_roots: scan_root_templates(id)
            .into_iter()
            .map(str::to_string)
            .collect(),
        scan_kind: scan_kind_for_category(id),
        metadata: CleanCategoryMetadata {
            description: category.description.clone(),
            safety: category.safety,
            default_cleanup: category.default_cleanup,
            cleanup_kind: category.cleanup_kind,
            reversible: category.reversible,
            move_to_trash: category.move_to_trash,
            caution: category.caution.clone(),
            recommendation: category.recommendation.clone(),
            impact: category.impact.clone(),
        },
    })
}

fn category_id_from_profile(category_id: &str) -> Result<CleanCategoryId, ProfileLoadError> {
    match category_id {
        "derived-data" => Ok(CleanCategoryId::DerivedData),
        "archives" => Ok(CleanCategoryId::Archives),
        "device-support" => Ok(CleanCategoryId::DeviceSupport),
        "swiftui-previews" => Ok(CleanCategoryId::SwiftUIPreviews),
        "products" => Ok(CleanCategoryId::Products),
        "documentation-cache" => Ok(CleanCategoryId::DocumentationCache),
        "test-logs" => Ok(CleanCategoryId::TestLogs),
        "result-bundles" => Ok(CleanCategoryId::ResultBundles),
        "temporary-xcode-build-folders" => Ok(CleanCategoryId::TemporaryXcodeBuildFolders),
        other => Err(ProfileLoadError::new(format!(
            "Unknown Xcode profile category id '{other}'"
        ))),
    }
}

fn scan_root_templates(category_id: CleanCategoryId) -> Vec<&'static str> {
    match category_id {
        CleanCategoryId::DerivedData => vec!["~/Library/Developer/Xcode/DerivedData"],
        CleanCategoryId::Archives => vec!["~/Library/Developer/Xcode/Archives"],
        CleanCategoryId::DeviceSupport => vec![
            "~/Library/Developer/Xcode/iOS DeviceSupport",
            "~/Library/Developer/Xcode/watchOS DeviceSupport",
            "~/Library/Developer/Xcode/tvOS DeviceSupport",
        ],
        CleanCategoryId::SwiftUIPreviews => {
            vec!["~/Library/Developer/Xcode/UserData/Previews"]
        }
        CleanCategoryId::Products => vec!["~/Library/Developer/Xcode/Products"],
        CleanCategoryId::DocumentationCache => {
            vec!["~/Library/Developer/Xcode/DocumentationCache"]
        }
        CleanCategoryId::TestLogs => vec!["~/Library/Developer/Xcode/DerivedData"],
        CleanCategoryId::ResultBundles => vec!["~/Library/Developer/Xcode/DerivedData"],
        CleanCategoryId::TemporaryXcodeBuildFolders => vec!["/private/tmp"],
    }
}

fn scan_kind_for_category(category_id: CleanCategoryId) -> ScanKind {
    match category_id {
        CleanCategoryId::DerivedData
        | CleanCategoryId::Archives
        | CleanCategoryId::DeviceSupport
        | CleanCategoryId::SwiftUIPreviews
        | CleanCategoryId::Products
        | CleanCategoryId::DocumentationCache => ScanKind::ImmediateChildren,
        CleanCategoryId::TestLogs => ScanKind::DerivedDataTestLogs,
        CleanCategoryId::ResultBundles => ScanKind::DerivedDataResultBundles,
        CleanCategoryId::TemporaryXcodeBuildFolders => ScanKind::TemporaryXcodeBuildFolders,
    }
}

fn fallback_category_specs() -> Vec<CategorySpec> {
    vec![
        fallback_category_spec(
            CleanCategoryId::DerivedData,
            "Derived Data",
            "~/Library/Developer/Xcode/DerivedData",
            "Build products, indexes, module caches, and other rebuildable output from local Xcode builds.",
            CategorySafetyLevel::HighConfidence,
            true,
            CleanupRecommendationKind::SafeCleanupCandidate,
            "Safe cleanup candidate when you want to reclaim space or clear stale build state.",
            "Xcode rebuilds these artifacts on the next open, index, or compile.",
            None,
        ),
        fallback_category_spec(
            CleanCategoryId::Archives,
            "Archives",
            "~/Library/Developer/Xcode/Archives",
            "Organizer archives kept for distribution, debugging, symbolication, and release history.",
            CategorySafetyLevel::HighCaution,
            false,
            CleanupRecommendationKind::KeepByDefault,
            "Keep by default and review individual archives before moving anything to Trash.",
            "Removing archives can make historical builds harder to inspect, export, or symbolicate.",
            Some(
                "Archives can still be needed for re-exporting releases or debugging shipped builds.",
            ),
        ),
        fallback_category_spec(
            CleanCategoryId::DeviceSupport,
            "Device Support",
            "~/Library/Developer/Xcode/* DeviceSupport",
            "Downloaded support files for attached iOS, watchOS, and tvOS devices.",
            CategorySafetyLevel::HighCaution,
            false,
            CleanupRecommendationKind::KeepByDefault,
            "Keep by default unless you know you no longer need older platform support files.",
            "Removing support files can delay the next device connection while Xcode downloads them again.",
            Some(
                "Xcode may need to re-download support files before it can debug or mount older device OS versions.",
            ),
        ),
        fallback_category_spec(
            CleanCategoryId::SwiftUIPreviews,
            "SwiftUI Previews",
            "~/Library/Developer/Xcode/UserData/Previews",
            "Generated preview render caches used by SwiftUI canvas previews.",
            CategorySafetyLevel::HighConfidence,
            true,
            CleanupRecommendationKind::SafeCleanupCandidate,
            "Safe cleanup candidate when preview caches have grown large or previews look stale.",
            "SwiftUI previews may take longer on the next load while caches are rebuilt.",
            None,
        ),
        fallback_category_spec(
            CleanCategoryId::Products,
            "Products",
            "~/Library/Developer/Xcode/Products",
            "Generated build products Xcode may keep outside project folders for reuse.",
            CategorySafetyLevel::MediumConfidence,
            false,
            CleanupRecommendationKind::ReviewCarefully,
            "Review carefully before cleanup, especially if you rely on preserved local build outputs.",
            "Xcode can rebuild products, but local build outputs may need to be regenerated.",
            Some("Some products may still be useful for local testing or ad hoc packaging."),
        ),
        fallback_category_spec(
            CleanCategoryId::DocumentationCache,
            "Documentation Cache",
            "~/Library/Developer/Xcode/DocumentationCache",
            "Cached documentation content downloaded or generated for developer documentation browsing.",
            CategorySafetyLevel::HighConfidence,
            true,
            CleanupRecommendationKind::SafeCleanupCandidate,
            "Safe cleanup candidate when reclaiming disk space from stale documentation caches.",
            "Documentation pages may reload or redownload the next time they are opened.",
            None,
        ),
        fallback_category_spec(
            CleanCategoryId::TestLogs,
            "Test Logs",
            "~/Library/Developer/Xcode/DerivedData/**/Logs/Test",
            "Per-project test logs written under DerivedData.",
            CategorySafetyLevel::HighConfidence,
            true,
            CleanupRecommendationKind::SafeCleanupCandidate,
            "Safe cleanup candidate after you no longer need older local test logs.",
            "Old local test logs disappear, but future test runs generate new logs.",
            None,
        ),
        fallback_category_spec(
            CleanCategoryId::ResultBundles,
            "Result Bundles",
            "~/Library/Developer/Xcode/DerivedData/**/*.xcresult",
            "Xcode test and build result bundles that can consume substantial disk space.",
            CategorySafetyLevel::HighConfidence,
            true,
            CleanupRecommendationKind::SafeCleanupCandidate,
            "Safe cleanup candidate when you no longer need older local result bundles.",
            "Historical local result bundles will no longer be available for review.",
            None,
        ),
        fallback_category_spec(
            CleanCategoryId::TemporaryXcodeBuildFolders,
            "Temporary Xcode Build Folders",
            "/private/tmp (xcodebuild-* and TemporaryItems Xcode artifacts only)",
            "Temporary Xcode build and test artifacts found in bounded /private/tmp patterns.",
            CategorySafetyLevel::MediumConfidence,
            false,
            CleanupRecommendationKind::ReviewCarefully,
            "Review carefully before cleanup; this category intentionally avoids broad temporary-file scanning.",
            "In-progress command-line builds or tests may lose temporary intermediates if cleaned too early.",
            Some("Only tightly matched Xcode-related temp artifacts should appear here."),
        ),
    ]
}

fn fallback_category_spec(
    id: CleanCategoryId,
    name: &str,
    display_path: &str,
    description: &str,
    safety: CategorySafetyLevel,
    default_cleanup: bool,
    cleanup_kind: CleanupRecommendationKind,
    recommendation: &str,
    impact: &str,
    caution: Option<&str>,
) -> CategorySpec {
    CategorySpec {
        id,
        name: name.to_string(),
        display_path: display_path.to_string(),
        scan_roots: scan_root_templates(id)
            .into_iter()
            .map(str::to_string)
            .collect(),
        scan_kind: scan_kind_for_category(id),
        metadata: CleanCategoryMetadata {
            description: description.to_string(),
            safety,
            default_cleanup,
            cleanup_kind,
            reversible: true,
            move_to_trash: true,
            caution: caution.map(str::to_string),
            recommendation: recommendation.to_string(),
            impact: impact.to_string(),
        },
    }
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

fn resolve_scan_root(
    profile_path: &str,
    home_dir: &Path,
    tmp_root_override: Option<&Path>,
) -> PathBuf {
    if profile_path == "/private/tmp" {
        return tmp_root_override
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(profile_path));
    }

    expand_profile_path(profile_path, home_dir)
}

fn scan_category(
    spec: &CategorySpec,
    home_dir: &Path,
    tmp_root_override: Option<&Path>,
    now: SystemTime,
    loaded_profile: Option<&LoadedProfile>,
    warnings: &mut Vec<String>,
) -> CleanCategory {
    let roots = spec
        .scan_roots
        .iter()
        .map(|root| resolve_scan_root(root, home_dir, tmp_root_override))
        .collect::<Vec<_>>();
    let profile = loaded_profile.map(|loaded| &loaded.profile);

    let category = match spec.scan_kind {
        ScanKind::ImmediateChildren => scan_immediate_children_category(spec, &roots, now, profile),
        ScanKind::DerivedDataTestLogs => scan_test_logs_category(spec, &roots, now, profile),
        ScanKind::DerivedDataResultBundles => {
            scan_result_bundles_category(spec, &roots, now, profile)
        }
        ScanKind::TemporaryXcodeBuildFolders => {
            scan_temporary_xcode_category(spec, &roots, now, profile)
        }
    };

    warnings.extend(category.warnings.iter().cloned());
    category
}

fn scan_immediate_children_category(
    spec: &CategorySpec,
    roots: &[PathBuf],
    now: SystemTime,
    profile: Option<&CleanerProfile>,
) -> CleanCategory {
    let mut entries = Vec::new();
    let mut category_warnings = Vec::new();
    let mut existing_roots = Vec::new();

    for root in roots {
        let status = inspect_root(root, &mut category_warnings);
        if !status.exists {
            continue;
        }
        if !status.readable {
            existing_roots.push(root.clone());
            continue;
        }

        existing_roots.push(root.clone());
        if let Ok(read_dir) = fs::read_dir(root) {
            for child in read_dir {
                let entry = match child {
                    Ok(entry) => entry,
                    Err(error) => {
                        category_warnings.push(format!(
                            "Could not read child in {}: {}",
                            root.display(),
                            error
                        ));
                        continue;
                    }
                };

                let base_name = entry.file_name().to_string_lossy().into_owned();
                let display_name = if roots.len() > 1 {
                    format!("{} / {}", root_label(root), base_name)
                } else {
                    base_name
                };

                if let Some(clean_entry) = clean_entry_from_path(
                    display_name,
                    entry.path(),
                    root.clone(),
                    now,
                    &spec.metadata,
                    profile,
                    &mut category_warnings,
                ) {
                    entries.push(clean_entry);
                }
            }
        }
    }

    finish_category(
        spec,
        roots,
        existing_roots,
        entries,
        category_warnings,
        None,
    )
}

fn scan_test_logs_category(
    spec: &CategorySpec,
    roots: &[PathBuf],
    now: SystemTime,
    profile: Option<&CleanerProfile>,
) -> CleanCategory {
    let Some(derived_data_root) = roots.first().cloned() else {
        return missing_category(spec, roots);
    };
    let mut category_warnings = Vec::new();
    let status = inspect_root(&derived_data_root, &mut category_warnings);
    if !status.exists {
        return missing_category(spec, roots);
    }

    let mut entries = Vec::new();
    if status.readable {
        if let Ok(read_dir) = fs::read_dir(&derived_data_root) {
            for child in read_dir {
                let entry = match child {
                    Ok(entry) => entry,
                    Err(error) => {
                        category_warnings.push(format!(
                            "Could not read child in {}: {}",
                            derived_data_root.display(),
                            error
                        ));
                        continue;
                    }
                };

                let child_path = entry.path();
                let child_meta = match fs::symlink_metadata(&child_path) {
                    Ok(metadata) => metadata,
                    Err(error) => {
                        category_warnings.push(format!(
                            "Could not inspect {}: {}",
                            child_path.display(),
                            error
                        ));
                        continue;
                    }
                };

                if child_meta.file_type().is_symlink() || !child_meta.is_dir() {
                    continue;
                }

                let logs_test_path = child_path.join("Logs/Test");
                let display_name = format!(
                    "{} / Logs/Test",
                    entry.file_name().to_string_lossy().into_owned()
                );

                if let Some(clean_entry) = clean_entry_from_path(
                    display_name,
                    logs_test_path,
                    derived_data_root.clone(),
                    now,
                    &spec.metadata,
                    profile,
                    &mut category_warnings,
                ) {
                    entries.push(clean_entry);
                }
            }
        }
    }

    finish_category(
        spec,
        roots,
        vec![derived_data_root],
        entries,
        category_warnings,
        Some("No matching test logs found."),
    )
}

fn scan_result_bundles_category(
    spec: &CategorySpec,
    roots: &[PathBuf],
    now: SystemTime,
    profile: Option<&CleanerProfile>,
) -> CleanCategory {
    let Some(derived_data_root) = roots.first().cloned() else {
        return missing_category(spec, roots);
    };
    let mut category_warnings = Vec::new();
    let status = inspect_root(&derived_data_root, &mut category_warnings);
    if !status.exists {
        return missing_category(spec, roots);
    }

    let mut matches = Vec::new();
    if status.readable {
        collect_result_bundle_paths(
            &derived_data_root,
            &derived_data_root,
            &mut matches,
            &mut category_warnings,
        );
    }

    let mut entries = Vec::new();
    for path in matches {
        let relative_name = path
            .strip_prefix(&derived_data_root)
            .ok()
            .map(|value| value.display().to_string())
            .unwrap_or_else(|| path.display().to_string());
        if let Some(clean_entry) = clean_entry_from_path(
            relative_name,
            path,
            derived_data_root.clone(),
            now,
            &spec.metadata,
            profile,
            &mut category_warnings,
        ) {
            entries.push(clean_entry);
        }
    }

    finish_category(
        spec,
        roots,
        vec![derived_data_root],
        entries,
        category_warnings,
        Some("No matching .xcresult bundles found."),
    )
}

fn scan_temporary_xcode_category(
    spec: &CategorySpec,
    roots: &[PathBuf],
    now: SystemTime,
    profile: Option<&CleanerProfile>,
) -> CleanCategory {
    let Some(tmp_root) = roots.first().cloned() else {
        return missing_category(spec, roots);
    };
    let mut category_warnings = Vec::new();
    let status = inspect_root(&tmp_root, &mut category_warnings);
    if !status.exists {
        return missing_category(spec, roots);
    }

    let mut entries = Vec::new();
    if status.readable {
        if let Ok(read_dir) = fs::read_dir(&tmp_root) {
            for child in read_dir {
                let entry = match child {
                    Ok(entry) => entry,
                    Err(error) => {
                        category_warnings.push(format!(
                            "Could not read child in {}: {}",
                            tmp_root.display(),
                            error
                        ));
                        continue;
                    }
                };

                let child_name = entry.file_name().to_string_lossy().into_owned();
                let child_path = entry.path();

                if is_top_level_tmp_xcode_artifact(&child_name) {
                    if let Some(clean_entry) = clean_entry_from_path(
                        child_name,
                        child_path,
                        tmp_root.clone(),
                        now,
                        &spec.metadata,
                        profile,
                        &mut category_warnings,
                    ) {
                        entries.push(clean_entry);
                    }
                    continue;
                }

                if child_name != "TemporaryItems" {
                    continue;
                }

                let temp_items_meta = match fs::symlink_metadata(&child_path) {
                    Ok(metadata) => metadata,
                    Err(error) => {
                        category_warnings.push(format!(
                            "Could not inspect {}: {}",
                            child_path.display(),
                            error
                        ));
                        continue;
                    }
                };

                if temp_items_meta.file_type().is_symlink() || !temp_items_meta.is_dir() {
                    continue;
                }

                let temp_items_dir = match fs::read_dir(&child_path) {
                    Ok(read_dir) => read_dir,
                    Err(error) => {
                        category_warnings.push(format!(
                            "Could not read {}: {}",
                            child_path.display(),
                            error
                        ));
                        continue;
                    }
                };

                for nested in temp_items_dir {
                    let nested = match nested {
                        Ok(nested) => nested,
                        Err(error) => {
                            category_warnings.push(format!(
                                "Could not read child in {}: {}",
                                child_path.display(),
                                error
                            ));
                            continue;
                        }
                    };

                    let nested_name = nested.file_name().to_string_lossy().into_owned();
                    if !is_temporary_items_xcode_artifact(&nested_name) {
                        continue;
                    }

                    if let Some(clean_entry) = clean_entry_from_path(
                        format!("TemporaryItems / {nested_name}"),
                        nested.path(),
                        tmp_root.clone(),
                        now,
                        &spec.metadata,
                        profile,
                        &mut category_warnings,
                    ) {
                        entries.push(clean_entry);
                    }
                }
            }
        }
    }

    finish_category(
        spec,
        roots,
        vec![tmp_root],
        entries,
        category_warnings,
        Some("No bounded Xcode temp artifacts found."),
    )
}

fn finish_category(
    spec: &CategorySpec,
    roots: &[PathBuf],
    existing_roots: Vec<PathBuf>,
    mut entries: Vec<CleanEntry>,
    category_warnings: Vec<String>,
    empty_note: Option<&str>,
) -> CleanCategory {
    sort_entries(&mut entries);

    let total_size_bytes = entries.iter().map(|entry| entry.size_bytes).sum();
    let total_file_count = entries.iter().map(|entry| entry.file_count).sum();
    let note = if existing_roots.is_empty() {
        Some("path missing".to_string())
    } else if !category_warnings.is_empty() {
        Some(format!("{} warning(s)", category_warnings.len()))
    } else if entries.is_empty() {
        empty_note
            .map(str::to_string)
            .or_else(|| Some("empty".to_string()))
    } else {
        None
    };

    CleanCategory {
        id: spec.id,
        name: spec.name.clone(),
        stats_key: Some(category_stats_key(spec.id)),
        path: existing_roots
            .first()
            .cloned()
            .or_else(|| roots.first().cloned())
            .unwrap_or_else(|| PathBuf::from(&spec.display_path)),
        roots: roots.to_vec(),
        exists: !existing_roots.is_empty(),
        note,
        warnings: category_warnings,
        entries,
        total_size_bytes,
        total_file_count,
        metadata: Some(spec.metadata.clone()),
    }
}

fn sort_entries(entries: &mut [CleanEntry]) {
    entries.sort_by(|left, right| {
        right
            .age
            .stale_bucket
            .sort_rank()
            .cmp(&left.age.stale_bucket.sort_rank())
            .then_with(|| {
                right
                    .age
                    .age_seconds
                    .unwrap_or(0)
                    .cmp(&left.age.age_seconds.unwrap_or(0))
            })
            .then_with(|| right.size_bytes.cmp(&left.size_bytes))
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.path.cmp(&right.path))
    });
}

fn missing_category(spec: &CategorySpec, roots: &[PathBuf]) -> CleanCategory {
    CleanCategory {
        id: spec.id,
        name: spec.name.clone(),
        stats_key: Some(category_stats_key(spec.id)),
        path: roots
            .first()
            .cloned()
            .unwrap_or_else(|| PathBuf::from(&spec.display_path)),
        roots: roots.to_vec(),
        exists: false,
        note: Some("path missing".to_string()),
        warnings: Vec::new(),
        entries: Vec::new(),
        total_size_bytes: 0,
        total_file_count: 0,
        metadata: Some(spec.metadata.clone()),
    }
}

struct RootStatus {
    exists: bool,
    readable: bool,
}

fn inspect_root(root: &Path, warnings: &mut Vec<String>) -> RootStatus {
    let metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return RootStatus {
                exists: false,
                readable: false,
            };
        }
        Err(error) => {
            warnings.push(format!("Could not inspect {}: {}", root.display(), error));
            return RootStatus {
                exists: true,
                readable: false,
            };
        }
    };

    if metadata.file_type().is_symlink() {
        warnings.push(format!("Skipped symlink category root {}", root.display()));
        return RootStatus {
            exists: true,
            readable: false,
        };
    }

    if !metadata.is_dir() {
        warnings.push(format!("Expected directory at {}", root.display()));
        return RootStatus {
            exists: true,
            readable: false,
        };
    }

    RootStatus {
        exists: true,
        readable: true,
    }
}

fn clean_entry_from_path(
    name: String,
    path: PathBuf,
    allowed_root: PathBuf,
    now: SystemTime,
    category_metadata: &CleanCategoryMetadata,
    profile: Option<&CleanerProfile>,
    warnings: &mut Vec<String>,
) -> Option<CleanEntry> {
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            warnings.push(format!("Could not inspect {}: {}", path.display(), error));
            return None;
        }
    };

    if metadata.file_type().is_symlink() {
        warnings.push(format!("Skipped symlink {}", path.display()));
        return None;
    }

    let metrics = path_metrics(&path, warnings);
    if metrics.file_count == 0 && metadata.is_dir() {
        return None;
    }

    Some(CleanEntry {
        name: name.clone(),
        path,
        allowed_root,
        size_bytes: metrics.size_bytes,
        file_count: metrics.file_count,
        age: entry_age_from_metrics(&metrics, now),
        keep: !category_metadata.default_cleanup,
        generated_selection: false,
        metadata: entry_metadata(&name, profile, category_metadata),
    })
}

fn entry_metadata(
    entry_name: &str,
    profile: Option<&CleanerProfile>,
    category_metadata: &CleanCategoryMetadata,
) -> CleanEntryMetadata {
    if let Some(rule) = profile.and_then(|profile| profile.match_rule(entry_name)) {
        return metadata_from_rule(rule);
    }

    CleanEntryMetadata {
        matched_rule: None,
        description: category_metadata.description.clone(),
        safety: entry_safety_from_category(category_metadata.cleanup_kind),
        recommendation: category_metadata.recommendation.clone(),
        impact: Some(category_metadata.impact.clone()),
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

fn entry_safety_from_category(cleanup_kind: CleanupRecommendationKind) -> SafetyLevel {
    match cleanup_kind {
        CleanupRecommendationKind::SafeCleanupCandidate => SafetyLevel::Recommended,
        CleanupRecommendationKind::ReviewCarefully => SafetyLevel::Caution,
        CleanupRecommendationKind::KeepByDefault => SafetyLevel::Protected,
    }
}

fn entry_age_from_metrics(metrics: &crate::size::PathMetrics, now: SystemTime) -> EntryAge {
    let Some(last_modified_unix_seconds) = metrics.last_modified_unix_seconds else {
        return EntryAge {
            last_modified_unix_seconds: None,
            last_modified_label: "Unknown".to_string(),
            age_seconds: None,
            age_label: "Unknown".to_string(),
            stale_bucket: StaleBucket::Unknown,
        };
    };

    let last_modified_time = UNIX_EPOCH + Duration::from_secs(last_modified_unix_seconds);
    let age_seconds = now
        .duration_since(last_modified_time)
        .ok()
        .map(|duration| duration.as_secs());

    EntryAge {
        last_modified_unix_seconds: Some(last_modified_unix_seconds),
        last_modified_label: format_unix_date(last_modified_unix_seconds),
        age_label: age_seconds
            .map(format_age_label)
            .unwrap_or_else(|| "Unknown".to_string()),
        stale_bucket: age_seconds
            .map(stale_bucket_for_age)
            .unwrap_or(StaleBucket::Unknown),
        age_seconds,
    }
}

fn stale_bucket_for_age(age_seconds: u64) -> StaleBucket {
    const DAY: u64 = 24 * 60 * 60;
    if age_seconds < 2 * DAY {
        StaleBucket::Fresh
    } else if age_seconds < 14 * DAY {
        StaleBucket::Recent
    } else if age_seconds < 90 * DAY {
        StaleBucket::Stale
    } else {
        StaleBucket::VeryStale
    }
}

fn format_age_label(age_seconds: u64) -> String {
    const DAY: u64 = 24 * 60 * 60;
    if age_seconds < DAY {
        return "Today".to_string();
    }

    let days = age_seconds / DAY;
    if days < 7 {
        return pluralized(days, "day");
    }

    if days < 30 {
        return pluralized(days / 7, "week");
    }

    if days < 365 {
        return pluralized(days / 30, "month");
    }

    pluralized(days / 365, "year")
}

fn pluralized(value: u64, unit: &str) -> String {
    if value == 1 {
        format!("1 {unit}")
    } else {
        format!("{value} {unit}s")
    }
}

fn format_unix_date(unix_seconds: u64) -> String {
    let days_since_epoch = unix_seconds / 86_400;
    let (year, month, day) = civil_from_days(days_since_epoch as i64);
    format!("{year:04}-{month:02}-{day:02}")
}

fn civil_from_days(days_since_epoch: i64) -> (i32, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    (year as i32, m as u32, d as u32)
}

fn category_stats_key(category_id: CleanCategoryId) -> String {
    let suffix = match category_id {
        CleanCategoryId::DerivedData => "derivedData",
        CleanCategoryId::Archives => "archives",
        CleanCategoryId::DeviceSupport => "deviceSupport",
        CleanCategoryId::SwiftUIPreviews => "swiftUIPreviews",
        CleanCategoryId::Products => "products",
        CleanCategoryId::DocumentationCache => "documentationCache",
        CleanCategoryId::TestLogs => "testLogs",
        CleanCategoryId::ResultBundles => "resultBundles",
        CleanCategoryId::TemporaryXcodeBuildFolders => "temporaryXcodeBuildFolders",
    };

    format!("xcode.{suffix}")
}

fn root_label(root: &Path) -> String {
    root.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("root")
        .to_string()
}

fn collect_result_bundle_paths(
    base_root: &Path,
    current: &Path,
    matches: &mut Vec<PathBuf>,
    warnings: &mut Vec<String>,
) {
    let metadata = match fs::symlink_metadata(current) {
        Ok(metadata) => metadata,
        Err(error) => {
            warnings.push(format!(
                "Could not inspect {}: {}",
                current.display(),
                error
            ));
            return;
        }
    };

    if metadata.file_type().is_symlink() {
        warnings.push(format!("Skipped symlink {}", current.display()));
        return;
    }

    if current != base_root
        && current
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("xcresult"))
    {
        matches.push(current.to_path_buf());
        return;
    }

    if !metadata.is_dir() {
        return;
    }

    let read_dir = match fs::read_dir(current) {
        Ok(read_dir) => read_dir,
        Err(error) => {
            warnings.push(format!("Could not read {}: {}", current.display(), error));
            return;
        }
    };

    for child in read_dir {
        match child {
            Ok(entry) => collect_result_bundle_paths(base_root, &entry.path(), matches, warnings),
            Err(error) => warnings.push(format!(
                "Could not read child in {}: {}",
                current.display(),
                error
            )),
        }
    }
}

fn is_top_level_tmp_xcode_artifact(name: &str) -> bool {
    name.starts_with("xcodebuild-") || name.ends_with(".xcresult")
}

fn is_temporary_items_xcode_artifact(name: &str) -> bool {
    name.starts_with("xcodebuild-")
        || name.ends_with(".xcresult")
        || name.to_ascii_lowercase().contains("xcode")
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    use crate::profile::{CategorySafetyLevel, CleanupRecommendationKind, SafetyLevel};
    use crate::sources::{CleanEntry, CleanEntryMetadata, EntryAge, StaleBucket};

    use super::{
        allowed_roots_for_home_with_tmp_root, entry_age_from_metrics, entry_metadata,
        format_age_label, load_xcode_profile, scan_for_home, scan_for_home_with_tmp_root,
        scan_for_home_with_tmp_root_and_now, sort_entries, stale_bucket_for_age,
    };

    fn temp_home(name: &str) -> PathBuf {
        let unique = format!(
            "cleanroom-xcode-scan-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock should be valid")
                .as_nanos()
        );
        let root = std::env::temp_dir().join(unique);
        fs::create_dir_all(&root).expect("temp home should be created");
        root
    }

    fn write_file(path: &PathBuf, bytes: usize) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("parent should exist");
        }
        fs::write(path, vec![b'x'; bytes]).expect("fixture file should write");
    }

    fn dummy_entry(name: &str, size_bytes: u64, age_seconds: Option<u64>) -> CleanEntry {
        let age = age_seconds.map_or_else(EntryAge::default, |age_seconds| EntryAge {
            last_modified_unix_seconds: Some(1),
            last_modified_label: "1970-01-01".to_string(),
            age_seconds: Some(age_seconds),
            age_label: format_age_label(age_seconds),
            stale_bucket: stale_bucket_for_age(age_seconds),
        });

        CleanEntry {
            name: name.to_string(),
            path: PathBuf::from(format!("/tmp/{name}")),
            allowed_root: PathBuf::from("/tmp"),
            size_bytes,
            file_count: 1,
            age,
            keep: false,
            generated_selection: false,
            metadata: CleanEntryMetadata {
                matched_rule: None,
                description: "Generated artifact.".to_string(),
                safety: SafetyLevel::Recommended,
                recommendation: "Review.".to_string(),
                impact: None,
            },
        }
    }

    #[test]
    fn category_metadata_lookup_uses_bundled_profile() {
        let loaded = load_xcode_profile().expect("xcode profile should load");
        let derived_data = loaded
            .category_specs
            .iter()
            .find(|category| category.name == "Derived Data")
            .expect("derived data category should exist");

        let metadata = &derived_data.metadata;
        assert_eq!(metadata.safety, CategorySafetyLevel::HighConfidence);
        assert_eq!(
            metadata.cleanup_kind,
            CleanupRecommendationKind::SafeCleanupCandidate
        );
        assert!(metadata.default_cleanup);
    }

    #[test]
    fn archives_and_device_support_are_high_caution_and_keep_by_default() {
        let loaded = load_xcode_profile().expect("xcode profile should load");
        let archives = loaded
            .category_specs
            .iter()
            .find(|category| category.id == crate::sources::CleanCategoryId::Archives)
            .expect("archives category should exist");
        let device_support = loaded
            .category_specs
            .iter()
            .find(|category| category.id == crate::sources::CleanCategoryId::DeviceSupport)
            .expect("device support category should exist");

        assert_eq!(archives.metadata.safety, CategorySafetyLevel::HighCaution);
        assert!(!archives.metadata.default_cleanup);
        assert_eq!(
            device_support.metadata.safety,
            CategorySafetyLevel::HighCaution
        );
        assert!(!device_support.metadata.default_cleanup);
    }

    #[test]
    fn missing_directory_produces_zero_size_category_without_warning() {
        let home = temp_home("missing");
        let scan = scan_for_home(&home);
        let derived_data = scan
            .categories
            .iter()
            .find(|category| category.name == "Derived Data")
            .expect("derived data category should exist");

        assert!(!derived_data.exists);
        assert_eq!(derived_data.total_size_bytes, 0);
        assert_eq!(derived_data.total_file_count, 0);
        assert!(derived_data.warnings.is_empty());
    }

    #[test]
    fn age_metadata_from_recent_temp_files_is_available() {
        let home = temp_home("age-now");
        write_file(
            &home.join("Library/Developer/Xcode/DerivedData/AppA/Build/file1"),
            10,
        );

        let scan = scan_for_home(&home);
        let entry = scan
            .categories
            .iter()
            .find(|category| category.name == "Derived Data")
            .and_then(|category| category.entries.first())
            .expect("derived data entry should exist");

        assert!(entry.age.last_modified_unix_seconds.is_some());
        assert_ne!(entry.age.last_modified_label, "Unknown");
        assert_ne!(entry.age.age_label, "Unknown");
        assert_eq!(entry.age.stale_bucket, StaleBucket::Fresh);
    }

    #[test]
    fn scanner_groups_known_roots_and_aggregates_size_and_file_counts() {
        let home = temp_home("grouping");
        write_file(
            &home.join("Library/Developer/Xcode/DerivedData/AppA/Build/file1"),
            10,
        );
        write_file(
            &home.join("Library/Developer/Xcode/DerivedData/AppA/Logs/Test/log1.txt"),
            20,
        );
        write_file(
            &home.join("Library/Developer/Xcode/DerivedData/AppA/Logs/Test/Run-AppA.xcresult/data"),
            30,
        );
        write_file(
            &home.join("Library/Developer/Xcode/Archives/2026-06-14/App.xcarchive/info.plist"),
            40,
        );
        write_file(
            &home.join("Library/Developer/Xcode/iOS DeviceSupport/17.5/support"),
            50,
        );
        write_file(
            &home.join("Library/Developer/Xcode/watchOS DeviceSupport/10.0/support"),
            60,
        );
        write_file(
            &home.join("Library/Developer/Xcode/UserData/Previews/cache/a"),
            70,
        );
        write_file(
            &home.join("Library/Developer/Xcode/Products/App.app/binary"),
            80,
        );
        write_file(
            &home.join("Library/Developer/Xcode/DocumentationCache/doc.db"),
            90,
        );
        write_file(&home.join("private/tmp/xcodebuild-123/output"), 15);
        write_file(
            &home.join("private/tmp/TemporaryItems/xcode-cache-fragment/tmp"),
            25,
        );
        write_file(
            &home.join("Library/Unrelated/ShouldNotScan/huge.bin"),
            1_000,
        );

        let scan = scan_for_home_with_tmp_root(&home, Some(&home.join("private/tmp")));

        let derived_data = scan
            .categories
            .iter()
            .find(|category| category.name == "Derived Data")
            .expect("derived data category should exist");
        assert_eq!(derived_data.total_size_bytes, 60);
        assert_eq!(derived_data.total_file_count, 3);
        assert_eq!(derived_data.reclaimable_size_bytes(), 60);

        let test_logs = scan
            .categories
            .iter()
            .find(|category| category.name == "Test Logs")
            .expect("test logs category should exist");
        assert_eq!(test_logs.total_size_bytes, 50);
        assert_eq!(test_logs.total_file_count, 2);

        let result_bundles = scan
            .categories
            .iter()
            .find(|category| category.name == "Result Bundles")
            .expect("result bundles category should exist");
        assert_eq!(result_bundles.total_size_bytes, 30);
        assert_eq!(result_bundles.total_file_count, 1);
        assert!(result_bundles.entries[0].name.ends_with(".xcresult"));

        let device_support = scan
            .categories
            .iter()
            .find(|category| category.name == "Device Support")
            .expect("device support category should exist");
        assert_eq!(device_support.total_size_bytes, 110);
        assert_eq!(device_support.total_file_count, 2);
        assert_eq!(device_support.roots.len(), 3);

        let temp = scan
            .categories
            .iter()
            .find(|category| category.name == "Temporary Xcode Build Folders")
            .expect("temporary category should exist");
        assert_eq!(temp.total_size_bytes, 40);
        assert_eq!(temp.total_file_count, 2);

        let total_scanned: u64 = scan
            .categories
            .iter()
            .map(|category| category.total_size_bytes)
            .sum();
        assert_eq!(total_scanned, 60 + 40 + 110 + 70 + 80 + 90 + 50 + 30 + 40);
    }

    #[test]
    fn synthetic_old_timestamps_produce_stale_buckets() {
        let now = UNIX_EPOCH + Duration::from_secs(200 * 24 * 60 * 60);
        let metrics = crate::size::PathMetrics {
            size_bytes: 10,
            file_count: 1,
            last_modified_unix_seconds: Some(10 * 24 * 60 * 60),
        };

        let age = entry_age_from_metrics(&metrics, now);

        assert_eq!(age.age_label, "6 months");
        assert_eq!(age.stale_bucket, StaleBucket::VeryStale);
        assert_eq!(age.last_modified_label, "1970-01-11");
    }

    #[test]
    fn unknown_age_is_handled_without_crashing() {
        let age = entry_age_from_metrics(&crate::size::PathMetrics::default(), SystemTime::now());

        assert_eq!(age.age_label, "Unknown");
        assert_eq!(age.last_modified_label, "Unknown");
        assert_eq!(age.stale_bucket, StaleBucket::Unknown);
    }

    #[test]
    fn entries_sort_stale_first_then_size_then_name() {
        let mut entries = vec![
            dummy_entry("beta", 100, Some(120 * 24 * 60 * 60)),
            dummy_entry("alpha", 200, Some(120 * 24 * 60 * 60)),
            dummy_entry("fresh", 999, Some(60 * 60)),
            dummy_entry("unknown", 500, None),
        ];

        sort_entries(&mut entries);

        assert_eq!(entries[0].name, "alpha");
        assert_eq!(entries[1].name, "beta");
        assert_eq!(entries[2].name, "fresh");
        assert_eq!(entries[3].name, "unknown");
    }

    #[test]
    fn size_tiebreaker_is_stable_with_alphabetical_name_order() {
        let mut entries = vec![
            dummy_entry("beta", 100, Some(30 * 24 * 60 * 60)),
            dummy_entry("alpha", 100, Some(30 * 24 * 60 * 60)),
        ];

        sort_entries(&mut entries);

        assert_eq!(entries[0].name, "alpha");
        assert_eq!(entries[1].name, "beta");
    }

    #[test]
    fn derived_data_entries_are_default_cleanup_candidates_when_policy_allows() {
        let home = temp_home("defaults");
        write_file(
            &home.join("Library/Developer/Xcode/DerivedData/AppA/Build/file1"),
            10,
        );

        let scan = scan_for_home(&home);
        let derived_data = scan
            .categories
            .iter()
            .find(|category| category.name == "Derived Data")
            .expect("derived data category should exist");

        assert!(!derived_data.entries[0].keep);
        assert_eq!(
            derived_data
                .metadata
                .as_ref()
                .map(|metadata| metadata.cleanup_kind),
            Some(CleanupRecommendationKind::SafeCleanupCandidate)
        );
    }

    #[test]
    fn stale_bucket_thresholds_are_human_readable() {
        assert_eq!(format_age_label(3 * 24 * 60 * 60), "3 days");
        assert_eq!(format_age_label(21 * 24 * 60 * 60), "3 weeks");
        assert_eq!(format_age_label(180 * 24 * 60 * 60), "6 months");
    }

    #[test]
    fn scanner_can_use_deterministic_now_for_tests() {
        let home = temp_home("deterministic-now");
        write_file(
            &home.join("Library/Developer/Xcode/DerivedData/AppA/Build/file1"),
            10,
        );
        let now = SystemTime::now() + Duration::from_secs(20 * 24 * 60 * 60);

        let scan = scan_for_home_with_tmp_root_and_now(&home, None, now);
        let entry = scan
            .categories
            .iter()
            .find(|category| category.name == "Derived Data")
            .and_then(|category| category.entries.first())
            .expect("derived data entry should exist");

        assert!(matches!(
            entry.age.stale_bucket,
            StaleBucket::Stale | StaleBucket::Recent
        ));
    }

    #[test]
    fn entry_rule_metadata_still_matches_known_artifacts() {
        let loaded = load_xcode_profile().expect("xcode profile should load");
        let metadata = entry_metadata(
            "ModuleCache.noindex",
            Some(&loaded.profile),
            &loaded.category_specs[0].metadata,
        );

        assert_eq!(metadata.safety, SafetyLevel::Rebuildable);
        assert_eq!(
            metadata.matched_rule.as_deref(),
            Some("ModuleCache.noindex")
        );
    }

    #[test]
    fn unmatched_entries_fall_back_to_category_recommendation() {
        let loaded = load_xcode_profile().expect("xcode profile should load");
        let metadata = entry_metadata(
            "NotAKnownArtifact",
            Some(&loaded.profile),
            &loaded.category_specs[0].metadata,
        );

        assert_eq!(metadata.safety, SafetyLevel::Recommended);
        assert!(metadata.matched_rule.is_none());
    }

    #[test]
    fn allowed_roots_are_bounded_to_known_xcode_locations() {
        let home = temp_home("roots");
        let roots = allowed_roots_for_home_with_tmp_root(&home, Some(&home.join("private/tmp")));

        assert!(roots.iter().all(|root| {
            let value = root.display().to_string();
            value.contains("Library/Developer/Xcode") || value.ends_with("/private/tmp")
        }));
        assert!(!roots.iter().any(|root| root.ends_with("Library/Unrelated")));
    }
}
