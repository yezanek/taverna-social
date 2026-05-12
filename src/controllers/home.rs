use axum::Extension;
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use loco_rs::prelude::*;
use serde_json::json;
use tracing::instrument;

use crate::initializers::auth_middleware::OptionalAuthUser;
use crate::initializers::theme_middleware::CurrentTheme;
use crate::models::posts::PostModel;
use crate::views::helpers;

const PER_PAGE: u64 = 20;

#[instrument(skip(v, ctx, theme, current_user, jar))]
pub async fn index(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    OptionalAuthUser(current_user): OptionalAuthUser,
    jar: CookieJar,
    Extension(theme): Extension<CurrentTheme>,
) -> Result<Response> {
    let (posts, _num_pages) = PostModel::list_all(&ctx.db, 0, PER_PAGE)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "failed to list posts");
            Error::InternalServerError
        })?;

    let csrf_token = uuid::Uuid::new_v4().to_string();
    let csrf_cookie = Cookie::build(("csrf_token", csrf_token.clone()))
        .http_only(true)
        .same_site(SameSite::Strict)
        .path("/")
        .build();
    let response = helpers::render_with_theme(
        &v,
        "home/index.html",
        json!({
            "current_user": current_user,
            "posts": posts,
            "csrf_token": csrf_token,
        }),
        &theme,
    )?;
    Ok((jar.add(csrf_cookie), response).into_response())
}
