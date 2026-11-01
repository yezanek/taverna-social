use activitypub_federation::config::Data;
use activitypub_federation::fetch::webfinger::{
    Webfinger, build_webfinger_response_with_type, extract_webfinger_name,
};
use axum::{extract::Query, http::header::CONTENT_TYPE, response::IntoResponse};
use loco_rs::prelude::*;
use sea_orm::DatabaseConnection;
use serde::Deserialize;
use tracing::instrument;
use url::Url;

use crate::models::_entities::communities;
use crate::models::users::UserModel;

const WEBFINGER_CONTENT_TYPE: &str = "application/jrd+json";

/// Query parameters for WebFinger requests.
#[derive(Debug, Deserialize)]
pub struct WebFingerQuery {
    pub resource: String,
}

/// WebFinger endpoint for discovering users and communities by acct: URI.
///
/// The crate checks that the `acct:` resource names this instance's domain;
/// a name may be a user (`Person`) or a community (`Group`).
#[instrument(skip(data))]
pub async fn webfinger(
    data: Data<DatabaseConnection>,
    Query(query): Query<WebFingerQuery>,
) -> Result<Response> {
    let name = extract_webfinger_name(&query.resource, &data)
        .map_err(|e| Error::BadRequest(format!("Invalid webfinger query: {e}")))?
        .to_lowercase();
    let db = data.app_data();

    let (ap_id, kind) = if let Some(user) = UserModel::find_by_username(db, &name).await? {
        (user.ap_id, "Person")
    } else if let Some(community) = communities::Entity::find()
        .filter(communities::Column::Slug.eq(&name))
        .one(db)
        .await?
    {
        (community.ap_id, "Group")
    } else {
        return Err(Error::NotFound);
    };

    let url: Url = ap_id.parse().map_err(Error::wrap)?;
    let response: Webfinger =
        build_webfinger_response_with_type(query.resource, vec![(url, Some(kind))]);
    Ok((
        [(CONTENT_TYPE, WEBFINGER_CONTENT_TYPE)],
        axum::Json(response),
    )
        .into_response())
}
