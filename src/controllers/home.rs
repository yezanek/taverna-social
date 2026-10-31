use axum::Extension;
use axum::extract::Query;
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use loco_rs::prelude::*;
use serde_json::json;
use tracing::instrument;

use crate::initializers::auth_middleware::OptionalAuthUser;
use crate::initializers::theme_middleware::CurrentTheme;
use crate::models::feed::{self, CursorQuery};
use crate::views::helpers;

const PER_PAGE: u64 = 20;

/// Renders the front page: local and remote posts, keyset-paginated.
#[instrument(skip(v, ctx, theme, current_user, jar))]
pub async fn index(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    OptionalAuthUser(current_user): OptionalAuthUser,
    jar: CookieJar,
    Extension(theme): Extension<CurrentTheme>,
    Query(page): Query<CursorQuery>,
) -> Result<Response> {
    let posts = feed::home_feed(&ctx.db, page.cursor().as_ref(), PER_PAGE)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "failed to list posts");
            Error::InternalServerError
        })?;

    let csrf_token = uuid::Uuid::new_v4().to_string();
    let csrf_cookie = Cookie::build(("csrf_token", csrf_token.clone()))
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(cfg!(not(debug_assertions)))
        .path("/")
        .build();
    let response = helpers::render_with_theme(
        &v,
        "home/index.html",
        json!({
            "current_user": current_user,
            "posts": posts.items,
            "next_cursor": posts.next_cursor,
            "csrf_token": csrf_token,
        }),
        &theme,
    )?;
    Ok((jar.add(csrf_cookie), response).into_response())
}
