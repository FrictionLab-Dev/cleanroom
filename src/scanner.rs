use crate::sources::{CleanupPlan, CleanupPreviewItem, ScannedSource, xcode};

pub fn scan_xcode() -> ScannedSource {
    xcode::scan()
}

pub fn build_cleanup_plan(scan: &ScannedSource) -> CleanupPlan {
    let mut preview_items = Vec::new();
    let mut total_reclaimable_bytes = 0_u64;
    let mut removal_count = 0_usize;
    let mut warnings = scan.warnings.clone();

    for category in &scan.categories {
        warnings.extend(category.warnings.iter().cloned());
        for entry in &category.entries {
            if entry.keep {
                continue;
            }

            removal_count += 1;
            total_reclaimable_bytes += entry.size_bytes;
            preview_items.push(CleanupPreviewItem {
                category_name: category.name.clone(),
                category_key: category.stats_key.clone(),
                entry_name: entry.name.clone(),
                size_bytes: entry.size_bytes,
                path: entry.path.clone(),
                allowed_root: category.path.clone(),
            });
        }
    }

    CleanupPlan {
        source_name: scan.source_name.to_string(),
        profile_key: scan.profile_key.clone(),
        total_reclaimable_bytes,
        removal_count,
        preview_items,
        warnings,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::{
        profile::SafetyLevel,
        sources::{
            CleanCategory, CleanCategoryId, CleanCategoryMetadata, CleanEntry, CleanEntryMetadata,
            CleanSourceId, ScannedSource,
        },
    };

    use super::build_cleanup_plan;

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
                exists: true,
                note: None,
                warnings: Vec::new(),
                entries: vec![
                    CleanEntry {
                        name: "keep-me".to_string(),
                        path: PathBuf::from(
                            "/tmp/home/Library/Developer/Xcode/DerivedData/keep-me",
                        ),
                        size_bytes: 10,
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
                        size_bytes: 20,
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
                metadata: Some(CleanCategoryMetadata {
                    description: "Build artifacts.".to_string(),
                    safety: SafetyLevel::Rebuildable,
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
    }
}
