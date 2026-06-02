use axum::extract::Path;
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::{Extension, Form};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use loco_rs::prelude::*;
use serde::Deserialize;
use serde_json::json;
use tracing::instrument;

use crate::common::config::TavernaConfig;
use crate::common::markdown::render_markdown;
use crate::controllers::auth::validate_csrf;
use crate::initializers::auth_middleware::{AuthUser, OptionalAuthUser};
use crate::initializers::content_negotiation::RenderMode;
use crate::initializers::theme_middleware::CurrentTheme;
use crate::models::communities::CommunityModel;
use crate::models::community_bans::Entity as CommunityBansEntity;
use crate::models::posts::PostModel;
use crate::models::users::UserModel;
use crate::views::helpers;

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
) -> Result<Response> {
    let page = 0;
    let (communities, _num_pages) = CommunityModel::list_all(&ctx.db, page, PER_PAGE)
        .await
        .map_err(|_| Error::InternalServerError)?;

    let csrf_token = uuid::Uuid::new_v4().to_string();
    let csrf_cookie = Cookie::build(("csrf_token", csrf_token.clone()))
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(cfg!(not(debug_assertions)))
        .path("/")
        .build();
    let response = helpers::render_with_theme(
        &v,
        "communities/list.html",
        json!({
            "current_user": current_user,
            "communities": communities,
            "csrf_token": csrf_token,
        }),
        &theme,
    )?;
    Ok((jar.add(csrf_cookie), response).into_response())
}

/// Shows a community's feed (posts) or its ActivityPub actor representation.
#[instrument(skip(v, ctx, theme, current_user, jar))]
pub async fn feed(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    OptionalAuthUser(current_user): OptionalAuthUser,
    jar: CookieJar,
    Extension(theme): Extension<CurrentTheme>,
    Extension(render_mode): Extension<RenderMode>,
    Path(slug): Path<String>,
) -> Result<Response> {
    let community = CommunityModel::find_by_slug(&ctx.db, &slug)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    match render_mode {
        RenderMode::ActivityPub => {
            let actor = json!({
                "@context": [
                    "https://www.w3.org/ns/activitystreams",
                    "https://w3id.org/security/v1"
                ],
                "id": community.ap_id,
                "type": "Group",
                "preferredUsername": community.slug,
                "name": community.name,
                "summary": community.description,
                "inbox": format!("{}/inbox", community.ap_id),
                "outbox": format!("{}/outbox", community.ap_id),
                "followers": format!("{}/followers", community.ap_id),
                "publicKey": {
                    "id": format!("{}#main-key", community.ap_id),
                    "owner": community.ap_id,
                    "publicKeyPem": community.public_key,
                },
                "published": community.created_at.format("%+").to_string(),
                "attributedTo": community.ap_id,
                "postingRestrictedToMods": community.posting_restricted,
                "sensitive": community.is_nsfw,
            });
            return Ok(axum::Json(actor).into_response());
        }
        RenderMode::Html => {}
    }

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

    let csrf_token = uuid::Uuid::new_v4().to_string();
    let csrf_cookie = Cookie::build(("csrf_token", csrf_token.clone()))
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(cfg!(not(debug_assertions)))
        .path("/")
        .build();
    let response = helpers::render_with_theme(
        &v,
        "communities/feed.html",
        json!({
            "current_user": current_user,
            "community": community,
            "description_rendered": description_rendered,
            "is_member": is_member,
            "posts": posts,
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
    let eligibility = crate::models::eligibility::check_eligibility(&ctx.db, current_user.id)
        .await
        .map_err(|_| Error::InternalServerError)?;

    let csrf_token = uuid::Uuid::new_v4().to_string();
    let csrf_cookie = Cookie::build(("csrf_token", csrf_token.clone()))
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(cfg!(not(debug_assertions)))
        .path("/")
        .build();

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
    use crate::controllers::auth::validate_csrf;

    if !validate_csrf(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(Error::Unauthorized("Invalid CSRF token".to_string()));
    }

    let eligibility = crate::models::eligibility::check_eligibility(&ctx.db, current_user.id)
        .await
        .map_err(|_| Error::InternalServerError)?;

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
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    let role = CommunityModel::get_member_role(&ctx.db, community.id, current_user.id)
        .await
        .map_err(|_| Error::InternalServerError)?;

    if role.is_none() {
        return Err(Error::Unauthorized("You must join to post".to_string()));
    }

    let csrf_token = uuid::Uuid::new_v4().to_string();
    let csrf_cookie = Cookie::build(("csrf_token", csrf_token.clone()))
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(cfg!(not(debug_assertions)))
        .path("/")
        .build();
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
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    let role = CommunityModel::get_member_role(&ctx.db, community.id, current_user.id)
        .await
        .map_err(|_| Error::InternalServerError)?;

    if role.is_none() {
        return Err(Error::Unauthorized("You must join to post".to_string()));
    }

    if !validate_csrf(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(Error::Unauthorized("Invalid CSRF token".to_string()));
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
        crate::models::posts::PostError::Banned => {
            Error::Unauthorized("You are banned from this community".to_string())
        }
        _ => Error::InternalServerError,
    })?;

    // Federation: enqueue Create activity to community followers
    if taverna.federation.enabled {
        let author = crate::models::users::UserModel::find_by_id(&ctx.db, current_user.id)
            .await
            .map_err(|_| Error::InternalServerError)?
            .ok_or_else(|| Error::InternalServerError)?;

        let ap_context: &[&str] = &[
            "https://www.w3.org/ns/activitystreams",
            "https://w3id.org/security/v1",
        ];

        let note_value = serde_json::json!({
            "@context": ap_context,
            "id": post.ap_id,
            "type": "Note",
            "attributedTo": community.ap_id,
            "content": post.body,
            "name": post.title,
            "published": post.published_at.to_rfc3339(),
            "url": post.url,
            "to": [community.followers_url],
            "cc": ["https://www.w3.org/ns/activitystreams#Public"],
        });

        let activity_id = format!("{}/activities/{}", author.ap_id, uuid::Uuid::new_v4());
        let create = serde_json::json!({
            "@context": ap_context,
            "id": activity_id,
            "type": "Create",
            "actor": author.ap_id,
            "object": note_value,
            "to": [community.followers_url],
            "cc": ["https://www.w3.org/ns/activitystreams#Public"],
            "published": chrono::Utc::now().to_rfc3339(),
        });

        let inboxes = crate::workers::deliver_activity::get_follower_inboxes_for_community(
            &ctx.db,
            community.id,
        )
        .await
        .unwrap_or_default();
        if !inboxes.is_empty()
            && let Err(e) = crate::workers::deliver_activity::enqueue_to_followers(
                &ctx.db,
                &create,
                &author.ap_id,
                inboxes,
            )
            .await
        {
            tracing::error!(
                "failed to enqueue to followers for activity {}: {:?}",
                create["id"],
                e
            );
        }
    }

    Ok(format::redirect(&format!("/c/{}", slug)).into_response())
}

/// Joins the current user to a community as a member.
#[instrument(skip(ctx, jar, headers, taverna))]
pub async fn join(
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> Result<Response> {
    let community = CommunityModel::find_by_slug(&ctx.db, &slug)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    if !validate_csrf(&jar, None, &headers, &taverna) {
        return Err(Error::Unauthorized("Invalid CSRF token".to_string()));
    }

    CommunityModel::add_member(&ctx.db, community.id, current_user.id, "member")
        .await
        .map_err(|_| Error::InternalServerError)?;

    // Federation: if remote community, send Follow activity
    if taverna.federation.enabled && !community.is_local {
        let ap_context: &[&str] = &[
            "https://www.w3.org/ns/activitystreams",
            "https://w3id.org/security/v1",
        ];
        let activity_id = format!("{}/activities/{}", current_user.ap_id, uuid::Uuid::new_v4());
        let follow_activity = serde_json::json!({
            "@context": ap_context,
            "id": activity_id,
            "type": "Follow",
            "actor": current_user.ap_id,
            "object": community.ap_id,
        });
        let inboxes = vec![community.inbox_url.clone()];
        if !inboxes.is_empty() {
            let _ = crate::workers::deliver_activity::enqueue_to_followers(
                &ctx.db,
                &follow_activity,
                &current_user.ap_id,
                inboxes,
            )
            .await;
        }
    }

    Ok(format::redirect(&format!("/c/{}", slug)).into_response())
}

/// Removes the current user from a community's member list.
#[instrument(skip(ctx, jar, headers, taverna))]
pub async fn leave(
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> Result<Response> {
    let community = CommunityModel::find_by_slug(&ctx.db, &slug)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    if !validate_csrf(&jar, None, &headers, &taverna) {
        return Err(Error::Unauthorized("Invalid CSRF token".to_string()));
    }

    CommunityModel::remove_member(&ctx.db, community.id, current_user.id)
        .await
        .map_err(|_| Error::InternalServerError)?;

    // Federation: if remote community, send Undo{Follow} activity
    if taverna.federation.enabled && !community.is_local {
        let ap_context: &[&str] = &[
            "https://www.w3.org/ns/activitystreams",
            "https://w3id.org/security/v1",
        ];
        let undo_id = format!("{}/activities/{}", current_user.ap_id, uuid::Uuid::new_v4());
        let follow_id = format!("{}/follows/{}", current_user.ap_id, community.id);
        let undo_activity = serde_json::json!({
            "@context": ap_context,
            "id": undo_id,
            "type": "Undo",
            "actor": current_user.ap_id,
            "object": {
                "id": follow_id,
                "type": "Follow",
                "actor": current_user.ap_id,
                "object": community.ap_id,
            },
        });
        let inboxes = vec![community.inbox_url.clone()];
        if !inboxes.is_empty() {
            let _ = crate::workers::deliver_activity::enqueue_to_followers(
                &ctx.db,
                &undo_activity,
                &current_user.ap_id,
                inboxes,
            )
            .await;
        }
    }

    Ok(format::redirect(&format!("/c/{}", slug)).into_response())
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
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    let role = CommunityModel::get_member_role(&ctx.db, community.id, current_user.id)
        .await
        .map_err(|_| Error::InternalServerError)?;

    if role.as_deref() != Some("moderator") && !current_user.is_admin {
        return Err(Error::Unauthorized("Moderator access required".to_string()));
    }

    let csrf_token = uuid::Uuid::new_v4().to_string();
    let csrf_cookie = Cookie::build(("csrf_token", csrf_token.clone()))
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(cfg!(not(debug_assertions)))
        .path("/")
        .build();
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
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    let role = CommunityModel::get_member_role(&ctx.db, community.id, current_user.id)
        .await
        .map_err(|_| Error::InternalServerError)?;

    if role.as_deref() != Some("moderator") && !current_user.is_admin {
        return Err(Error::Unauthorized("Moderator access required".to_string()));
    }

    if !validate_csrf(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(Error::Unauthorized("Invalid CSRF token".to_string()));
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

/// Bans a user from the community (moderator only).
#[instrument(skip(ctx, jar, headers, taverna))]
pub async fn ban_user(
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    Path((slug, username)): Path<(String, String)>,
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

    if !validate_csrf(&jar, None, &headers, &taverna) {
        return Err(Error::Unauthorized("Invalid CSRF token".to_string()));
    }

    let target = UserModel::find_by_username(&ctx.db, &username)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    CommunityBansEntity::create_ban(&ctx.db, community.id, target.id, current_user.id, None)
        .await
        .map_err(|_| Error::InternalServerError)?;

    Ok(format::redirect(&format!("/c/{}/settings", slug)).into_response())
}

/// Unbans a user from the community (moderator only).
#[instrument(skip(ctx, jar, headers, taverna))]
pub async fn unban_user(
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    Path((slug, username)): Path<(String, String)>,
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

    if !validate_csrf(&jar, None, &headers, &taverna) {
        return Err(Error::Unauthorized("Invalid CSRF token".to_string()));
    }

    let target = UserModel::find_by_username(&ctx.db, &username)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    CommunityBansEntity::remove_ban(&ctx.db, community.id, target.id)
        .await
        .map_err(|_| Error::InternalServerError)?;

    Ok(format::redirect(&format!("/c/{}/settings", slug)).into_response())
}
