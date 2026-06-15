use crate::{
    profile::CategorySafetyLevel,
    sources::{CleanCategoryId, CleanupPlan, CleanupPreviewItem, ScannedSource, xcode},
};

pub fn scan_xcode() -> ScannedSource {
    xcode::scan()
}

pub fn build_cleanup_plan(scan: &ScannedSource) -> CleanupPlan {
    let mut preview_items = Vec::new();
    let mut total_reclaimable_bytes = 0_u64;
    let mut removal_count = 0_usize;
    let mut high_caution_categories = Vec::new();
    let mut warnings = scan.warnings.clone();

    for category in &scan.categories {
        warnings.extend(category.warnings.iter().cloned());

        let is_high_caution = category
            .metadata
            .as_ref()
            .is_some_and(|metadata| metadata.safety == CategorySafetyLevel::HighCaution);
        if is_high_caution && category.remove_count() > 0 {
            high_caution_categories.push(category.name.clone());
        }

        for entry in &category.entries {
            if entry.keep {
                continue;
            }

            removal_count += 1;
            total_reclaimable_bytes += entry.size_bytes;
            preview_items.push(CleanupPreviewItem {
                category_id: category.id,
                category_name: category.name.clone(),
                category_key: category.stats_key.clone(),
                entry_name: entry.name.clone(),
                size_bytes: entry.size_bytes,
                file_count: entry.file_count,
                age: entry.age.clone(),
                high_caution: is_high_caution,
                path: entry.path.clone(),
                allowed_root: entry.allowed_root.clone(),
            });
        }
    }

    preview_items.sort_by(|left, right| {
        right
            .high_caution
            .cmp(&left.high_caution)
            .then_with(|| {
                right
                    .age
                    .stale_bucket
                    .sort_rank()
                    .cmp(&left.age.stale_bucket.sort_rank())
            })
            .then_with(|| {
                right
                    .age
                    .age_seconds
                    .unwrap_or(0)
                    .cmp(&left.age.age_seconds.unwrap_or(0))
            })
            .then_with(|| right.size_bytes.cmp(&left.size_bytes))
            .then_with(|| left.entry_name.cmp(&right.entry_name))
    });

    let high_caution_phrase = high_caution_phrase(&preview_items);

    CleanupPlan {
        source_name: scan.source_name.to_string(),
        profile_key: scan.profile_key.clone(),
        total_reclaimable_bytes,
        removal_count,
        preview_items,
        high_caution_categories,
        high_caution_phrase,
        warnings,
    }
}

fn high_caution_phrase(items: &[CleanupPreviewItem]) -> Option<String> {
    let has_archives = items
        .iter()
        .any(|item| item.high_caution && item.category_id == CleanCategoryId::Archives);
    let has_device_support = items
        .iter()
        .any(|item| item.high_caution && item.category_id == CleanCategoryId::DeviceSupport);

    match (has_archives, has_device_support) {
        (true, true) => Some("CLEAN HIGH CAUTION".to_string()),
        (true, false) => Some("CLEAN ARCHIVES".to_string()),
        (false, true) => Some("CLEAN DEVICE SUPPORT".to_string()),
        (false, false) => None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::{
        profile::{CategorySafetyLevel, CleanupRecommendationKind, SafetyLevel},
        sources::{
            CleanCategory, CleanCategoryId, CleanCategoryMetadata, CleanEntry, CleanEntryMetadata,
            CleanSourceId, CleanupPreviewItem, EntryAge, ScannedSource, StaleBucket,
        },
    };

    use super::{build_cleanup_plan, high_caution_phrase};

    #[test]
    fn cleanup_plan_only_contains_entries_marked_for_cleanup() {
        let scan = ScannedSource {
            source_id: CleanSourceId::Xcode,
            source_name: "Xcode",
            profile_key: "xcode".to_string(),
            root_hint: PathBuf::from("/tmp/home/Library/Developer"),
            categories: vec![CleanCategory {
                id: CleanCategoryId::DerivedData,
                name: "DerivedData".to_string(),
                stats_key: Some("xcode.derivedData".to_string()),
                path: PathBuf::from("/tmp/home/Library/Developer/Xcode/DerivedData"),
                roots: vec![PathBuf::from(
                    "/tmp/home/Library/Developer/Xcode/DerivedData",
                )],
                exists: true,
                note: None,
                warnings: Vec::new(),
                entries: vec![
                    CleanEntry {
                        name: "keep-me".to_string(),
                        path: PathBuf::from(
                            "/tmp/home/Library/Developer/Xcode/DerivedData/keep-me",
                        ),
                        allowed_root: PathBuf::from(
                            "/tmp/home/Library/Developer/Xcode/DerivedData",
                        ),
                        size_bytes: 10,
                        file_count: 1,
                        age: EntryAge {
                            last_modified_unix_seconds: Some(1),
                            last_modified_label: "1970-01-01".to_string(),
                            age_seconds: Some(10),
                            age_label: "Today".to_string(),
                            stale_bucket: StaleBucket::Fresh,
                        },
                        keep: true,
                        metadata: CleanEntryMetadata {
                            matched_rule: None,
                            description: "Unknown generated artifact.".to_string(),
                            safety: SafetyLevel::Unknown,
                            recommendation: "Inspect before cleaning.".to_string(),
                            impact: None,
                        },
                    },
                    CleanEntry {
                        name: "remove-me".to_string(),
                        path: PathBuf::from(
                            "/tmp/home/Library/Developer/Xcode/DerivedData/remove-me",
                        ),
                        allowed_root: PathBuf::from(
                            "/tmp/home/Library/Developer/Xcode/DerivedData",
                        ),
                        size_bytes: 20,
                        file_count: 2,
                        age: EntryAge {
                            last_modified_unix_seconds: Some(2),
                            last_modified_label: "1970-01-01".to_string(),
                            age_seconds: Some(100_000),
                            age_label: "1 day".to_string(),
                            stale_bucket: StaleBucket::Recent,
                        },
                        keep: false,
                        metadata: CleanEntryMetadata {
                            matched_rule: Some("ModuleCache.noindex".to_string()),
                            description: "Known rebuildable cache.".to_string(),
                            safety: SafetyLevel::Rebuildable,
                            recommendation: "Safe to remove.".to_string(),
                            impact: Some("Cache is rebuilt on demand.".to_string()),
                        },
                    },
                ],
                total_size_bytes: 30,
                total_file_count: 3,
                metadata: Some(CleanCategoryMetadata {
                    description: "Build artifacts.".to_string(),
                    safety: CategorySafetyLevel::HighConfidence,
                    default_cleanup: true,
                    cleanup_kind: CleanupRecommendationKind::SafeCleanupCandidate,
                    reversible: true,
                    move_to_trash: true,
                    caution: None,
                    recommendation: "Usually safe to clean.".to_string(),
                    impact: "Builds may regenerate caches.".to_string(),
                }),
            }],
            warnings: Vec::new(),
        };

        let plan = build_cleanup_plan(&scan);

        assert_eq!(plan.removal_count, 1);
        assert_eq!(plan.total_reclaimable_bytes, 20);
        assert_eq!(plan.preview_items.len(), 1);
        assert_eq!(plan.profile_key, "xcode");
        assert_eq!(
            plan.preview_items[0].category_key.as_deref(),
            Some("xcode.derivedData")
        );
        assert_eq!(plan.preview_items[0].entry_name, "remove-me");
        assert_eq!(plan.preview_items[0].file_count, 2);
        assert_eq!(plan.preview_items[0].age.age_label, "1 day");
        assert!(!plan.requires_high_caution_confirmation());
    }

    #[test]
    fn high_caution_phrase_depends_on_selected_categories() {
        let archives_item = CleanupPreviewItem {
            category_id: CleanCategoryId::Archives,
            category_name: "Archives".to_string(),
            category_key: Some("xcode.archives".to_string()),
            entry_name: "A".to_string(),
            size_bytes: 10,
            file_count: 1,
            age: EntryAge::default(),
            high_caution: true,
            path: PathBuf::from("/tmp/A"),
            allowed_root: PathBuf::from("/tmp"),
        };
        let device_support_item = CleanupPreviewItem {
            category_id: CleanCategoryId::DeviceSupport,
            category_name: "Device Support".to_string(),
            category_key: Some("xcode.deviceSupport".to_string()),
            entry_name: "B".to_string(),
            size_bytes: 10,
            file_count: 1,
            age: EntryAge::default(),
            high_caution: true,
            path: PathBuf::from("/tmp/B"),
            allowed_root: PathBuf::from("/tmp"),
        };

        assert_eq!(
            high_caution_phrase(std::slice::from_ref(&archives_item)).as_deref(),
            Some("CLEAN ARCHIVES")
        );
        assert_eq!(
            high_caution_phrase(std::slice::from_ref(&device_support_item)).as_deref(),
            Some("CLEAN DEVICE SUPPORT")
        );
        assert_eq!(
            high_caution_phrase(&[archives_item, device_support_item]).as_deref(),
            Some("CLEAN HIGH CAUTION")
        );
    }
}
