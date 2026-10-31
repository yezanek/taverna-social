use crate::controllers::errors::{forbidden, invalid_csrf};
use axum::extract::{Path, Query};
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::{Extension, Form};
use axum_extra::extract::cookie::CookieJar;
use loco_rs::prelude::*;
use serde::Deserialize;
use serde_json::json;
use tracing::instrument;

use crate::common::config::TavernaConfig;
use crate::common::markdown::render_markdown;
use crate::controllers::csrf::{self, CsrfForm};
use crate::federation::{self, outbound, traits::DbCommunity};
use crate::initializers::auth_middleware::{AuthUser, OptionalAuthUser};
use crate::initializers::content_negotiation::RenderMode;
use crate::initializers::theme_middleware::CurrentTheme;
use crate::models::communities::CommunityModel;
use crate::models::community_bans::Entity as CommunityBansEntity;
use crate::models::feed::{self, CursorQuery};
use crate::models::posts::{PostError, PostModel};
use crate::models::users::UserModel;
use crate::views::helpers;
use activitypub_federation::{config::Data, traits::Object};
use sea_orm::DatabaseConnection;

const PER_PAGE: u64 = 20;

/// Form data for creating a community post.
#[derive(Debug, Deserialize)]
pub struct CreatePostForm {
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub csrf_token: Option<String>,
}

/// Form data for updating community settings.
#[derive(Debug, Deserialize)]
pub struct CommunitySettingsForm {
    pub name: Option<String>,
    pub description: Option<String>,
    pub is_nsfw: Option<String>,
    pub posting_restricted: Option<String>,
    #[serde(default)]
    pub csrf_token: Option<String>,
}

/// Lists all communities.
#[instrument(skip(v, ctx, theme, current_user, jar))]
pub async fn list(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    OptionalAuthUser(current_user): OptionalAuthUser,
    jar: CookieJar,
    Extension(theme): Extension<CurrentTheme>,
    Query(page): Query<CursorQuery>,
) -> Result<Response> {
    let communities = CommunityModel::list_all(&ctx.db, page.cursor().as_ref(), PER_PAGE).await?;

    let (csrf_token, csrf_cookie) = csrf::issue();
    let response = helpers::render_with_theme(
        &v,
        "communities/list.html",
        json!({
            "current_user": current_user,
            "communities": communities.items,
            "next_cursor": communities.next_cursor,
            "csrf_token": csrf_token,
        }),
        &theme,
    )?;
    Ok((jar.add(csrf_cookie), response).into_response())
}

/// Shows a community's feed (posts) or its ActivityPub actor representation.
#[allow(clippy::too_many_arguments)]
#[instrument(skip(v, ctx, theme, current_user, jar, data))]
pub async fn feed(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    OptionalAuthUser(current_user): OptionalAuthUser,
    jar: CookieJar,
    Extension(theme): Extension<CurrentTheme>,
    Extension(render_mode): Extension<RenderMode>,
    data: Data<DatabaseConnection>,
    Path(slug): Path<String>,
    Query(page): Query<CursorQuery>,
) -> Result<Response> {
    let community = CommunityModel::find_by_slug(&ctx.db, &slug)
        .await?
        .ok_or_else(|| Error::NotFound)?;

    match render_mode {
        RenderMode::ActivityPub => {
            let group = DbCommunity::new(community)
                .map_err(Error::wrap)?
                .into_json(&data)
                .await
                .map_err(federation::internal)?;
            return Ok(federation::ap_response(group));
        }
        RenderMode::Html => {}
    }

    let is_member = if let Some(ref user) = current_user {
        CommunityModel::get_member_role(&ctx.db, community.id, user.id)
            .await?
            .is_some()
    } else {
        false
    };

    let posts =
        feed::community_feed(&ctx.db, community.id, page.cursor().as_ref(), PER_PAGE).await?;

    let description_rendered = community
        .description
        .as_deref()
        .map(render_markdown)
        .unwrap_or_default();

    let (csrf_token, csrf_cookie) = csrf::issue();
    let response = helpers::render_with_theme(
        &v,
        "communities/feed.html",
        json!({
            "current_user": current_user,
            "community": community,
            "description_rendered": description_rendered,
            "is_member": is_member,
            "posts": posts.items,
            "next_cursor": posts.next_cursor,
            "csrf_token": csrf_token,
        }),
        &theme,
    )?;
    Ok((jar.add(csrf_cookie), response).into_response())
}

/// Form data for creating a new community.
#[derive(Debug, Deserialize)]
pub struct CreateCommunityForm {
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    #[serde(default)]
    pub csrf_token: Option<String>,
}

/// Shows the form to create a new community (with eligibility check).
#[instrument(skip(v, ctx, theme, current_user, jar))]
pub async fn new_community_form(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(theme): Extension<CurrentTheme>,
) -> Result<Response> {
    let eligibility =
        crate::models::eligibility::check_eligibility(&ctx.db, current_user.id).await?;

    let (csrf_token, csrf_cookie) = csrf::issue();

    let response = helpers::render_with_theme(
        &v,
        "communities/new.html",
        json!({
            "current_user": current_user,
            "eligible": eligibility.eligible,
            "eligibility_reasons": eligibility.reasons,
            "csrf_token": csrf_token,
        }),
        &theme,
    )?;
    Ok((jar.add(csrf_cookie), response).into_response())
}

/// Creates a new community (with eligibility check).
#[instrument(skip(ctx, jar, headers, taverna))]
pub async fn create_community(
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    Form(form): Form<CreateCommunityForm>,
) -> Result<Response> {
    if !csrf::validate(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(invalid_csrf());
    }

    let eligibility =
        crate::models::eligibility::check_eligibility(&ctx.db, current_user.id).await?;

    if !eligibility.eligible {
        return Err(Error::CustomError(
            axum::http::StatusCode::FORBIDDEN,
            loco_rs::controller::ErrorDetail::with_reason(eligibility.reasons.join("; ")),
        ));
    }

    let community = CommunityModel::create(
        &ctx.db,
        &form.slug,
        &form.name,
        form.description.as_deref(),
        current_user.id,
        &taverna.base_url,
    )
    .await
    .map_err(|e| Error::BadRequest(e.to_string()))?;

    Ok(format::redirect(&format!("/c/{}", community.slug)).into_response())
}

/// Renders the form to create a new post in a community.
#[instrument(skip(v, ctx, theme, current_user, jar))]
pub async fn new_post_form(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(theme): Extension<CurrentTheme>,
    Path(slug): Path<String>,
) -> Result<Response> {
    let community = CommunityModel::find_by_slug(&ctx.db, &slug)
        .await?
        .ok_or_else(|| Error::NotFound)?;

    let role = CommunityModel::get_member_role(&ctx.db, community.id, current_user.id).await?;

    if role.is_none() {
        return Err(forbidden("You must join to post"));
    }

    let (csrf_token, csrf_cookie) = csrf::issue();
    let response = helpers::render_with_theme(
        &v,
        "communities/new_post.html",
        json!({
            "current_user": current_user,
            "community": community,
            "csrf_token": csrf_token,
        }),
        &theme,
    )?;
    Ok((jar.add(csrf_cookie), response).into_response())
}

/// Handles community post creation with federation support.
#[instrument(skip(ctx, form, jar, headers, taverna))]
pub async fn create_post(
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Form(form): Form<CreatePostForm>,
) -> Result<Response> {
    let community = CommunityModel::find_by_slug(&ctx.db, &slug)
        .await?
        .ok_or_else(|| Error::NotFound)?;

    let role = CommunityModel::get_member_role(&ctx.db, community.id, current_user.id).await?;

    if role.is_none() {
        return Err(forbidden("You must join to post"));
    }

    // `posting_restricted` communities only accept posts from moderators.
    if community.posting_restricted
        && !CommunityModel::can_moderate(&ctx.db, community.id, &current_user).await?
    {
        return Err(forbidden("Only moderators can post in this community"));
    }

    if !csrf::validate(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(invalid_csrf());
    }

    let post = PostModel::create(
        &ctx.db,
        Some(community.id),
        current_user.id,
        &form.title,
        form.body.as_deref(),
        form.url.as_deref(),
        &taverna.base_url,
    )
    .await
    .map_err(|e| match e {
        PostError::Banned => forbidden("You are banned from this community"),
        PostError::Validation(msg) => Error::BadRequest(msg),
        e => Error::wrap(e),
    })?;

    if taverna.federation.enabled {
        let note = outbound::post_note(
            &post,
            &current_user.ap_id,
            Some(&community),
            &current_user.followers_url,
        );
        let create = outbound::wrap("Create", &current_user.ap_id, note);
        outbound::send_to_community_followers(&ctx.db, &create, &current_user.ap_id, community.id)
            .await;
    }

    Ok(format::redirect(&format!("/c/{slug}")).into_response())
}

/// Joins the current user to a community as a member. For a remote
/// community this also sends a Follow to its inbox.
#[instrument(skip(ctx, jar, headers, taverna, form))]
pub async fn join(
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Form(form): Form<CsrfForm>,
) -> Result<Response> {
    let community = CommunityModel::find_by_slug(&ctx.db, &slug)
        .await?
        .ok_or_else(|| Error::NotFound)?;

    if !csrf::validate(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(invalid_csrf());
    }

    CommunityModel::add_member(&ctx.db, community.id, current_user.id, "member").await?;

    if taverna.federation.enabled && !community.is_local {
        let follow = outbound::follow(&current_user.ap_id, &community.ap_id, community.id);
        outbound::send(
            &ctx.db,
            &follow,
            &current_user.ap_id,
            vec![community.inbox_url.clone()],
        )
        .await;
    }
    Ok(format::redirect(&format!("/c/{slug}")).into_response())
}

/// Removes the current user from a community's member list. For a remote
/// community this also sends Undo{Follow}.
#[instrument(skip(ctx, jar, headers, taverna, form))]
pub async fn leave(
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Form(form): Form<CsrfForm>,
) -> Result<Response> {
    let community = CommunityModel::find_by_slug(&ctx.db, &slug)
        .await?
        .ok_or_else(|| Error::NotFound)?;
    if !csrf::validate(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(invalid_csrf());
    }
    CommunityModel::remove_member(&ctx.db, community.id, current_user.id).await?;

    if taverna.federation.enabled && !community.is_local {
        let undo = outbound::undo_follow(&current_user.ap_id, &community.ap_id, community.id);
        outbound::send(
            &ctx.db,
            &undo,
            &current_user.ap_id,
            vec![community.inbox_url.clone()],
        )
        .await;
    }
    Ok(format::redirect(&format!("/c/{slug}")).into_response())
}

/// Renders community settings form (moderator only).
#[instrument(skip(v, ctx, theme, current_user, jar))]
pub async fn settings_form(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(theme): Extension<CurrentTheme>,
    Path(slug): Path<String>,
) -> Result<Response> {
    let community = CommunityModel::find_by_slug(&ctx.db, &slug)
        .await?
        .ok_or_else(|| Error::NotFound)?;

    let role = CommunityModel::get_member_role(&ctx.db, community.id, current_user.id).await?;

    if role.as_deref() != Some("moderator") && !current_user.is_admin {
        return Err(forbidden("Moderator access required"));
    }

    let (csrf_token, csrf_cookie) = csrf::issue();
    let response = helpers::render_with_theme(
        &v,
        "communities/settings.html",
        json!({
            "current_user": current_user,
            "community": community,
            "csrf_token": csrf_token,
        }),
        &theme,
    )?;
    Ok((jar.add(csrf_cookie), response).into_response())
}

/// Updates community settings (moderator only).
#[instrument(skip(ctx, form, jar, headers, taverna))]
pub async fn update_settings(
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Form(form): Form<CommunitySettingsForm>,
) -> Result<Response> {
    let community = CommunityModel::find_by_slug(&ctx.db, &slug)
        .await?
        .ok_or_else(|| Error::NotFound)?;

    let role = CommunityModel::get_member_role(&ctx.db, community.id, current_user.id).await?;

    if role.as_deref() != Some("moderator") && !current_user.is_admin {
        return Err(forbidden("Moderator access required"));
    }

    if !csrf::validate(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(invalid_csrf());
    }

    CommunityModel::update_settings(
        &ctx.db,
        community.id,
        form.name.as_deref(),
        form.description.as_deref(),
        // Unchecked checkboxes are absent from the form, so absence means off.
        Some(form.is_nsfw.is_some()),
        Some(form.posting_restricted.is_some()),
    )
    .await
    .map_err(Error::wrap)?;

    Ok(format::redirect(&format!("/c/{}/settings", slug)).into_response())
}

/// Bans a user from the community (moderator only).
#[instrument(skip(ctx, jar, headers, taverna, form))]
pub async fn ban_user(
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    Path((slug, username)): Path<(String, String)>,
    Form(form): Form<CsrfForm>,
) -> Result<Response> {
    let community = CommunityModel::find_by_slug(&ctx.db, &slug)
        .await?
        .ok_or_else(|| Error::NotFound)?;

    let role = CommunityModel::get_member_role(&ctx.db, community.id, current_user.id).await?;

    if role.as_deref() != Some("moderator") && !current_user.is_admin {
        return Err(forbidden("Moderator access required"));
    }

    if !csrf::validate(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(invalid_csrf());
    }

    let target = UserModel::find_by_username(&ctx.db, &username)
        .await?
        .ok_or_else(|| Error::NotFound)?;

    CommunityBansEntity::create_ban(&ctx.db, community.id, target.id, current_user.id, None)
        .await?;

    Ok(format::redirect(&format!("/c/{}/settings", slug)).into_response())
}

/// Unbans a user from the community (moderator only).
#[instrument(skip(ctx, jar, headers, taverna, form))]
pub async fn unban_user(
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    Path((slug, username)): Path<(String, String)>,
    Form(form): Form<CsrfForm>,
) -> Result<Response> {
    let community = CommunityModel::find_by_slug(&ctx.db, &slug)
        .await?
        .ok_or_else(|| Error::NotFound)?;

    let role = CommunityModel::get_member_role(&ctx.db, community.id, current_user.id).await?;

    if role.as_deref() != Some("moderator") && !current_user.is_admin {
        return Err(forbidden("Moderator access required"));
    }

    if !csrf::validate(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(invalid_csrf());
    }

    let target = UserModel::find_by_username(&ctx.db, &username)
        .await?
        .ok_or_else(|| Error::NotFound)?;

    CommunityBansEntity::remove_ban(&ctx.db, community.id, target.id).await?;

    Ok(format::redirect(&format!("/c/{}/settings", slug)).into_response())
}
