use activitypub_federation::config::Data;
use activitypub_federation::fetch::webfinger::{
    build_webfinger_response_with_type, extract_webfinger_name,
};
use axum::{extract::Query, http::header::CONTENT_TYPE, response::IntoResponse};
use loco_rs::prelude::*;
use sea_orm::DatabaseConnection;
use serde::Deserialize;
use tracing::instrument;
use url::Url;

use crate::models::_entities::{communities, users};

const WEBFINGER_CONTENT_TYPE: &str = "application/jrd+json";

#[derive(Debug, Deserialize)]
pub struct WebFingerQuery {
    pub resource: String,
}

#[instrument(skip(ctx, data))]
pub async fn webfinger(
    State(ctx): State<AppContext>,
    data: Data<DatabaseConnection>,
    Query(query): Query<WebFingerQuery>,
) -> Result<Response> {
    tracing::warn!(target: "webfinger", "extract_webfinger_name called with: resource={}, domain={:?}", query.resource, data.domain());
    let username_or_slug = extract_webfinger_name(&query.resource, &data)
        .map_err(|e| Error::BadRequest(format!("Invalid webfinger query: {}", e)))?;
    tracing::warn!(target: "webfinger", "extract_webfinger_name succeeded: {}", username_or_slug);

    if let Ok(Some(user)) = users::Entity::find()
        .filter(users::Column::Username.eq(username_or_slug.to_lowercase()))
        .one(&ctx.db)
        .await
    {
        let url: Url = user.ap_id.parse().map_err(|_| Error::InternalServerError)?;
        let wf =
            build_webfinger_response_with_type(query.resource.clone(), vec![(url, Some("Person"))]);
        let json = serde_json::to_string(&wf).map_err(|_| Error::InternalServerError)?;
        return Ok(([(CONTENT_TYPE, WEBFINGER_CONTENT_TYPE)], json).into_response());
    }

    if let Ok(Some(community)) = communities::Entity::find()
        .filter(communities::Column::Slug.eq(username_or_slug.to_lowercase()))
        .one(&ctx.db)
        .await
    {
        let url: Url = community
            .ap_id
            .parse()
            .map_err(|_| Error::InternalServerError)?;
        let wf =
            build_webfinger_response_with_type(query.resource.clone(), vec![(url, Some("Group"))]);
        let json = serde_json::to_string(&wf).map_err(|_| Error::InternalServerError)?;
        return Ok(([(CONTENT_TYPE, WEBFINGER_CONTENT_TYPE)], json).into_response());
    }

    Err(Error::NotFound)
}
