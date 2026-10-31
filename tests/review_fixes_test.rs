//! Regression tests for the HTTP flows fixed in the spec/quality review:
//! CSRF on token-only forms, comment counting, self-votes, the theme
//! redirect, and refresh-token sessions.
mod common;

#[cfg(test)]
mod tests {
    use axum_extra::extract::cookie::Cookie;
    use serial_test::serial;
    use taverna::common::jwt::{create_access_token, create_refresh_token};
    use taverna::models::_entities::users;
    use taverna::models::communities::CommunityModel;
    use taverna::models::posts::PostModel;
    use taverna::models::users::UserModel;

    const BASE: &str = "http://localhost:5150";

    async fn user(db: &sea_orm::DatabaseConnection, name: &str) -> users::Model {
        UserModel::create_local(
            db,
            name,
            &format!("{name}@example.com"),
            "password123",
            BASE,
        )
        .await
        .unwrap()
    }

    /// Logs in and returns the session cookies plus a CSRF token/cookie pair.
    async fn session(server: &axum_test::TestServer, name: &str) -> (Vec<Cookie<'static>>, String) {
        let csrf = server
            .get("/login")
            .await
            .cookie("csrf_token")
            .value()
            .to_string();
        let resp = server
            .post("/login")
            .add_cookie(Cookie::new("csrf_token", csrf.clone()))
            .form(&serde_json::json!({
                "identifier": name,
                "password": "password123",
                "csrf_token": csrf,
            }))
            .await;
        let mut cookies: Vec<Cookie<'static>> = resp
            .cookies()
            .iter()
            .map(|c| Cookie::new(c.name().to_string(), c.value().to_string()))
            .collect();
        let csrf = uuid::Uuid::new_v4().to_string();
        cookies.push(Cookie::new("csrf_token", csrf.clone()));
        (cookies, csrf)
    }

    fn with_cookies(
        mut req: axum_test::TestRequest,
        cookies: &[Cookie<'static>],
    ) -> axum_test::TestRequest {
        for c in cookies {
            req = req.add_cookie(c.clone());
        }
        req
    }

    #[tokio::test]
    #[serial]
    async fn join_and_leave_accept_the_form_csrf_token() {
        let (server, ctx) = crate::common::test_server_and_ctx().await;
        let owner = user(&ctx.db, "owner").await;
        let member = user(&ctx.db, "member").await;
        let community = CommunityModel::create(&ctx.db, "pub", "Pub", None, owner.id, BASE)
            .await
            .unwrap();
        let (cookies, csrf) = session(&server, "member").await;

        let resp = with_cookies(server.post("/c/pub/join"), &cookies)
            .form(&serde_json::json!({ "csrf_token": csrf }))
            .await;
        assert!(
            resp.status_code().is_redirection(),
            "join: {}",
            resp.status_code()
        );
        let role = CommunityModel::get_member_role(&ctx.db, community.id, member.id)
            .await
            .unwrap();
        assert_eq!(role.as_deref(), Some("member"));

        let resp = with_cookies(server.post("/c/pub/leave"), &cookies)
            .form(&serde_json::json!({ "csrf_token": csrf }))
            .await;
        assert!(
            resp.status_code().is_redirection(),
            "leave: {}",
            resp.status_code()
        );
        let role = CommunityModel::get_member_role(&ctx.db, community.id, member.id)
            .await
            .unwrap();
        assert!(role.is_none());

        // Without the token the request is still refused.
        let resp = with_cookies(server.post("/c/pub/join"), &cookies)
            .form(&serde_json::json!({}))
            .await;
        assert_eq!(resp.status_code(), 401);
    }

    #[tokio::test]
    #[serial]
    async fn commenting_over_http_counts_the_comment_once() {
        let (server, ctx) = crate::common::test_server_and_ctx().await;
        let author = user(&ctx.db, "author").await;
        user(&ctx.db, "replier").await;
        let community = CommunityModel::create(&ctx.db, "talk", "Talk", None, author.id, BASE)
            .await
            .unwrap();
        let post = PostModel::create(
            &ctx.db,
            Some(community.id),
            author.id,
            "Hello",
            Some("body"),
            None,
            BASE,
        )
        .await
        .unwrap();
        let (cookies, csrf) = session(&server, "replier").await;

        let resp = with_cookies(
            server.post(&format!("/c/talk/posts/{}/comments", post.id)),
            &cookies,
        )
        .form(&serde_json::json!({ "body": "first!", "csrf_token": csrf }))
        .await;
        assert!(
            resp.status_code().is_redirection(),
            "{}",
            resp.status_code()
        );

        let post = PostModel::find_by_id(&ctx.db, post.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(post.comment_count, 1);
    }

    #[tokio::test]
    #[serial]
    async fn voting_on_your_own_post_is_forbidden() {
        let (server, ctx) = crate::common::test_server_and_ctx().await;
        let author = user(&ctx.db, "selfvoter").await;
        let community = CommunityModel::create(&ctx.db, "mirror", "Mirror", None, author.id, BASE)
            .await
            .unwrap();
        let post = PostModel::create(
            &ctx.db,
            Some(community.id),
            author.id,
            "Mine",
            Some("body"),
            None,
            BASE,
        )
        .await
        .unwrap();
        let (cookies, csrf) = session(&server, "selfvoter").await;

        let resp = with_cookies(
            server.post(&format!("/c/mirror/posts/{}/vote", post.id)),
            &cookies,
        )
        .form(&serde_json::json!({ "vote": "1", "csrf_token": csrf }))
        .await;
        assert_eq!(resp.status_code(), 403);

        let post = PostModel::find_by_id(&ctx.db, post.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(post.score, 0);
    }

    #[tokio::test]
    #[serial]
    async fn theme_redirect_only_follows_local_referers() {
        let (server, ctx) = crate::common::test_server_and_ctx().await;
        user(&ctx.db, "themer").await;
        let base = taverna::common::config::TavernaConfig::from_env()
            .unwrap()
            .base_url;

        let resp = server
            .post("/theme")
            .add_header("Referer", "https://evil.example/phish")
            .form(&serde_json::json!({ "theme": "dark" }))
            .await;
        assert_eq!(resp.header("location"), "/");

        let resp = server
            .post("/theme")
            .add_header("Referer", format!("{base}/c/pub?cursor=abc"))
            .form(&serde_json::json!({ "theme": "dark" }))
            .await;
        assert_eq!(resp.header("location"), "/c/pub?cursor=abc");
    }

    #[tokio::test]
    #[serial]
    async fn refresh_token_renews_an_expired_session() {
        let (server, ctx) = crate::common::test_server_and_ctx().await;
        let account = user(&ctx.db, "sleeper").await;
        let secret = ctx.config.get_jwt_config().unwrap().secret.clone();
        let refresh = create_refresh_token(&account, &secret).unwrap();

        // Only the refresh cookie: the access token has expired and is gone.
        let resp = server
            .get("/settings")
            .add_cookie(Cookie::new("refresh_token", refresh.clone()))
            .await;
        assert_eq!(resp.status_code(), 200);
        let renewed = resp.cookie("access_token");
        assert!(!renewed.value().is_empty());

        // A refresh token presented as the access token is refused.
        let resp = server
            .get("/settings")
            .add_cookie(Cookie::new("access_token", refresh))
            .await;
        assert_eq!(resp.status_code(), 401);

        // A normal access token still works on its own.
        let access = create_access_token(&account, &secret).unwrap();
        let resp = server
            .get("/settings")
            .add_cookie(Cookie::new("access_token", access))
            .await;
        assert_eq!(resp.status_code(), 200);
    }
}
