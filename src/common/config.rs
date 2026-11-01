use anyhow::{Result, anyhow};
use serde::{Deserialize, Deserializer};

/// Taverna's own settings, read from the `settings:` block of
/// `config/<env>.yaml`. Values that differ per deployment are pulled from the
/// environment there with `get_env`, so the YAML is the one place that lists
/// every setting and its default.
#[derive(Debug, Clone, Deserialize)]
pub struct TavernaConfig {
    pub base_url: String,
    pub domain: String,
    pub federation: FederationSettings,
    #[serde(default)]
    pub rate_limiting: RateLimitSettings,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FederationSettings {
    pub enabled: bool,
    /// A YAML list, or a comma-separated string (as `BLOCKED_DOMAINS` is).
    #[serde(default, deserialize_with = "list_or_comma_separated")]
    pub blocked_domains: Vec<String>,
    pub http_signature_expiry_seconds: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RateLimitSettings {
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Default for RateLimitSettings {
    fn default() -> Self {
        Self { enabled: true }
    }
}

fn default_true() -> bool {
    true
}

fn list_or_comma_separated<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum ListOrString {
        List(Vec<String>),
        String(String),
    }

    let items = match Option::<ListOrString>::deserialize(deserializer)? {
        None => vec![],
        Some(ListOrString::List(items)) => items,
        Some(ListOrString::String(s)) => s.split(',').map(str::to_string).collect(),
    };
    Ok(items
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

impl TavernaConfig {
    /// Deserializes the settings from Loco's `settings:` config block.
    pub fn from_config(config: &loco_rs::config::Config) -> Result<Self> {
        let settings = config
            .settings
            .clone()
            .ok_or_else(|| anyhow!("the `settings:` block is missing from the config file"))?;
        serde_json::from_value(settings).map_err(|e| anyhow!("invalid `settings:` block: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(blocked: serde_json::Value) -> Vec<String> {
        let config: TavernaConfig = serde_json::from_value(json!({
            "base_url": "http://localhost:3000",
            "domain": "localhost:3000",
            "federation": {
                "enabled": false,
                "blocked_domains": blocked,
                "http_signature_expiry_seconds": 30
            }
        }))
        .expect("test settings deserialize");
        assert!(config.rate_limiting.enabled);
        config.federation.blocked_domains
    }

    #[test]
    fn blocked_domains_accepts_list_and_comma_string() {
        assert_eq!(
            parse(json!(["a.example", " b.example "])),
            ["a.example", "b.example"]
        );
        assert_eq!(
            parse(json!("a.example, b.example,,")),
            ["a.example", "b.example"]
        );
        assert!(parse(json!("")).is_empty());
        assert!(parse(json!(null)).is_empty());
    }
}
