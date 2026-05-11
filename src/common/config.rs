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
    pub fn from_env() -> Self {
        let base_url =
            std::env::var("BASE_URL").unwrap_or_else(|_| "http://localhost:3000".to_string());
        let domain = std::env::var("DOMAIN").unwrap_or_else(|_| "localhost:3000".to_string());
        let federation_enabled = std::env::var("FEDERATION_ENABLED")
            .map(|v| v == "true")
            .unwrap_or(false);

        Self {
            base_url,
            domain,
            federation: FederationSettings {
                enabled: federation_enabled,
                blocked_domains: vec![],
                http_signature_expiry_seconds: 30,
            },
        }
    }
}
