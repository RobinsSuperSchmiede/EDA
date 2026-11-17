//! Permission policy for agents and the frontend SDK.

use openshell_config::{AgentsConfig, Mode, Permission};
use serde::Serialize;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Allow,
    /// Allowed after explicit human approval.
    NeedsApproval,
    Deny,
}

/// Which scopes a caller has and which of them need approval.
#[derive(Debug, Clone, Serialize)]
pub struct Policy {
    pub mode: Mode,
    pub granted: BTreeSet<Permission>,
    pub approval_required: BTreeSet<Permission>,
}

impl Policy {
    /// Policy for agents (tools, MCP).
    pub fn for_agents(cfg: &AgentsConfig, mode: Mode) -> Self {
        let granted = if cfg.enabled {
            cfg.permissions_for(mode)
        } else {
            BTreeSet::new()
        };
        Self {
            mode,
            granted,
            approval_required: cfg.approval_required(),
        }
    }

    /// Policy for the application frontend (SDK token). Never needs approval:
    /// the frontend is the application itself, so scopes are simply granted or not.
    pub fn for_sdk(cfg: &AgentsConfig, mode: Mode) -> Self {
        Self {
            mode,
            granted: cfg.sdk_permissions(),
            approval_required: BTreeSet::new(),
        }
    }

    /// Unrestricted policy (local CLI / administrator).
    pub fn admin(mode: Mode) -> Self {
        Self {
            mode,
            granted: Permission::ALL.into_iter().collect(),
            approval_required: BTreeSet::new(),
        }
    }

    pub fn check(&self, permission: Permission) -> Decision {
        if !self.granted.contains(&permission) {
            Decision::Deny
        } else if self.approval_required.contains(&permission) {
            Decision::NeedsApproval
        } else {
            Decision::Allow
        }
    }

    pub fn denial_message(&self, permission: Permission) -> String {
        format!(
            "permission `{permission}` is not granted to agents in {} mode (granted: {}). \
             Adjust `agents.permissions` in openshell.json to allow it.",
            self.mode,
            if self.granted.is_empty() {
                "none".to_string()
            } else {
                self.granted
                    .iter()
                    .map(|p| p.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openshell_config::{parse_json, Mode};

    #[test]
    fn defaults_per_mode() {
        let cfg = AgentsConfig::default();
        let dev = Policy::for_agents(&cfg, Mode::Development);
        assert_eq!(dev.check(Permission::Code), Decision::Allow);
        assert_eq!(dev.check(Permission::System), Decision::Deny);
        let prod = Policy::for_agents(&cfg, Mode::Production);
        assert_eq!(prod.check(Permission::Read), Decision::Allow);
        assert_eq!(prod.check(Permission::Services), Decision::Deny);
        assert_eq!(prod.check(Permission::Code), Decision::Deny);
    }

    #[test]
    fn approval_required() {
        let cfg = parse_json(
            r#"{"app":{"name":"x"},"agents":{"permissions":["read","code","system"],"approval_required":["code"]}}"#,
        )
        .unwrap()
        .agents;
        let p = Policy::for_agents(&cfg, Mode::Production);
        assert_eq!(p.check(Permission::Code), Decision::NeedsApproval);
        assert_eq!(p.check(Permission::System), Decision::Allow);
        assert_eq!(p.check(Permission::Ui), Decision::Deny);
        assert!(p.denial_message(Permission::Ui).contains("read, code, system"));
    }

    #[test]
    fn disabled_agents_get_nothing() {
        let mut cfg = AgentsConfig::default();
        cfg.enabled = false;
        assert_eq!(
            Policy::for_agents(&cfg, Mode::Development).check(Permission::Read),
            Decision::Deny
        );
    }
}
