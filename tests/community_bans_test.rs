//! The community ban UI on the settings page (issue #14).
mod common;

#[cfg(test)]
mod tests {
    use axum_extra::extract::cookie::Cookie;
    use serial_test::serial;
    use taverna::models::_entities::users;
    use taverna::models::communities::CommunityModel;
    use taverna::models::community_bans::Entity as CommunityBans;
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
    async fn moderator_bans_and_unbans_from_the_settings_page() {
        let (server, ctx) = crate::common::test_server_and_ctx().await;
        let owner = user(&ctx.db, "owner").await;
        let troll = user(&ctx.db, "troll").await;
        let community = CommunityModel::create(&ctx.db, "pub", "Pub", None, owner.id, BASE)
            .await
            .unwrap();
        let (cookies, csrf) = session(&server, "owner").await;

        // The feed links moderators to the settings page.
        let feed = with_cookies(server.get("/c/pub"), &cookies).await;
        assert!(feed.text().contains("/c/pub/settings"));

        let resp = with_cookies(server.post("/c/pub/bans"), &cookies)
            .form(&serde_json::json!({
                "username": "@troll",
                "reason": "Spam",
                "csrf_token": csrf,
            }))
            .await;
        assert_eq!(resp.status_code(), 303);
        assert!(
            CommunityBans::is_banned(&ctx.db, community.id, troll.id)
                .await
                .unwrap()
        );

        // Banning again is not an error; it updates the reason.
        let resp = with_cookies(server.post("/c/pub/bans"), &cookies)
            .form(&serde_json::json!({
                "username": "troll",
                "reason": "Still spam",
                "csrf_token": csrf,
            }))
            .await;
        assert_eq!(resp.status_code(), 303);

        let page = with_cookies(server.get("/c/pub/settings"), &cookies).await;
        let html = page.text();
        assert!(html.contains("/c/pub/bans/troll/delete"));
        assert!(html.contains("Still spam"));

        let resp = with_cookies(server.post("/c/pub/bans/troll/delete"), &cookies)
            .form(&serde_json::json!({ "csrf_token": csrf }))
            .await;
        assert_eq!(resp.status_code(), 303);
        assert!(
            !CommunityBans::is_banned(&ctx.db, community.id, troll.id)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    #[serial]
    async fn ban_form_reports_unknown_users_and_self_bans() {
        let (server, ctx) = crate::common::test_server_and_ctx().await;
        let owner = user(&ctx.db, "owner").await;
        CommunityModel::create(&ctx.db, "pub", "Pub", None, owner.id, BASE)
            .await
            .unwrap();
        let (cookies, csrf) = session(&server, "owner").await;

        let resp = with_cookies(server.post("/c/pub/bans"), &cookies)
            .form(&serde_json::json!({ "username": "nobody", "csrf_token": csrf }))
            .await;
        assert_eq!(resp.status_code(), 200);
        assert!(resp.text().contains("No user named"));

        let resp = with_cookies(server.post("/c/pub/bans"), &cookies)
            .form(&serde_json::json!({ "username": "owner", "csrf_token": csrf }))
            .await;
        assert_eq!(resp.status_code(), 200);
        assert!(resp.text().contains("You can"));
    }

    #[tokio::test]
    #[serial]
    async fn non_moderators_cannot_ban() {
        let (server, ctx) = crate::common::test_server_and_ctx().await;
        let owner = user(&ctx.db, "owner").await;
        user(&ctx.db, "member").await;
        let community = CommunityModel::create(&ctx.db, "pub", "Pub", None, owner.id, BASE)
            .await
            .unwrap();
        let (cookies, csrf) = session(&server, "member").await;

        let feed = with_cookies(server.get("/c/pub"), &cookies).await;
        assert!(!feed.text().contains("/c/pub/settings"));

        let resp = with_cookies(server.post("/c/pub/bans"), &cookies)
            .form(&serde_json::json!({ "username": "owner", "csrf_token": csrf }))
            .await;
        assert_eq!(resp.status_code(), 403);
        assert!(
            !CommunityBans::is_banned(&ctx.db, community.id, owner.id)
                .await
                .unwrap()
        );
    }
}
