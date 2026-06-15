use std::{
    env,
    path::{Path, PathBuf},
};

use crate::{
    cleanup,
    profile::{CategorySafetyLevel, CleanupRecommendationKind, SafetyLevel},
    scanner,
    size::format_bytes,
    sources::{
        CleanCategory, CleanCategoryId, CleanSource, CleanSourceId, CleanupExecutionResult,
        CleanupMode, CleanupPlan, ScannedSource, default_sources,
    },
    stats::CleanupStats,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Screen {
    SourceList,
    CategorySummary,
    EntryChecklist,
    PreviewCleanup,
    HighCautionConfirmation,
    ConfirmCleanup,
    CleanupResult,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InteractionMode {
    Normal,
    Action,
}

pub struct App {
    pub screen: Screen,
    pub mode: InteractionMode,
    pub sources: Vec<CleanSource>,
    pub source_selected: usize,
    pub category_selected: usize,
    pub entry_selected: usize,
    pub xcode_scan: Option<ScannedSource>,
    pub cleanup_result: Option<CleanupExecutionResult>,
    pub cleanup_stats: Option<CleanupStats>,
    pub high_caution_confirmation: Option<HighCautionConfirmationState>,
    pub warning: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HighCautionConfirmationState {
    pub phrase: String,
    pub categories: Vec<String>,
    pub typed: String,
}

impl App {
    pub fn new() -> Self {
        Self {
            screen: Screen::SourceList,
            mode: InteractionMode::Normal,
            sources: default_sources(),
            source_selected: 0,
            category_selected: 0,
            entry_selected: 0,
            xcode_scan: None,
            cleanup_result: None,
            cleanup_stats: None,
            high_caution_confirmation: None,
            warning: None,
        }
    }

    pub fn move_up(&mut self) {
        let selected = self.selected_index_mut();
        if *selected > 0 {
            *selected -= 1;
        }
    }

    pub fn move_down(&mut self) {
        let len = self.current_list_len();
        if len == 0 {
            return;
        }

        let selected = self.selected_index_mut();
        if *selected + 1 < len {
            *selected += 1;
        }
    }

    pub fn enter(&mut self) {
        self.mode = InteractionMode::Normal;
        match self.screen {
            Screen::SourceList => self.enter_source(),
            Screen::CategorySummary => self.enter_category(),
            Screen::EntryChecklist
            | Screen::PreviewCleanup
            | Screen::HighCautionConfirmation
            | Screen::ConfirmCleanup
            | Screen::CleanupResult => {}
        }
    }

    pub fn back(&mut self) {
        self.mode = InteractionMode::Normal;
        self.warning = None;
        match self.screen {
            Screen::SourceList => {}
            Screen::CategorySummary => self.screen = Screen::SourceList,
            Screen::EntryChecklist | Screen::PreviewCleanup | Screen::CleanupResult => {
                self.screen = Screen::CategorySummary
            }
            Screen::HighCautionConfirmation => {
                self.high_caution_confirmation = None;
                self.screen = Screen::PreviewCleanup;
            }
            Screen::ConfirmCleanup => self.screen = Screen::PreviewCleanup,
        }
    }

    pub fn rescan_xcode(&mut self) {
        self.mode = InteractionMode::Normal;
        self.warning = None;
        self.cleanup_result = None;
        self.cleanup_stats = None;
        self.high_caution_confirmation = None;
        self.refresh_xcode_scan();
        self.screen = Screen::CategorySummary;
    }

    pub fn show_preview(&mut self) {
        if self.xcode_scan.is_some() {
            self.mode = InteractionMode::Normal;
            self.warning = None;
            self.cleanup_result = None;
            self.cleanup_stats = None;
            self.high_caution_confirmation = None;
            self.screen = Screen::PreviewCleanup;
        }
    }

    pub fn show_confirmation(&mut self) {
        let plan = self.preview_plan();
        if plan.removal_count == 0 {
            self.warning = Some(
                "Nothing is marked for cleanup yet. Toggle entries to REMOVE first.".to_string(),
            );
            return;
        }

        self.mode = InteractionMode::Normal;
        let preflight = cleanup::execute_cleanup(&plan, CleanupMode::DryRun);
        self.warning = if preflight.skipped_count > 0 || preflight.failed_count > 0 {
            Some(format!(
                "Dry run found {} skipped and {} failed preflight item(s). They will be reported safely.",
                preflight.skipped_count, preflight.failed_count
            ))
        } else {
            None
        };
        self.cleanup_result = None;
        self.cleanup_stats = None;
        self.high_caution_confirmation =
            plan.high_caution_phrase
                .clone()
                .map(|phrase| HighCautionConfirmationState {
                    phrase,
                    categories: plan.high_caution_categories.clone(),
                    typed: String::new(),
                });
        self.screen = if self.high_caution_confirmation.is_some() {
            Screen::HighCautionConfirmation
        } else {
            Screen::ConfirmCleanup
        };
    }

    pub fn execute_cleanup(&mut self) {
        let plan = self.preview_plan();
        if plan.removal_count == 0 {
            self.warning = Some(
                "Nothing is marked for cleanup yet. Toggle entries to REMOVE first.".to_string(),
            );
            self.screen = Screen::PreviewCleanup;
            return;
        }

        self.mode = InteractionMode::Normal;
        let mut result = cleanup::execute_cleanup(&plan, CleanupMode::MoveToTrash);
        // Stats are best-effort. Cleanup success/failure is decided by the
        // move-to-Trash flow itself, not by aggregate counter persistence.
        self.cleanup_stats = if result.moved_count > 0 {
            match crate::stats::record_cleanup(&result) {
                Ok(stats) => Some(stats),
                Err(error) => {
                    result
                        .warnings
                        .push(format!("Could not update aggregate stats: {error}"));
                    None
                }
            }
        } else {
            None
        };
        self.cleanup_result = Some(result);
        self.warning = None;
        self.high_caution_confirmation = None;
        self.refresh_xcode_scan();
        self.screen = Screen::CleanupResult;
    }

    pub fn toggle_selected_entry(&mut self) {
        let selected = self.entry_selected;
        if let Some(entry) = self
            .selected_category_mut()
            .and_then(|category| category.entries.get_mut(selected))
        {
            entry.keep = !entry.keep;
        }
    }

    pub fn mark_all_selected_category(&mut self, keep: bool) {
        if let Some(category) = self.selected_category_mut() {
            for entry in &mut category.entries {
                entry.keep = keep;
            }
        }
    }

    pub fn enter_action_mode(&mut self) {
        if self.supports_action_mode() {
            self.mode = InteractionMode::Action;
            self.warning = None;
        }
    }

    pub fn cancel_action_mode(&mut self) {
        self.mode = InteractionMode::Normal;
    }

    pub fn is_action_mode(&self) -> bool {
        self.mode == InteractionMode::Action
    }

    pub fn append_high_caution_confirmation_char(&mut self, ch: char) {
        let Some(state) = self.high_caution_confirmation.as_mut() else {
            return;
        };

        if !matches!(self.screen, Screen::HighCautionConfirmation) {
            return;
        }

        if ch.is_ascii_control() {
            return;
        }

        state.typed.push(ch.to_ascii_uppercase());
        self.warning = None;
    }

    pub fn backspace_high_caution_confirmation(&mut self) {
        if let Some(state) = self.high_caution_confirmation.as_mut() {
            state.typed.pop();
        }
    }

    pub fn submit_high_caution_confirmation(&mut self) {
        let Some(state) = self.high_caution_confirmation.as_ref() else {
            return;
        };

        if state.typed.trim() == state.phrase {
            self.warning = None;
            self.screen = Screen::ConfirmCleanup;
            return;
        }

        self.warning = Some(format!(
            "Typed confirmation must exactly match {}",
            state.phrase
        ));
    }

    pub fn current_screen_title(&self) -> &'static str {
        match self.screen {
            Screen::SourceList => "Source list",
            Screen::CategorySummary => "Xcode categories",
            Screen::EntryChecklist => "Entry checklist",
            Screen::PreviewCleanup => "Cleanup preview",
            Screen::HighCautionConfirmation => "High-caution confirmation",
            Screen::ConfirmCleanup => "Confirm cleanup",
            Screen::CleanupResult => "Cleanup result",
        }
    }

    pub fn current_context_label(&self) -> String {
        match self.screen {
            Screen::SourceList => "Choose a cleanup source".to_string(),
            Screen::CategorySummary => "Xcode".to_string(),
            Screen::EntryChecklist => self
                .selected_category()
                .map(|category| format!("Xcode / {}", category.name))
                .unwrap_or_else(|| "Xcode".to_string()),
            Screen::PreviewCleanup => self
                .xcode_scan
                .as_ref()
                .map(|scan| format!("{} / preview only", source_label(scan.source_id)))
                .unwrap_or_else(|| "Xcode / preview only".to_string()),
            Screen::HighCautionConfirmation => "Xcode / high-caution confirmation".to_string(),
            Screen::ConfirmCleanup => "Xcode / move to Trash confirmation".to_string(),
            Screen::CleanupResult => "Xcode / cleanup result".to_string(),
        }
    }

    pub fn current_warning_line(&self) -> String {
        if let Some(warning) = &self.warning {
            return format!("Warning: {}", warning);
        }

        let warnings = match self.screen {
            Screen::PreviewCleanup | Screen::HighCautionConfirmation | Screen::ConfirmCleanup => {
                self.preview_plan().warnings
            }
            Screen::CleanupResult => self
                .cleanup_result
                .as_ref()
                .map(|result| result.warnings.clone())
                .unwrap_or_default(),
            _ => self.current_warnings(),
        };

        if warnings.is_empty() {
            return divider(64);
        }

        let latest = warnings[warnings.len() - 1].replace('\n', " ");
        format!("Recovered warning ({} total): {}", warnings.len(), latest)
    }

    pub fn source_rows(&self) -> Vec<String> {
        self.sources
            .iter()
            .map(|source| source.name.to_string())
            .collect()
    }

    pub fn category_table_header(&self) -> String {
        format!(
            "{:<18} {:>9} {:>9} {:>7}  {:<16}",
            "Category", "Total", "Selected", "Files", "Safety"
        )
    }

    pub fn category_rows(&self) -> Vec<String> {
        self.xcode_scan
            .as_ref()
            .map(|scan| {
                scan.categories
                    .iter()
                    .map(|category| {
                        format!(
                            "{:<18} {:>9} {:>9} {:>7}  {:<16}",
                            truncate_text(&category.name, 18),
                            format_bytes(category.total_size_bytes),
                            format_bytes(category.reclaimable_size_bytes()),
                            category.total_file_count,
                            category
                                .metadata
                                .as_ref()
                                .map(|metadata| category_safety_badge(metadata.safety))
                                .unwrap_or("Unknown")
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn entry_table_header(&self) -> String {
        format!(
            "{:<4}  {:<24} {:<10} {:>10}",
            "Keep", "Entry", "Age", "Size"
        )
    }

    pub fn entry_rows(&self) -> Vec<String> {
        self.selected_category()
            .map(|category| {
                if category.entries.is_empty() {
                    vec!["No entries found in this category.".to_string()]
                } else {
                    category
                        .entries
                        .iter()
                        .map(|entry| {
                            format!(
                                "{:<4}  {:<24} {:<10} {:>10}",
                                if entry.keep { "✓" } else { "" },
                                truncate_text(&entry.name, 24),
                                truncate_text(&entry.age.age_label, 10),
                                format_bytes(entry.size_bytes)
                            )
                        })
                        .collect()
                }
            })
            .unwrap_or_else(|| vec!["No category selected.".to_string()])
    }

    pub fn preview_rows(&self) -> Vec<String> {
        let plan = self.preview_plan();
        if plan.preview_items.is_empty() {
            return vec![
                "Nothing selected to clean yet. Press r inside a category to mark cleanup candidates."
                    .to_string(),
            ];
        }

        let mut rows = vec![
            "These items will be moved to Trash, not permanently deleted.".to_string(),
            format!(
                "Selected-to-clean size: {} | Candidates: {} | Files: {}",
                format_bytes(plan.total_reclaimable_bytes),
                plan.removal_count,
                self.total_reclaimable_file_count()
            ),
            String::new(),
        ];

        rows.extend(self.preview_policy_lines());
        rows.push(String::new());

        rows.extend(plan.preview_items.iter().map(|item| {
            format!(
                "{} / {} - {} - {} files - {} - {}",
                item.category_name,
                item.entry_name,
                item.age.age_label,
                item.file_count,
                format_bytes(item.size_bytes),
                shorten_path(&item.path, 36)
            )
        }));

        rows
    }

    pub fn confirmation_rows(&self) -> Vec<String> {
        let plan = self.preview_plan();
        vec![
            format!(
                "Selected-to-clean size: {}",
                format_bytes(plan.total_reclaimable_bytes)
            ),
            format!("Cleanup candidates: {}", plan.removal_count),
            format!("Selected files: {}", self.total_reclaimable_file_count()),
            "These items will be moved to Trash, not permanently deleted.".to_string(),
            if plan.requires_high_caution_confirmation() {
                "High-caution typed confirmation was required before this final step.".to_string()
            } else {
                "No high-caution typed confirmation is required for this plan.".to_string()
            },
            "Press y to confirm or n / Esc to cancel.".to_string(),
        ]
    }

    pub fn high_caution_confirmation_rows(&self) -> Vec<String> {
        let Some(state) = &self.high_caution_confirmation else {
            return vec!["No high-caution confirmation is required.".to_string()];
        };

        vec![
            "High-caution cleanup needs a typed confirmation before the final Trash step."
                .to_string(),
            format!("Categories: {}", state.categories.join(", ")),
            format!("Type exactly: {}", state.phrase),
            format!("Current input: {}", state.typed),
            "Press Enter after the phrase, or Esc to cancel.".to_string(),
        ]
    }

    pub fn result_rows(&self) -> Vec<String> {
        let Some(result) = &self.cleanup_result else {
            return vec!["No cleanup result yet.".to_string()];
        };

        let mut rows = vec![
            format!("Moved to Trash: {} entries", result.moved_count),
            format!(
                "Dry-run eligible: {} entries",
                result.dry_run_eligible_count
            ),
            format!("Reclaimed: {}", format_bytes(result.cleaned_size_bytes)),
            format!("Skipped safely: {} entries", result.skipped_count),
            format!("Failed moves: {} entries", result.failed_count),
            format!("Log file: {}", shorten_path(&result.log_path, 52)),
        ];

        if let Some(stats) = &self.cleanup_stats {
            rows.push(format!("Total cleanups: {}", stats.total_cleanups));
            rows.push(format!(
                "All-time cleaned: {}",
                format_bytes(stats.total_bytes_cleaned)
            ));
            rows.push(format!("All-time entries: {}", stats.entries_cleaned));
        }

        for item in result.failed_items.iter().take(4) {
            rows.push(format!(
                "[FAILED] {} / {} - {}",
                item.category_name, item.entry_name, item.message
            ));
        }

        for item in result.skipped_items.iter().take(4) {
            rows.push(format!(
                "[SKIPPED] {} / {} - {}",
                item.category_name, item.entry_name, item.message
            ));
        }

        if result.failed_items.is_empty() && result.skipped_items.is_empty() {
            rows.push("No failures or skip warnings were reported.".to_string());
        }

        rows
    }

    pub fn selected_index(&self) -> Option<usize> {
        match self.screen {
            Screen::SourceList => Some(self.source_selected),
            Screen::CategorySummary => Some(self.category_selected),
            Screen::EntryChecklist => Some(self.entry_selected),
            Screen::PreviewCleanup
            | Screen::HighCautionConfirmation
            | Screen::ConfirmCleanup
            | Screen::CleanupResult => None,
        }
    }

    pub fn footer_lines(&self) -> [&'static str; 2] {
        match (self.screen, self.mode) {
            (Screen::SourceList, InteractionMode::Normal) => {
                ["↑↓ move · Enter select · Tab actions · q quit", ""]
            }
            (Screen::SourceList, InteractionMode::Action) => {
                ["action mode · no actions here · Esc cancel", ""]
            }
            (Screen::CategorySummary, InteractionMode::Normal) => [
                "↑↓ move · Enter open category · Tab actions · Esc back · q quit",
                "",
            ],
            (Screen::CategorySummary, InteractionMode::Action) => {
                ["action mode · c preview · r rescan · Esc cancel", ""]
            }
            (Screen::EntryChecklist, InteractionMode::Normal) => [
                "↑↓ move · Space toggle · Tab actions · Esc back · q quit",
                "",
            ],
            (Screen::EntryChecklist, InteractionMode::Action) => [
                "action mode · a keep all · r remove all · c preview · Esc cancel",
                "",
            ],
            (Screen::PreviewCleanup, InteractionMode::Normal) => {
                ["Tab actions · Esc back · q quit", ""]
            }
            (Screen::PreviewCleanup, InteractionMode::Action) => {
                ["action mode · c confirm cleanup · Esc cancel", ""]
            }
            (Screen::HighCautionConfirmation, _) => [
                "Type the phrase exactly · Enter continue · Backspace edit · Esc cancel · q quit",
                "",
            ],
            (Screen::ConfirmCleanup, _) => ["y move to Trash · n cancel · Esc cancel · q quit", ""],
            (Screen::CleanupResult, _) => ["Esc back · q quit", ""],
        }
    }

    pub fn detail_panel_title(&self) -> String {
        match self.screen {
            Screen::SourceList => self
                .selected_source()
                .map(|source| source.name.to_string())
                .unwrap_or_else(|| "Source".to_string()),
            Screen::CategorySummary => self
                .selected_category()
                .map(|category| category.name.clone())
                .unwrap_or_else(|| "Xcode Summary".to_string()),
            Screen::EntryChecklist => self
                .selected_entry()
                .map(|entry| entry.name.clone())
                .or_else(|| {
                    self.selected_category()
                        .map(|category| category.name.clone())
                })
                .unwrap_or_else(|| "Entry details".to_string()),
            Screen::PreviewCleanup => "Preview".to_string(),
            Screen::HighCautionConfirmation => "Type Confirmation".to_string(),
            Screen::ConfirmCleanup => "Confirmation".to_string(),
            Screen::CleanupResult => "Result".to_string(),
        }
    }

    pub fn detail_panel_lines(&self) -> Vec<String> {
        match self.screen {
            Screen::SourceList => self.source_detail_lines(),
            Screen::CategorySummary => self.category_detail_lines(),
            Screen::EntryChecklist => self.selected_category().map_or_else(
                || vec!["No category selected.".to_string()],
                |category| self.entry_detail_lines(category),
            ),
            Screen::PreviewCleanup => {
                let plan = self.preview_plan();
                let mut lines = vec![
                    format!(
                        "Selected-to-clean size: {}",
                        format_bytes(plan.total_reclaimable_bytes)
                    ),
                    format!("Cleanup candidates: {}", plan.removal_count),
                    format!("Selected files: {}", self.total_reclaimable_file_count()),
                    "These items will be moved to Trash, not permanently deleted.".to_string(),
                ];
                lines.extend(self.preview_policy_lines());
                lines
            }
            Screen::HighCautionConfirmation => self.high_caution_detail_lines(),
            Screen::ConfirmCleanup => {
                let plan = self.preview_plan();
                let mut lines = vec![
                    format!(
                        "Selected-to-clean size: {}",
                        format_bytes(plan.total_reclaimable_bytes)
                    ),
                    format!("Cleanup candidates: {}", plan.removal_count),
                    format!("Selected files: {}", self.total_reclaimable_file_count()),
                    "These items will be moved to Trash, not permanently deleted.".to_string(),
                ];
                lines.extend(self.preview_policy_lines());
                lines
            }
            Screen::CleanupResult => self
                .cleanup_result
                .as_ref()
                .map(|result| self.cleanup_result_detail_lines(result))
                .unwrap_or_else(|| vec!["No cleanup result yet.".to_string()]),
        }
    }

    pub fn current_path_label(&self) -> String {
        match self.screen {
            Screen::SourceList => "Available cleanup sources".to_string(),
            Screen::CategorySummary | Screen::PreviewCleanup | Screen::ConfirmCleanup => self
                .xcode_scan
                .as_ref()
                .map(|scan| shorten_path(&scan.root_hint, 54))
                .unwrap_or_else(|| "No scan yet".to_string()),
            Screen::HighCautionConfirmation => self
                .xcode_scan
                .as_ref()
                .map(|scan| shorten_path(&scan.root_hint, 54))
                .unwrap_or_else(|| "No scan yet".to_string()),
            Screen::EntryChecklist => self
                .selected_category()
                .map(|category| shorten_path(&category.path, 54))
                .unwrap_or_else(|| "No category selected".to_string()),
            Screen::CleanupResult => self
                .cleanup_result
                .as_ref()
                .map(|result| shorten_path(&result.log_path, 54))
                .unwrap_or_else(|| "No cleanup result yet".to_string()),
        }
    }

    fn current_warnings(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        if let Some(scan) = &self.xcode_scan {
            warnings.extend(scan.warnings.iter().cloned());
            for category in &scan.categories {
                warnings.extend(category.warnings.iter().cloned());
            }
        }
        warnings
    }

    fn total_size_bytes(&self) -> u64 {
        self.xcode_scan
            .as_ref()
            .map(|scan| {
                scan.categories
                    .iter()
                    .map(|category| category.total_size_bytes)
                    .sum()
            })
            .unwrap_or(0)
    }

    fn total_file_count(&self) -> u64 {
        self.xcode_scan
            .as_ref()
            .map(|scan| {
                scan.categories
                    .iter()
                    .map(|category| category.total_file_count)
                    .sum()
            })
            .unwrap_or(0)
    }

    fn total_reclaimable_bytes(&self) -> u64 {
        self.xcode_scan
            .as_ref()
            .map(|scan| {
                scan.categories
                    .iter()
                    .map(CleanCategory::reclaimable_size_bytes)
                    .sum()
            })
            .unwrap_or(0)
    }

    fn total_reclaimable_file_count(&self) -> u64 {
        self.xcode_scan
            .as_ref()
            .map(|scan| {
                scan.categories
                    .iter()
                    .map(CleanCategory::reclaimable_file_count)
                    .sum()
            })
            .unwrap_or(0)
    }

    fn preview_plan(&self) -> CleanupPlan {
        self.xcode_scan
            .as_ref()
            .map(scanner::build_cleanup_plan)
            .unwrap_or_default()
    }

    fn selected_index_mut(&mut self) -> &mut usize {
        match self.screen {
            Screen::SourceList => &mut self.source_selected,
            Screen::CategorySummary => &mut self.category_selected,
            Screen::EntryChecklist => &mut self.entry_selected,
            Screen::PreviewCleanup | Screen::ConfirmCleanup | Screen::CleanupResult => {
                &mut self.category_selected
            }
            Screen::HighCautionConfirmation => &mut self.category_selected,
        }
    }

    fn current_list_len(&self) -> usize {
        match self.screen {
            Screen::SourceList => self.sources.len(),
            Screen::CategorySummary => self
                .xcode_scan
                .as_ref()
                .map(|scan| scan.categories.len())
                .unwrap_or(0),
            Screen::EntryChecklist => self
                .selected_category()
                .map(|category| category.entries.len().max(1))
                .unwrap_or(1),
            Screen::PreviewCleanup
            | Screen::HighCautionConfirmation
            | Screen::ConfirmCleanup
            | Screen::CleanupResult => 0,
        }
    }

    fn enter_source(&mut self) {
        match self
            .sources
            .get(self.source_selected)
            .map(|source| source.id)
        {
            Some(CleanSourceId::Xcode) => self.rescan_xcode(),
            Some(CleanSourceId::CustomPlaceholder) => {
                self.warning = Some(
                    "Custom source scanning is not implemented yet. This pass supports Xcode only."
                        .to_string(),
                );
            }
            None => {}
        }
    }

    fn enter_category(&mut self) {
        if self
            .selected_category()
            .map(|category| category.id)
            .is_some_and(is_known_category)
        {
            self.warning = None;
            self.entry_selected = 0;
            self.screen = Screen::EntryChecklist;
        }
    }

    fn selected_category(&self) -> Option<&CleanCategory> {
        self.xcode_scan
            .as_ref()
            .and_then(|scan| scan.categories.get(self.category_selected))
    }

    fn selected_entry(&self) -> Option<&crate::sources::CleanEntry> {
        self.selected_category()
            .and_then(|category| category.entries.get(self.entry_selected))
    }

    fn selected_source(&self) -> Option<&CleanSource> {
        self.sources.get(self.source_selected)
    }

    fn selected_category_mut(&mut self) -> Option<&mut CleanCategory> {
        self.xcode_scan
            .as_mut()
            .and_then(|scan| scan.categories.get_mut(self.category_selected))
    }

    fn refresh_xcode_scan(&mut self) {
        let previous_index = self.category_selected;
        let scan = scanner::scan_xcode();
        let category_len = scan.categories.len();
        self.category_selected = previous_index.min(category_len.saturating_sub(1));
        self.entry_selected = 0;
        self.xcode_scan = Some(scan);
    }

    fn supports_action_mode(&self) -> bool {
        matches!(
            self.screen,
            Screen::SourceList
                | Screen::CategorySummary
                | Screen::EntryChecklist
                | Screen::PreviewCleanup
        )
    }

    fn source_detail_lines(&self) -> Vec<String> {
        match self.selected_source() {
            Some(source) if source.id == CleanSourceId::Xcode => vec![
                "Scan known Xcode cache locations.".to_string(),
                "Supported categories: Derived Data, Archives, Device Support, SwiftUI Previews, Products, Documentation Cache, Test Logs, Result Bundles, and bounded Xcode temp folders."
                    .to_string(),
                format!(
                    "Status: {}",
                    if source.available {
                        "available"
                    } else {
                        "unavailable"
                    }
                ),
            ],
            Some(source) if source.id == CleanSourceId::CustomPlaceholder => vec![
                "Coming soon.".to_string(),
                "Future custom paths and rules.".to_string(),
                format!(
                    "Status: {}",
                    if source.available {
                        "available"
                    } else {
                        "coming soon"
                    }
                ),
            ],
            None => vec!["No source selected.".to_string()],
            Some(_) => vec!["No source selected.".to_string()],
        }
    }

    fn category_detail_lines(&self) -> Vec<String> {
        self.selected_category()
            .map(|category| {
                let mut lines = vec![
                    format!("Category: {}", category.name),
                    format!(
                        "Root status: {}",
                        if category.exists { "available" } else { "missing" }
                    ),
                    format!("Total size: {}", format_bytes(category.total_size_bytes)),
                    format!("Total files: {}", category.total_file_count),
                    format!(
                        "Selected to clean: {}",
                        format_bytes(category.reclaimable_size_bytes())
                    ),
                    format!("Selected files: {}", category.reclaimable_file_count()),
                    format!("Kept entries: {}", category.keep_count()),
                    format!("Cleanup candidates: {}", category.remove_count()),
                    format!(
                        "Oldest entry: {}",
                        category.oldest_entry_age_label().unwrap_or("Unknown")
                    ),
                    format!(
                        "Stale entries: {} stale / {} very stale",
                        category.stale_entry_count(),
                        category.very_stale_entry_count()
                    ),
                ];

                if let Some(metadata) = &category.metadata {
                    lines.push(format!("Safety: {}", category_safety_label(metadata.safety)));
                    lines.push(format!(
                        "Recommendation: {}",
                        cleanup_kind_label(metadata.cleanup_kind)
                    ));
                    lines.push(format!(
                        "Default cleanup: {}",
                        if metadata.default_cleanup {
                            "selected by default"
                        } else {
                            "review first"
                        }
                    ));
                    lines.push(format!(
                        "Cleanup mode: {}",
                        if metadata.move_to_trash && metadata.reversible {
                            "move to Trash"
                        } else {
                            "review only"
                        }
                    ));
                    lines.push(format!("Summary: {}", metadata.description));
                    lines.push(format!("Impact: {}", metadata.impact));
                    lines.push(format!("Guidance: {}", metadata.recommendation));
                    if let Some(caution) = &metadata.caution {
                        lines.push(format!("Caution: {}", caution));
                    }
                } else {
                    lines.push(format!(
                        "Safety: {}",
                        category_safety_label(CategorySafetyLevel::HighCaution)
                    ));
                    lines.push("Summary: Profile metadata unavailable.".to_string());
                    lines.push("Recommendation: Inspect entries before cleaning.".to_string());
                }

                if !category.roots.is_empty() {
                    lines.push(format!("Roots scanned: {}", category.roots.len()));
                    for root in &category.roots {
                        lines.push(format!("Root: {}", shorten_path(root, 48)));
                    }
                }

                if let Some(note) = &category.note {
                    lines.push(format!("Scan status: {}", note));
                }

                lines
            })
            .unwrap_or_else(|| {
                vec![
                    "Source: Xcode".to_string(),
                    format!("Total size: {}", format_bytes(self.total_size_bytes())),
                    format!("Total files: {}", self.total_file_count()),
                    format!(
                        "Selected to clean: {}",
                        format_bytes(self.total_reclaimable_bytes())
                    ),
                    format!("Selected files: {}", self.total_reclaimable_file_count()),
                    "Safety: cleanup remains review-first and moves selected entries to Trash after confirmation."
                        .to_string(),
                    "Hint: Enter opens category.".to_string(),
                ]
            })
    }

    fn entry_detail_lines(&self, category: &CleanCategory) -> Vec<String> {
        let Some(entry) = self.selected_entry() else {
            return vec![
                format!("Category: {}", category.name),
                format!("Total size: {}", format_bytes(category.total_size_bytes)),
                format!("Total files: {}", category.total_file_count),
                "No entry selected.".to_string(),
            ];
        };

        let mut lines = vec![
            format!("Entry: {}", entry.name),
            format!("Category: {}", category.name),
            format!("Size: {}", format_bytes(entry.size_bytes)),
            format!("Files: {}", entry.file_count),
            format!("Age: {}", entry.age.age_label),
            format!("Last modified: {}", entry.age.last_modified_label),
            format!("Staleness: {}", entry.age.stale_bucket.label()),
            format!("Safety: {}", safety_label(entry.metadata.safety)),
        ];

        if let Some(rule) = &entry.metadata.matched_rule {
            lines.push(format!("Rule: {}", rule));
        }

        lines.push(format!("Artifact: {}", entry.metadata.description));

        if let Some(impact) = &entry.metadata.impact {
            lines.push(format!("Impact: {}", impact));
        }

        lines.push(format!("Recommendation: {}", entry.metadata.recommendation));
        lines.push(format!(
            "Selection: {}",
            if entry.keep {
                "keep"
            } else {
                "cleanup candidate"
            }
        ));
        lines
    }

    fn cleanup_result_detail_lines(&self, result: &CleanupExecutionResult) -> Vec<String> {
        let mut lines = vec![
            format!("Moved to Trash: {}", result.moved_count),
            format!("Dry-run eligible: {}", result.dry_run_eligible_count),
            format!("Skipped: {}", result.skipped_count),
            format!("Failed: {}", result.failed_count),
            format!("Reclaimed: {}", format_bytes(result.cleaned_size_bytes)),
            format!("Log: {}", shorten_path(&result.log_path, 42)),
        ];

        if let Some(stats) = &self.cleanup_stats {
            lines.push(String::new());
            lines.push(format!("Total cleanups: {}", stats.total_cleanups));
            lines.push(format!(
                "Total cleaned: {}",
                format_bytes(stats.total_bytes_cleaned)
            ));
            lines.push(format!("Entries cleaned: {}", stats.entries_cleaned));
        }

        lines
    }

    fn preview_policy_lines(&self) -> Vec<String> {
        let Some(scan) = &self.xcode_scan else {
            return Vec::new();
        };

        let mut review = Vec::new();
        let mut keep_by_default = Vec::new();

        for category in &scan.categories {
            if category.remove_count() == 0 {
                continue;
            }

            let Some(metadata) = &category.metadata else {
                continue;
            };

            match metadata.cleanup_kind {
                CleanupRecommendationKind::SafeCleanupCandidate => {}
                CleanupRecommendationKind::ReviewCarefully => review.push(category.name.clone()),
                CleanupRecommendationKind::KeepByDefault => {
                    keep_by_default.push(category.name.clone())
                }
            }
        }

        let mut lines = Vec::new();
        if review.is_empty() && keep_by_default.is_empty() {
            lines.push("Selected categories are in the safe cleanup candidate tier.".to_string());
            return lines;
        }

        if !review.is_empty() {
            lines.push(format!("Review carefully: {}", review.join(", ")));
        }

        if !keep_by_default.is_empty() {
            lines.push(format!("Keep by default: {}", keep_by_default.join(", ")));
        }

        let plan = self.preview_plan();
        if let Some(phrase) = &plan.high_caution_phrase {
            lines.push(format!("Typed confirmation required: {}", phrase));
        }

        lines
    }

    fn high_caution_detail_lines(&self) -> Vec<String> {
        let Some(state) = &self.high_caution_confirmation else {
            return vec!["No high-caution confirmation is required.".to_string()];
        };

        vec![
            "High-caution cleanup was selected.".to_string(),
            format!("Categories: {}", state.categories.join(", ")),
            "These categories are keep-by-default and need extra friction before Trash move."
                .to_string(),
            format!("Type exactly: {}", state.phrase),
            format!("Current input: {}", state.typed),
        ]
    }
}

fn is_known_category(category_id: CleanCategoryId) -> bool {
    matches!(
        category_id,
        CleanCategoryId::DerivedData
            | CleanCategoryId::Archives
            | CleanCategoryId::DeviceSupport
            | CleanCategoryId::SwiftUIPreviews
            | CleanCategoryId::Products
            | CleanCategoryId::DocumentationCache
            | CleanCategoryId::TestLogs
            | CleanCategoryId::ResultBundles
            | CleanCategoryId::TemporaryXcodeBuildFolders
    )
}

fn source_label(source_id: CleanSourceId) -> &'static str {
    match source_id {
        CleanSourceId::Xcode => "Xcode",
        CleanSourceId::CustomPlaceholder => "Custom source",
    }
}

fn shorten_path(path: &Path, max_width: usize) -> String {
    let mut value = path.display().to_string();
    if let Some(home) = env::var_os("HOME") {
        let home_path = PathBuf::from(home).display().to_string();
        if value.starts_with(&home_path) {
            value = format!("~{}", &value[home_path.len()..]);
        }
    }

    if value.len() <= max_width {
        return value;
    }

    let tail_len = max_width.saturating_sub(3);
    format!("...{}", &value[value.len().saturating_sub(tail_len)..])
}

fn divider(width: usize) -> String {
    "-".repeat(width)
}

fn truncate_text(value: &str, max_chars: usize) -> String {
    let char_count = value.chars().count();
    if char_count <= max_chars {
        return value.to_string();
    }

    if max_chars <= 1 {
        return "…".to_string();
    }

    let visible = max_chars.saturating_sub(1);
    let head: String = value.chars().take(visible).collect();
    format!("{head}…")
}

fn safety_label(safety: SafetyLevel) -> &'static str {
    safety.label()
}

fn category_safety_label(safety: CategorySafetyLevel) -> &'static str {
    safety.label()
}

fn category_safety_badge(safety: CategorySafetyLevel) -> &'static str {
    match safety {
        CategorySafetyLevel::HighConfidence => "High confidence",
        CategorySafetyLevel::MediumConfidence => "Medium confidence",
        CategorySafetyLevel::HighCaution => "High caution",
    }
}

fn cleanup_kind_label(kind: CleanupRecommendationKind) -> &'static str {
    kind.label()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::{
        profile::{CategorySafetyLevel, CleanupRecommendationKind, SafetyLevel},
        sources::{
            CleanCategory, CleanCategoryId, CleanCategoryMetadata, CleanEntry, CleanEntryMetadata,
            CleanSourceId, EntryAge, ScannedSource, StaleBucket,
        },
    };

    use super::{App, Screen};

    fn make_entry(name: &str, keep: bool) -> CleanEntry {
        CleanEntry {
            name: name.to_string(),
            path: PathBuf::from(format!("/tmp/{name}")),
            allowed_root: PathBuf::from("/tmp"),
            size_bytes: 10,
            file_count: 1,
            age: EntryAge {
                last_modified_unix_seconds: Some(1),
                last_modified_label: "1970-01-01".to_string(),
                age_seconds: Some(60 * 60),
                age_label: "Today".to_string(),
                stale_bucket: StaleBucket::Fresh,
            },
            keep,
            metadata: CleanEntryMetadata {
                matched_rule: None,
                description: "Generated artifact.".to_string(),
                safety: SafetyLevel::Recommended,
                recommendation: "Review.".to_string(),
                impact: None,
            },
        }
    }

    fn make_category(
        id: CleanCategoryId,
        name: &str,
        safety: CategorySafetyLevel,
        cleanup_kind: CleanupRecommendationKind,
        default_cleanup: bool,
        keep: bool,
    ) -> CleanCategory {
        CleanCategory {
            id,
            name: name.to_string(),
            stats_key: Some(format!("xcode.{name}")),
            path: PathBuf::from("/tmp"),
            roots: vec![PathBuf::from("/tmp")],
            exists: true,
            note: None,
            warnings: Vec::new(),
            entries: vec![make_entry(name, keep)],
            total_size_bytes: 10,
            total_file_count: 1,
            metadata: Some(CleanCategoryMetadata {
                description: "Category".to_string(),
                safety,
                default_cleanup,
                cleanup_kind,
                reversible: true,
                move_to_trash: true,
                caution: None,
                recommendation: "Review".to_string(),
                impact: "Impact".to_string(),
            }),
        }
    }

    fn app_with_scan(categories: Vec<CleanCategory>) -> App {
        let mut app = App::new();
        app.xcode_scan = Some(ScannedSource {
            source_id: CleanSourceId::Xcode,
            source_name: "Xcode",
            profile_key: "xcode".to_string(),
            root_hint: PathBuf::from("/tmp"),
            categories,
            warnings: Vec::new(),
        });
        app
    }

    #[test]
    fn archives_cleanup_requires_high_caution_confirmation() {
        let mut app = app_with_scan(vec![make_category(
            CleanCategoryId::Archives,
            "Archives",
            CategorySafetyLevel::HighCaution,
            CleanupRecommendationKind::KeepByDefault,
            false,
            false,
        )]);

        app.show_confirmation();

        assert_eq!(app.screen, Screen::HighCautionConfirmation);
        assert_eq!(
            app.high_caution_confirmation
                .as_ref()
                .map(|state| state.phrase.as_str()),
            Some("CLEAN ARCHIVES")
        );
    }

    #[test]
    fn device_support_cleanup_requires_high_caution_confirmation() {
        let mut app = app_with_scan(vec![make_category(
            CleanCategoryId::DeviceSupport,
            "Device Support",
            CategorySafetyLevel::HighCaution,
            CleanupRecommendationKind::KeepByDefault,
            false,
            false,
        )]);

        app.show_confirmation();

        assert_eq!(app.screen, Screen::HighCautionConfirmation);
        assert_eq!(
            app.high_caution_confirmation
                .as_ref()
                .map(|state| state.phrase.as_str()),
            Some("CLEAN DEVICE SUPPORT")
        );
    }

    #[test]
    fn high_confidence_cleanup_does_not_require_high_caution_confirmation() {
        let mut app = app_with_scan(vec![make_category(
            CleanCategoryId::DerivedData,
            "Derived Data",
            CategorySafetyLevel::HighConfidence,
            CleanupRecommendationKind::SafeCleanupCandidate,
            true,
            false,
        )]);

        app.show_confirmation();

        assert_eq!(app.screen, Screen::ConfirmCleanup);
        assert!(app.high_caution_confirmation.is_none());
    }

    #[test]
    fn typed_high_caution_phrase_advances_to_final_confirmation() {
        let mut app = app_with_scan(vec![make_category(
            CleanCategoryId::Archives,
            "Archives",
            CategorySafetyLevel::HighCaution,
            CleanupRecommendationKind::KeepByDefault,
            false,
            false,
        )]);
        app.show_confirmation();

        for ch in "CLEAN ARCHIVES".chars() {
            app.append_high_caution_confirmation_char(ch);
        }
        app.submit_high_caution_confirmation();

        assert_eq!(app.screen, Screen::ConfirmCleanup);
        assert!(app.warning.is_none());
    }

    #[test]
    fn incorrect_high_caution_phrase_keeps_user_on_typed_confirmation() {
        let mut app = app_with_scan(vec![make_category(
            CleanCategoryId::Archives,
            "Archives",
            CategorySafetyLevel::HighCaution,
            CleanupRecommendationKind::KeepByDefault,
            false,
            false,
        )]);
        app.show_confirmation();

        for ch in "WRONG".chars() {
            app.append_high_caution_confirmation_char(ch);
        }
        app.submit_high_caution_confirmation();

        assert_eq!(app.screen, Screen::HighCautionConfirmation);
        assert!(app.warning.is_some());
    }
}
