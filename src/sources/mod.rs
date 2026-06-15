use crate::profile::{CategorySafetyLevel, CleanupRecommendationKind, SafetyLevel};

pub mod xcode;

use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CleanSourceId {
    #[default]
    Xcode,
    CustomPlaceholder,
}

#[derive(Clone, Debug)]
pub struct CleanSource {
    pub id: CleanSourceId,
    pub name: &'static str,
    pub available: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CleanCategoryId {
    DerivedData,
    Archives,
    DeviceSupport,
    SwiftUIPreviews,
    Products,
    DocumentationCache,
    TestLogs,
    ResultBundles,
    TemporaryXcodeBuildFolders,
}

#[derive(Clone, Debug)]
pub struct CleanEntry {
    pub name: String,
    pub path: PathBuf,
    pub allowed_root: PathBuf,
    pub size_bytes: u64,
    pub file_count: u64,
    pub keep: bool,
    pub metadata: CleanEntryMetadata,
}

#[derive(Clone, Debug)]
pub struct CleanCategory {
    pub id: CleanCategoryId,
    pub name: String,
    pub stats_key: Option<String>,
    pub path: PathBuf,
    pub roots: Vec<PathBuf>,
    pub exists: bool,
    pub note: Option<String>,
    pub warnings: Vec<String>,
    pub entries: Vec<CleanEntry>,
    pub total_size_bytes: u64,
    pub total_file_count: u64,
    pub metadata: Option<CleanCategoryMetadata>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CleanCategoryMetadata {
    pub description: String,
    pub safety: CategorySafetyLevel,
    pub default_cleanup: bool,
    pub cleanup_kind: CleanupRecommendationKind,
    pub reversible: bool,
    pub move_to_trash: bool,
    pub caution: Option<String>,
    pub recommendation: String,
    pub impact: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CleanEntryMetadata {
    pub matched_rule: Option<String>,
    pub description: String,
    pub safety: SafetyLevel,
    pub recommendation: String,
    pub impact: Option<String>,
}

impl CleanCategory {
    pub fn reclaimable_size_bytes(&self) -> u64 {
        self.entries
            .iter()
            .filter(|entry| !entry.keep)
            .map(|entry| entry.size_bytes)
            .sum()
    }

    pub fn reclaimable_file_count(&self) -> u64 {
        self.entries
            .iter()
            .filter(|entry| !entry.keep)
            .map(|entry| entry.file_count)
            .sum()
    }

    pub fn keep_count(&self) -> usize {
        self.entries.iter().filter(|entry| entry.keep).count()
    }

    pub fn remove_count(&self) -> usize {
        self.entries.len().saturating_sub(self.keep_count())
    }
}

#[derive(Clone, Debug)]
pub struct ScannedSource {
    pub source_id: CleanSourceId,
    pub source_name: &'static str,
    pub profile_key: String,
    pub root_hint: PathBuf,
    pub categories: Vec<CleanCategory>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct CleanupPlan {
    pub source_name: String,
    pub profile_key: String,
    pub total_reclaimable_bytes: u64,
    pub removal_count: usize,
    pub preview_items: Vec<CleanupPreviewItem>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct CleanupPreviewItem {
    pub category_name: String,
    pub category_key: Option<String>,
    pub entry_name: String,
    pub size_bytes: u64,
    pub path: PathBuf,
    pub allowed_root: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CleanupMode {
    DryRun,
    MoveToTrash,
}

impl CleanupMode {
    pub fn label(self) -> &'static str {
        match self {
            CleanupMode::DryRun => "dry-run",
            CleanupMode::MoveToTrash => "move-to-trash",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CleanupStatus {
    /// Item was actually moved to Trash (MoveToTrash mode only).
    Moved,
    /// Item passed validation in dry-run mode; nothing was touched.
    DryRunEligible,
    Skipped,
    Failed,
}

#[derive(Clone, Debug)]
pub struct CleanupRecord {
    pub category_name: String,
    pub category_key: Option<String>,
    pub entry_name: String,
    pub path: PathBuf,
    pub size_bytes: u64,
    pub message: String,
}

impl CleanupRecord {
    pub fn new_from_item(item: &CleanupPreviewItem, message: impl Into<String>) -> Self {
        Self {
            category_name: item.category_name.clone(),
            category_key: item.category_key.clone(),
            entry_name: item.entry_name.clone(),
            path: item.path.clone(),
            size_bytes: item.size_bytes,
            message: message.into(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct CleanupExecutionResult {
    pub source_name: String,
    pub profile_key: String,
    pub mode: CleanupMode,
    pub log_path: PathBuf,
    pub moved_count: usize,
    /// Items that passed validation in dry-run mode. Always 0 for MoveToTrash runs.
    pub dry_run_eligible_count: usize,
    pub skipped_count: usize,
    pub failed_count: usize,
    pub cleaned_size_bytes: u64,
    pub moved_items: Vec<CleanupRecord>,
    /// Items that passed validation in dry-run mode. Always empty for MoveToTrash runs.
    pub dry_run_eligible_items: Vec<CleanupRecord>,
    pub skipped_items: Vec<CleanupRecord>,
    pub failed_items: Vec<CleanupRecord>,
    pub warnings: Vec<String>,
}

impl CleanupExecutionResult {
    pub fn new(
        source_name: String,
        profile_key: String,
        mode: CleanupMode,
        log_path: PathBuf,
    ) -> Self {
        Self {
            source_name,
            profile_key,
            mode,
            log_path,
            moved_count: 0,
            dry_run_eligible_count: 0,
            skipped_count: 0,
            failed_count: 0,
            cleaned_size_bytes: 0,
            moved_items: Vec::new(),
            dry_run_eligible_items: Vec::new(),
            skipped_items: Vec::new(),
            failed_items: Vec::new(),
            warnings: Vec::new(),
        }
    }

    pub fn record(&mut self, status: CleanupStatus, record: CleanupRecord) {
        match status {
            CleanupStatus::Moved => {
                self.moved_count += 1;
                self.cleaned_size_bytes += record.size_bytes;
                self.moved_items.push(record);
            }
            CleanupStatus::DryRunEligible => {
                // Track eligible items without incrementing moved_count or cleaned_size_bytes.
                // Nothing was actually moved; the item passed validation only.
                self.dry_run_eligible_count += 1;
                self.dry_run_eligible_items.push(record);
            }
            CleanupStatus::Skipped => {
                self.skipped_count += 1;
                self.warnings.push(record.message.clone());
                self.skipped_items.push(record);
            }
            CleanupStatus::Failed => {
                self.failed_count += 1;
                self.warnings.push(record.message.clone());
                self.failed_items.push(record);
            }
        }
    }
}

pub fn default_sources() -> Vec<CleanSource> {
    vec![
        CleanSource {
            id: CleanSourceId::Xcode,
            name: "Xcode",
            available: true,
        },
        CleanSource {
            id: CleanSourceId::CustomPlaceholder,
            name: "Custom source",
            available: false,
        },
    ]
}
