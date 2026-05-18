use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct TavernaConfig {
    pub base_url: String,
    pub domain: String,
    pub federation: FederationSettings,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FederationSettings {
    pub enabled: bool,
    pub blocked_domains: Vec<String>,
    pub http_signature_expiry_seconds: u32,
}

impl TavernaConfig {
    pub fn from_env() -> Result<Self, String> {
        let base_url =
            std::env::var("BASE_URL").unwrap_or_else(|_| "http://localhost:3000".to_string());
        let domain = std::env::var("TAVERNA_DOMAIN")
            .or_else(|_| std::env::var("DOMAIN"))
            .unwrap_or_else(|_| "localhost:3000".to_string());
        let federation_enabled = match std::env::var("FEDERATION_ENABLED") {
            Ok(v) => match v.to_lowercase().as_str() {
                "true" => true,
                "false" => false,
                other => {
                    return Err(format!(
                        "Invalid FEDERATION_ENABLED value '{}': expected 'true' or 'false'",
                        other
                    ));
                }
            },
            Err(_) => false,
        };

        let blocked_domains: Vec<String> = match std::env::var("BLOCKED_DOMAINS") {
            Ok(val) => val
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            Err(_) => vec![],
        };

        let http_signature_expiry_seconds = match std::env::var("HTTP_SIGNATURE_EXPIRY_SECONDS") {
            Ok(val) => val
                .parse::<u32>()
                .map_err(|e| format!("Invalid HTTP_SIGNATURE_EXPIRY_SECONDS '{}': {}", val, e))?,
            Err(_) => 30,
        };

        Ok(Self {
            base_url,
            domain,
            federation: FederationSettings {
                enabled: federation_enabled,
                blocked_domains,
                http_signature_expiry_seconds,
            },
        })
    }
}
