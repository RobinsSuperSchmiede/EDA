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
    /// Scopes of an agent session or paired device this policy was limited to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scoped: Option<Vec<Permission>>,
}

impl Policy {
    /// Policy for agents (tools, MCP).
    pub fn for_agents(cfg: &AgentsConfig, mode: Mode) -> Self {
        let granted = if cfg.enabled { cfg.permissions_for(mode) } else { BTreeSet::new() };
        Self { mode, granted, approval_required: cfg.approval_required(), scoped: None }
    }

    /// Policy for the application frontend (SDK token). Never needs approval:
    /// the frontend is the application itself, so scopes are simply granted or not.
    pub fn for_sdk(cfg: &AgentsConfig, mode: Mode) -> Self {
        Self { mode, granted: cfg.sdk_permissions(), approval_required: BTreeSet::new(), scoped: None }
    }

    /// Unrestricted policy (local CLI / administrator).
    pub fn admin(mode: Mode) -> Self {
        Self {
            mode,
            granted: Permission::ALL.into_iter().collect(),
            approval_required: BTreeSet::new(),
            scoped: None,
        }
    }

    /// The same policy limited to `scopes` (empty: unchanged). Agent sessions and paired devices
    /// get their own scopes, but never more than the app grants agents in this mode.
    pub fn restricted(&self, scopes: &[Permission]) -> Self {
        if scopes.is_empty() {
            return self.clone();
        }
        Self {
            mode: self.mode,
            granted: self.granted.iter().filter(|p| scopes.contains(p)).copied().collect(),
            approval_required: self.approval_required.clone(),
            scoped: Some(scopes.to_vec()),
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
        if let Some(scopes) = self.scoped.as_ref().filter(|s| !s.contains(&permission)) {
            return format!(
                "permission `{permission}` is not in the scopes of this agent session or device ({}).                  Create its token again with more scopes (`openshell agents token <name> --scopes …`).",
                scopes.iter().map(|p| p.as_str()).collect::<Vec<_>>().join(", ")
            );
        }
        format!(
            "permission `{permission}` is not granted to agents in {} mode (granted: {}). \
             Adjust `agents.permissions` in openshell.json to allow it.",
            self.mode,
            if self.granted.is_empty() {
                "none".to_string()
            } else {
                self.granted.iter().map(|p| p.as_str()).collect::<Vec<_>>().join(", ")
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
        assert!(!AgentsConfig::default().enabled, "agents are opt-in");
        assert!(Policy::for_agents(&AgentsConfig::default(), Mode::Development).granted.is_empty());
        let cfg = AgentsConfig { enabled: true, ..Default::default() };
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
            r#"{"app":{"name":"x"},"agents":{"enabled":true,"permissions":["read","code","system"],"approval_required":["code"]}}"#,
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
    fn restricted_policies_never_grow() {
        let cfg = AgentsConfig { enabled: true, ..Default::default() };
        let prod = Policy::for_agents(&cfg, Mode::Production);
        let r = prod.restricted(&[Permission::Read, Permission::Code]);
        assert_eq!(r.check(Permission::Read), Decision::Allow);
        assert_eq!(r.check(Permission::Code), Decision::Deny, "not granted in production");
        assert_eq!(r.check(Permission::Actions), Decision::Deny, "not in the session's scopes");
        assert_eq!(prod.restricted(&[]).granted, prod.granted);
        assert!(r.denial_message(Permission::Actions).contains("scopes of this agent session"));
        assert!(r.denial_message(Permission::Code).contains("production mode"));
    }

    #[test]
    fn disabled_agents_get_nothing() {
        let cfg = AgentsConfig { enabled: false, ..Default::default() };
        assert_eq!(Policy::for_agents(&cfg, Mode::Development).check(Permission::Read), Decision::Deny);
    }
}
