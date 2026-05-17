use activitypub_federation::axum::inbox::{ActivityData, receive_activity};
use activitypub_federation::config::Data;
use loco_rs::prelude::*;
use sea_orm::DatabaseConnection;
use tracing::instrument;

use crate::federation::protocol::TavernaActivity;
use crate::federation::traits::{DbCommunity, DbUser};

#[instrument(skip(_ctx, activity_data, data))]
pub async fn user_inbox(
    State(_ctx): State<AppContext>,
    data: Data<DatabaseConnection>,
    activity_data: ActivityData,
) -> Result<Response> {
    receive_activity::<TavernaActivity, DbUser, DatabaseConnection>(activity_data, &data)
        .await
        .map_err(|e| {
            tracing::warn!("Failed to receive activity: {:#}", e);
            Error::BadRequest("Invalid activity".to_string())
        })?;
    Ok(axum::Json(serde_json::json!({"status": "ok"})).into_response())
}

#[instrument(skip(_ctx, activity_data, data))]
pub async fn community_inbox(
    State(_ctx): State<AppContext>,
    data: Data<DatabaseConnection>,
    activity_data: ActivityData,
) -> Result<Response> {
    receive_activity::<TavernaActivity, DbCommunity, DatabaseConnection>(activity_data, &data)
        .await
        .map_err(|e| {
            tracing::warn!("Failed to receive activity: {:#}", e);
            Error::BadRequest("Invalid activity".to_string())
        })?;
    Ok(axum::Json(serde_json::json!({"status": "ok"})).into_response())
}
