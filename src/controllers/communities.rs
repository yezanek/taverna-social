use axum::extract::Path;
use axum::{Extension, Form};
use loco_rs::prelude::*;
use serde::Deserialize;
use serde_json::json;
use tracing::instrument;

use crate::common::config::TavernaConfig;
use crate::common::markdown::render_markdown;
use crate::initializers::auth_middleware::{AuthUser, OptionalAuthUser};
use crate::initializers::theme_middleware::CurrentTheme;
use crate::models::communities::CommunityModel;
use crate::models::posts::PostModel;
use crate::views::helpers;

const PER_PAGE: u64 = 20;

#[derive(Debug, Deserialize)]
pub struct CreatePostForm {
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CommunitySettingsForm {
    pub name: Option<String>,
    pub description: Option<String>,
    pub is_nsfw: Option<String>,
    pub posting_restricted: Option<String>,
}

#[instrument(skip(v, ctx, theme))]
pub async fn list(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    OptionalAuthUser(current_user): OptionalAuthUser,
    Extension(theme): Extension<CurrentTheme>,
) -> Result<Response> {
    let page = 0;
    let (communities, _num_pages) = CommunityModel::list_all(&ctx.db, page, PER_PAGE)
        .await
        .map_err(|_| Error::InternalServerError)?;

    helpers::render_with_theme(
        &v,
        "communities/list.html",
        json!({
            "current_user": current_user,
            "communities": communities,
        }),
        &theme,
    )
}

#[instrument(skip(v, ctx, theme))]
pub async fn feed(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    OptionalAuthUser(current_user): OptionalAuthUser,
    Extension(theme): Extension<CurrentTheme>,
    Path(slug): Path<String>,
) -> Result<Response> {
    let community = CommunityModel::find_by_slug(&ctx.db, &slug)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    let is_member = if let Some(ref user) = current_user {
        CommunityModel::get_member_role(&ctx.db, community.id, user.id)
            .await
            .map_err(|_| Error::InternalServerError)?
            .is_some()
    } else {
        false
    };

    let (posts, _num_pages) = PostModel::list_for_community(&ctx.db, community.id, 0, PER_PAGE)
        .await
        .map_err(|_| Error::InternalServerError)?;

    let description_rendered = community
        .description
        .as_deref()
        .map(render_markdown)
        .unwrap_or_default();

    helpers::render_with_theme(
        &v,
        "communities/feed.html",
        json!({
            "current_user": current_user,
            "community": community,
            "description_rendered": description_rendered,
            "is_member": is_member,
            "posts": posts,
        }),
        &theme,
    )
}

#[instrument(skip(v, ctx, theme))]
pub async fn new_post_form(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    Extension(theme): Extension<CurrentTheme>,
    Path(slug): Path<String>,
) -> Result<Response> {
    let community = CommunityModel::find_by_slug(&ctx.db, &slug)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    let role = CommunityModel::get_member_role(&ctx.db, community.id, current_user.id)
        .await
        .map_err(|_| Error::InternalServerError)?;

    if role.is_none() {
        return Err(Error::Unauthorized("You must join to post".to_string()));
    }

    helpers::render_with_theme(
        &v,
        "communities/new_post.html",
        json!({
            "current_user": current_user,
            "community": community,
        }),
        &theme,
    )
}

#[instrument(skip(ctx, form))]
pub async fn create_post(
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    Extension(taverna): Extension<TavernaConfig>,
    Path(slug): Path<String>,
    Form(form): Form<CreatePostForm>,
) -> Result<Response> {
    let community = CommunityModel::find_by_slug(&ctx.db, &slug)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    let role = CommunityModel::get_member_role(&ctx.db, community.id, current_user.id)
        .await
        .map_err(|_| Error::InternalServerError)?;

    if role.is_none() {
        return Err(Error::Unauthorized("You must join to post".to_string()));
    }

    PostModel::create(
        &ctx.db,
        Some(community.id),
        current_user.id,
        &form.title,
        form.body.as_deref(),
        form.url.as_deref(),
        &taverna.base_url,
    )
    .await
    .map_err(|_| Error::InternalServerError)?;

    Ok(format::redirect(&format!("/c/{}", slug)).into_response())
}

#[instrument(skip(ctx))]
pub async fn join(
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    Path(slug): Path<String>,
) -> Result<Response> {
    let community = CommunityModel::find_by_slug(&ctx.db, &slug)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    CommunityModel::add_member(&ctx.db, community.id, current_user.id, "member")
        .await
        .map_err(|_| Error::InternalServerError)?;

    Ok(format::redirect(&format!("/c/{}", slug)).into_response())
}

#[instrument(skip(ctx))]
pub async fn leave(
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    Path(slug): Path<String>,
) -> Result<Response> {
    let community = CommunityModel::find_by_slug(&ctx.db, &slug)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    CommunityModel::remove_member(&ctx.db, community.id, current_user.id)
        .await
        .map_err(|_| Error::InternalServerError)?;

    Ok(format::redirect(&format!("/c/{}", slug)).into_response())
}

#[instrument(skip(v, ctx, theme))]
pub async fn settings_form(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    Extension(theme): Extension<CurrentTheme>,
    Path(slug): Path<String>,
) -> Result<Response> {
    let community = CommunityModel::find_by_slug(&ctx.db, &slug)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    let role = CommunityModel::get_member_role(&ctx.db, community.id, current_user.id)
        .await
        .map_err(|_| Error::InternalServerError)?;

    if role.as_deref() != Some("moderator") && !current_user.is_admin {
        return Err(Error::Unauthorized("Moderator access required".to_string()));
    }

    helpers::render_with_theme(
        &v,
        "communities/settings.html",
        json!({
            "current_user": current_user,
            "community": community,
        }),
        &theme,
    )
}

#[instrument(skip(ctx, form))]
pub async fn update_settings(
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    Path(slug): Path<String>,
    Form(form): Form<CommunitySettingsForm>,
) -> Result<Response> {
    let community = CommunityModel::find_by_slug(&ctx.db, &slug)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    let role = CommunityModel::get_member_role(&ctx.db, community.id, current_user.id)
        .await
        .map_err(|_| Error::InternalServerError)?;

    if role.as_deref() != Some("moderator") && !current_user.is_admin {
        return Err(Error::Unauthorized("Moderator access required".to_string()));
    }

    CommunityModel::update_settings(
        &ctx.db,
        community.id,
        form.name.as_deref(),
        form.description.as_deref(),
        form.is_nsfw.map(|v| v == "on"),
        form.posting_restricted.map(|v| v == "on"),
    )
    .await
    .map_err(|_| Error::InternalServerError)?;

    Ok(format::redirect(&format!("/c/{}/settings", slug)).into_response())
}
