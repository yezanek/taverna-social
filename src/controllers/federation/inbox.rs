use activitypub_federation::axum::inbox::{ActivityData, receive_activity};
use activitypub_federation::config::Data;
use loco_rs::prelude::*;
use sea_orm::DatabaseConnection;
use tracing::instrument;

use crate::federation::protocol::TavernaActivity;
use crate::federation::traits::{DbCommunity, DbUser};

fn map_inbox_error(e: anyhow::Error) -> Error {
    let msg = format!("{:#}", e);
    let lower = msg.to_lowercase();
    if lower.contains("missing field")
        || lower.contains("invalid type")
        || lower.contains("unknown field")
        || lower.contains("trailing characters")
        || lower.contains("invalid value")
        || lower.contains("domains do not match")
    {
        tracing::warn!("Bad request activity: {}", msg);
        Error::BadRequest("Invalid activity".to_string())
    } else {
        tracing::error!("Internal error processing activity: {}", msg);
        Error::InternalServerError
    }
}

/// Handles incoming ActivityPub activities addressed to a user.
#[instrument(skip(_ctx, activity_data, data))]
pub async fn user_inbox(
    State(_ctx): State<AppContext>,
    data: Data<DatabaseConnection>,
    activity_data: ActivityData,
) -> Result<Response> {
    receive_activity::<TavernaActivity, DbUser, DatabaseConnection>(activity_data, &data)
        .await
        .map_err(map_inbox_error)?;
    Ok(axum::Json(serde_json::json!({"status": "ok"})).into_response())
}

/// Handles incoming ActivityPub activities addressed to a community.
#[instrument(skip(_ctx, activity_data, data))]
pub async fn community_inbox(
    State(_ctx): State<AppContext>,
    data: Data<DatabaseConnection>,
    activity_data: ActivityData,
) -> Result<Response> {
    receive_activity::<TavernaActivity, DbCommunity, DatabaseConnection>(activity_data, &data)
        .await
        .map_err(map_inbox_error)?;
    Ok(axum::Json(serde_json::json!({"status": "ok"})).into_response())
}
