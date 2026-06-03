use serde::Deserialize;

// Bundled profiles are data-only. They describe artifacts and safety levels,
// while cleanup execution stays in the core flow.
const XCODE_PROFILE_TOML: &str = include_str!("profiles/xcode.toml");

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct CleanerProfile {
    pub id: String,
    pub name: String,
    pub description: String,
    pub platform: String,
    #[serde(default)]
    pub categories: Vec<ProfileCategory>,
    #[serde(default)]
    pub rules: Vec<ProfileRule>,
}

impl CleanerProfile {
    // Parsing errors are surfaced as readable messages so the scan layer can
    // fall back safely and still leave a useful warning for debugging.
    pub fn from_toml_str(contents: &str) -> Result<Self, ProfileLoadError> {
        toml::from_str(contents).map_err(|error| ProfileLoadError {
            message: format!("Failed to parse cleanup profile TOML: {error}"),
        })
    }

    pub fn bundled_xcode() -> Result<Self, ProfileLoadError> {
        Self::from_toml_str(XCODE_PROFILE_TOML)
    }

    // Rules are descriptive only. They annotate scan results and do not
    // override confirmation, allowed roots, or cleanup execution.
    pub fn match_rule(&self, artifact_name: &str) -> Option<&ProfileRule> {
        self.rules.iter().find(|rule| rule.matches(artifact_name))
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct ProfileCategory {
    pub id: String,
    pub name: String,
    pub path: String,
    pub description: String,
    pub safety: SafetyLevel,
    pub recommendation: String,
    pub impact: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct ProfileRule {
    #[serde(rename = "match")]
    pub pattern: String,
    #[serde(rename = "type")]
    pub match_type: RuleMatchType,
    pub safety: SafetyLevel,
    pub description: String,
    pub recommendation: String,
    pub impact: Option<String>,
}

impl ProfileRule {
    pub fn matches(&self, artifact_name: &str) -> bool {
        self.match_type.matches(&self.pattern, artifact_name)
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SafetyLevel {
    // Recommended: stale transient artifacts that are usually safe to remove.
    Recommended,
    // Rebuildable: caches or outputs that tools can regenerate.
    Rebuildable,
    // Caution: artifacts that may still be useful and should be reviewed.
    Caution,
    // Protected: artifacts users should generally avoid cleaning.
    Protected,
    // Unknown: unmatched or unclassified artifacts that need inspection.
    #[default]
    Unknown,
}

impl SafetyLevel {
    pub fn label(self) -> &'static str {
        match self {
            SafetyLevel::Recommended => "Recommended",
            SafetyLevel::Rebuildable => "Rebuildable",
            SafetyLevel::Caution => "Caution",
            SafetyLevel::Protected => "Protected",
            SafetyLevel::Unknown => "Unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum RuleMatchType {
    Exact,
    Glob,
}

impl RuleMatchType {
    pub fn matches(self, pattern: &str, artifact_name: &str) -> bool {
        match self {
            RuleMatchType::Exact => artifact_name == pattern,
            // Keep glob support intentionally small: only `*` wildcards are
            // supported so bundled rules stay easy to reason about.
            RuleMatchType::Glob => simple_glob_match(pattern, artifact_name),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileLoadError {
    message: String,
}

impl ProfileLoadError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ProfileLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ProfileLoadError {}

fn simple_glob_match(pattern: &str, artifact_name: &str) -> bool {
    if !pattern.contains('*') {
        return artifact_name == pattern;
    }

    let parts = pattern
        .split('*')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.is_empty() {
        return true;
    }

    let mut search_start = 0usize;

    for (index, part) in parts.iter().enumerate() {
        if index == 0 && !pattern.starts_with('*') {
            let Some(remaining) = artifact_name.get(search_start..) else {
                return false;
            };
            if !remaining.starts_with(part) {
                return false;
            }
            search_start += part.len();
            continue;
        }

        if index == parts.len() - 1 && !pattern.ends_with('*') {
            let Some(remaining) = artifact_name.get(search_start..) else {
                return false;
            };
            let Some(position) = remaining.rfind(part) else {
                return false;
            };
            let absolute_position = search_start + position;
            return absolute_position + part.len() == artifact_name.len();
        }

        let Some(remaining) = artifact_name.get(search_start..) else {
            return false;
        };
        let Some(position) = remaining.find(part) else {
            return false;
        };
        search_start += position + part.len();
    }

    true
}

#[cfg(test)]
mod tests {
    use super::{CleanerProfile, RuleMatchType, SafetyLevel};

    #[test]
    fn bundled_xcode_profile_loads_successfully() {
        let profile = CleanerProfile::bundled_xcode().expect("xcode profile should load");

        assert_eq!(profile.id, "xcode");
        assert_eq!(profile.name, "Xcode");
        assert_eq!(profile.platform, "macos");
        assert_eq!(profile.categories.len(), 4);
        assert!(profile.rules.len() >= 5);
    }

    #[test]
    fn parses_safety_levels_from_toml() {
        let profile = CleanerProfile::from_toml_str(
            r#"
id = "test"
name = "Test"
description = "Test profile"
platform = "macos"

[[categories]]
id = "sample"
name = "Sample"
path = "~/Library/Sample"
description = "Sample category"
safety = "protected"
recommendation = "Leave alone"
impact = "Loss is difficult to recover."
"#,
        )
        .expect("profile should parse");

        assert_eq!(profile.categories[0].safety, SafetyLevel::Protected);
    }

    #[test]
    fn exact_rule_matching_is_supported() {
        let profile = CleanerProfile::bundled_xcode().expect("xcode profile should load");
        let rule = profile
            .match_rule("ModuleCache.noindex")
            .expect("exact rule should match");

        assert_eq!(rule.match_type, RuleMatchType::Exact);
        assert_eq!(rule.safety, SafetyLevel::Rebuildable);
    }

    #[test]
    fn glob_rule_matching_is_supported() {
        let profile = CleanerProfile::bundled_xcode().expect("xcode profile should load");
        let rule = profile
            .match_rule("Unsaved_Xcode_Document_12345")
            .expect("glob rule should match");

        assert_eq!(rule.match_type, RuleMatchType::Glob);
    }

    #[test]
    fn unmatched_artifacts_fall_back_to_unknown_safely() {
        let profile = CleanerProfile::bundled_xcode().expect("xcode profile should load");

        assert!(
            profile
                .match_rule("DefinitelyNotAKnownXcodeArtifact")
                .is_none()
        );
    }
}
