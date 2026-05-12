use axum::Extension;
use loco_rs::prelude::*;
use serde_json::json;
use tracing::instrument;

use crate::initializers::auth_middleware::OptionalAuthUser;
use crate::initializers::theme_middleware::CurrentTheme;
use crate::models::posts::PostModel;
use crate::views::helpers;

const PER_PAGE: u64 = 20;

#[instrument(skip(v, ctx, theme))]
pub async fn index(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    OptionalAuthUser(current_user): OptionalAuthUser,
    Extension(theme): Extension<CurrentTheme>,
) -> Result<Response> {
    let (posts, _num_pages) = PostModel::list_all(&ctx.db, 0, PER_PAGE)
        .await
        .map_err(|_| Error::InternalServerError)?;

    helpers::render_with_theme(
        &v,
        "home/index.html",
        json!({
            "current_user": current_user,
            "posts": posts,
        }),
        &theme,
    )
}
