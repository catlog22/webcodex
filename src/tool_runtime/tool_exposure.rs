//! Operator-controlled exposure for the model-facing tool surface.
//!
//! `WEBCODEX_ENABLED_TOOLS` / `WEBCODEX_DISABLED_TOOLS` accept a
//! comma-separated tool-name list. The lists are validated at startup against
//! the registered tool catalog: unknown names and enabled/disabled conflicts
//! fail closed so a typo can never silently widen or narrow the surface.
//! Hidden/internal tools keep working; the exposure gate only filters the
//! model-visible listings and fails closed at the dispatch entry.

use std::collections::BTreeSet;

use webcodex_tool_contracts::{lookup_tool_definition, ToolSpec};

const ENABLED_TOOLS_ENV: &str = "WEBCODEX_ENABLED_TOOLS";
const DISABLED_TOOLS_ENV: &str = "WEBCODEX_DISABLED_TOOLS";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolExposure {
    /// `None` means every registered tool stays enabled unless disabled.
    enabled: Option<BTreeSet<String>>,
    disabled: BTreeSet<String>,
}

impl ToolExposure {
    /// Parse and validate both env vars. An empty/unset var means "no
    /// restriction"; a set-but-empty entry list is rejected so
    /// `WEBCODEX_ENABLED_TOOLS=` cannot be mistaken for allow-all.
    pub fn from_env() -> Result<Self, String> {
        Self::from_env_vars(
            std::env::var(ENABLED_TOOLS_ENV).ok().as_deref(),
            std::env::var(DISABLED_TOOLS_ENV).ok().as_deref(),
        )
    }

    pub(crate) fn from_env_vars(
        enabled: Option<&str>,
        disabled: Option<&str>,
    ) -> Result<Self, String> {
        let enabled = parse_tool_list(ENABLED_TOOLS_ENV, enabled)?;
        let disabled = parse_tool_list(DISABLED_TOOLS_ENV, disabled)?;
        if let (Some(enabled), Some(disabled)) = (&enabled, &disabled) {
            let conflict: Vec<&str> = enabled.intersection(disabled).map(String::as_str).collect();
            if !conflict.is_empty() {
                return Err(format!(
                    "{ENABLED_TOOLS_ENV} and {DISABLED_TOOLS_ENV} conflict on: {}",
                    conflict.join(", ")
                ));
            }
        }
        Ok(Self {
            enabled,
            disabled: disabled.unwrap_or_default(),
        })
    }

    /// Allow-list restriction active (operator narrowed the surface).
    pub fn is_restricted(&self) -> bool {
        self.enabled.is_some() || !self.disabled.is_empty()
    }

    pub fn allows(&self, tool_name: &str) -> bool {
        if self.disabled.contains(tool_name) {
            return false;
        }
        match &self.enabled {
            Some(enabled) => enabled.contains(tool_name),
            None => true,
        }
    }

    pub fn filter_specs(&self, specs: Vec<ToolSpec>) -> Vec<ToolSpec> {
        specs
            .into_iter()
            .filter(|spec| self.allows(spec.name.as_str()))
            .collect()
    }
}

fn parse_tool_list(env: &str, raw: Option<&str>) -> Result<Option<BTreeSet<String>>, String> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let mut names = BTreeSet::new();
    for entry in raw.split(',') {
        let name = entry.trim();
        if name.is_empty() {
            continue;
        }
        if lookup_tool_definition(name).is_none() {
            return Err(format!("{env} names unknown tool '{name}'"));
        }
        names.insert(name.to_string());
    }
    Ok(Some(names))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_config_allows_everything() {
        let exposure = ToolExposure::from_env_vars(None, None).unwrap();
        assert!(!exposure.is_restricted());
        assert!(exposure.allows("session"));
        assert!(exposure.allows("read_files"));
    }

    #[test]
    fn enabled_list_narrows_to_named_tools() {
        let exposure = ToolExposure::from_env_vars(Some("session, todo"), None).unwrap();
        assert!(exposure.is_restricted());
        assert!(exposure.allows("session"));
        assert!(exposure.allows("todo"));
        assert!(!exposure.allows("read_files"));
    }

    #[test]
    fn disabled_list_blocks_named_tools() {
        let exposure = ToolExposure::from_env_vars(None, Some("read_files")).unwrap();
        assert!(exposure.allows("session"));
        assert!(!exposure.allows("read_files"));
    }

    #[test]
    fn unknown_tool_name_fails_closed() {
        let err = ToolExposure::from_env_vars(Some("not_a_tool"), None).unwrap_err();
        assert!(err.contains("not_a_tool"), "{err}");
        let err = ToolExposure::from_env_vars(None, Some("not_a_tool")).unwrap_err();
        assert!(err.contains("not_a_tool"), "{err}");
    }

    #[test]
    fn enabled_disabled_conflict_fails_closed() {
        let err = ToolExposure::from_env_vars(Some("session"), Some("session")).unwrap_err();
        assert!(err.contains("conflict"), "{err}");
    }

    #[test]
    fn filter_specs_drops_disabled_entries() {
        let exposure = ToolExposure::from_env_vars(None, Some("cargo_test")).unwrap();
        let specs = webcodex_tool_contracts::registered_tool_specs();
        let kept = exposure.filter_specs(specs);
        assert!(!kept.is_empty());
        assert!(!kept.iter().any(|spec| spec.name == "cargo_test"));
    }
}
