//! AI provider discovery.
//!
//! OpenShell does not run or prescribe a model. It only makes the providers an
//! application is configured for — and local servers it can detect — visible
//! to agents and to the application (via the SDK).

use openshell_config::{ProviderConfig, ProviderKind};
use serde::Serialize;
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Clone, Serialize)]
pub struct ProviderInfo {
    pub name: String,
    pub kind: ProviderKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Reachable (local providers) or API key present (remote providers).
    pub available: bool,
    pub models: Vec<String>,
    /// `config` or `detected`
    pub source: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

const OLLAMA_URL: &str = "http://127.0.0.1:11434";
const LMSTUDIO_URL: &str = "http://127.0.0.1:1234";

async fn fetch_json(client: &reqwest::Client, url: &str) -> Option<Value> {
    let r = client.get(url).timeout(Duration::from_millis(800)).send().await.ok()?;
    if !r.status().is_success() {
        return None;
    }
    r.json().await.ok()
}

/// Query a provider endpoint for its models. Returns `None` if unreachable.
async fn probe(client: &reqwest::Client, kind: ProviderKind, base: &str) -> Option<Vec<String>> {
    let base = base.trim_end_matches('/');
    match kind {
        ProviderKind::Ollama => {
            let v = fetch_json(client, &format!("{base}/api/tags")).await?;
            Some(
                v["models"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|m| m["name"].as_str().map(str::to_string)).collect())
                    .unwrap_or_default(),
            )
        }
        ProviderKind::LmStudio | ProviderKind::OpenaiCompatible => {
            let url =
                if base.ends_with("/v1") { format!("{base}/models") } else { format!("{base}/v1/models") };
            let v = fetch_json(client, &url).await?;
            Some(
                v["data"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|m| m["id"].as_str().map(str::to_string)).collect())
                    .unwrap_or_default(),
            )
        }
        _ => None,
    }
}

/// Configured providers plus (optionally) detected local providers.
pub async fn discover(configured: &[ProviderConfig], detect_local: bool) -> Vec<ProviderInfo> {
    let client = reqwest::Client::builder().no_proxy().build().unwrap_or_default();
    let mut out = Vec::new();
    for p in configured {
        let (available, models, note) = match p.kind {
            ProviderKind::Ollama | ProviderKind::LmStudio | ProviderKind::OpenaiCompatible => {
                let base = p.url.clone().unwrap_or_else(|| {
                    if p.kind == ProviderKind::Ollama {
                        OLLAMA_URL.into()
                    } else {
                        LMSTUDIO_URL.into()
                    }
                });
                match probe(&client, p.kind, &base).await {
                    Some(models) => (true, if p.models.is_empty() { models } else { p.models.clone() }, None),
                    None => (false, p.models.clone(), Some(format!("not reachable at {base}"))),
                }
            }
            _ => {
                let has_key = p
                    .api_key_env
                    .as_ref()
                    .map(|k| std::env::var(k).map(|v| !v.is_empty()).unwrap_or(false))
                    .unwrap_or(true);
                let note = (!has_key).then(|| {
                    format!("environment variable {} is not set", p.api_key_env.clone().unwrap_or_default())
                });
                (has_key, p.models.clone(), note)
            }
        };
        out.push(ProviderInfo {
            name: p.name.clone(),
            kind: p.kind,
            url: p.url.clone(),
            available,
            models,
            source: "config",
            api_key_env: p.api_key_env.clone(),
            note,
        });
    }
    if detect_local {
        for (name, kind, url) in [
            ("ollama", ProviderKind::Ollama, OLLAMA_URL),
            ("lm-studio", ProviderKind::LmStudio, LMSTUDIO_URL),
        ] {
            if out.iter().any(|p| p.kind == kind) {
                continue;
            }
            if let Some(models) = probe(&client, kind, url).await {
                out.push(ProviderInfo {
                    name: name.into(),
                    kind,
                    url: Some(url.into()),
                    available: true,
                    models,
                    source: "detected",
                    api_key_env: None,
                    note: None,
                });
            }
        }
    }
    out
}
