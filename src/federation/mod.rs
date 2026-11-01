pub mod activities;
pub mod outbound;
pub mod protocol;
pub mod traits;

use activitypub_federation::config::{FederationConfig, FederationConfigBuilderError};
use sea_orm::DatabaseConnection;

/// The crate's federation settings, keyed to Taverna's database.
pub type TavernaFederation = FederationConfig<DatabaseConnection>;

/// Builds the one [`FederationConfig`] the app uses.
///
/// `App::after_context` stores it in Loco's shared store, so the HTTP
/// middleware and the delivery worker share its HTTP client and settings.
pub async fn build_config(
    db: DatabaseConnection,
    domain: &str,
) -> Result<TavernaFederation, FederationConfigBuilderError> {
    FederationConfig::builder()
        .domain(domain)
        .app_data(db)
        .debug(cfg!(debug_assertions))
        .build()
        .await
}

/// Serves `object` as `application/activity+json` under Taverna's
/// `@context` (ActivityStreams plus the security vocabulary for keys).
pub fn ap_response<T: serde::Serialize>(object: T) -> axum::response::Response {
    use activitypub_federation::{axum::json::FederationJson, protocol::context::WithContext};
    use axum::response::IntoResponse;
    FederationJson(WithContext::new(
        object,
        serde_json::json!(protocol::AP_CONTEXT),
    ))
    .into_response()
}

/// Turns the crate's `anyhow` errors into a Loco 500 that keeps the cause.
pub fn internal(e: anyhow::Error) -> loco_rs::Error {
    loco_rs::Error::Any(e.into())
}
