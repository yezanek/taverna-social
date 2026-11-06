mod common;

#[cfg(test)]
mod tests {
    use axum_extra::extract::cookie::Cookie;
    use serial_test::serial;
    use taverna::models::users::UserModel;

    async fn seed_first_user(db: &sea_orm::DatabaseConnection) {
        UserModel::create_local(
            db,
            "admin",
            "admin@taverna.local",
            "password123",
            "http://localhost:5150",
        )
        .await
        .ok();
    }

    #[tokio::test]
    #[serial]
    async fn test_set_theme_light() {
        let (request, ctx) = crate::common::test_server_and_ctx().await;
        seed_first_user(&ctx.db).await;
        let response = request
            .post("/theme")
            .form(&serde_json::json!({"theme": "light"}))
            .await;
        assert!(response.status_code() == 302 || response.status_code() == 303);
        let jar = response.cookies();
        let cookie = jar.iter().find(|c| c.name() == "taverna_theme");
        assert!(cookie.is_some());
        assert_eq!(cookie.unwrap().value(), "light");
    }

    #[tokio::test]
    #[serial]
    async fn test_set_theme_dark() {
        let (request, ctx) = crate::common::test_server_and_ctx().await;
        seed_first_user(&ctx.db).await;
        let response = request
            .post("/theme")
            .form(&serde_json::json!({"theme": "dark"}))
            .await;
        assert!(response.status_code() == 302 || response.status_code() == 303);
        let jar = response.cookies();
        let cookie = jar.iter().find(|c| c.name() == "taverna_theme");
        assert!(cookie.is_some());
        assert_eq!(cookie.unwrap().value(), "dark");
    }

    #[tokio::test]
    #[serial]
    async fn test_set_theme_sepia() {
        let (request, ctx) = crate::common::test_server_and_ctx().await;
        seed_first_user(&ctx.db).await;
        let response = request
            .post("/theme")
            .form(&serde_json::json!({"theme": "sepia"}))
            .await;
        assert!(response.status_code() == 302 || response.status_code() == 303);
        let jar = response.cookies();
        let cookie = jar.iter().find(|c| c.name() == "taverna_theme");
        assert!(cookie.is_some());
        assert_eq!(cookie.unwrap().value(), "sepia");
    }

    #[tokio::test]
    #[serial]
    async fn test_invalid_theme_defaults_to_light() {
        let (request, ctx) = crate::common::test_server_and_ctx().await;
        seed_first_user(&ctx.db).await;
        let response = request
            .post("/theme")
            .form(&serde_json::json!({"theme": "hotdog"}))
            .await;
        assert!(response.status_code() == 302 || response.status_code() == 303);
        let jar = response.cookies();
        let cookie = jar.iter().find(|c| c.name() == "taverna_theme");
        assert!(cookie.is_some());
        assert_eq!(cookie.unwrap().value(), "light");
    }

    #[tokio::test]
    #[serial]
    async fn test_theme_cookie_max_age() {
        let (request, ctx) = crate::common::test_server_and_ctx().await;
        seed_first_user(&ctx.db).await;
        let response = request
            .post("/theme")
            .form(&serde_json::json!({"theme": "dark"}))
            .await;
        let jar = response.cookies();
        let cookie = jar.iter().find(|c| c.name() == "taverna_theme");
        assert!(cookie.is_some());
        let cookie = cookie.unwrap();
        assert!(cookie.max_age().is_some());
        let max_age = cookie.max_age().unwrap();
        assert_eq!(max_age.whole_seconds(), 31536000);
    }

    #[tokio::test]
    #[serial]
    async fn test_theme_middleware_reads_cookie() {
        let (request, ctx) = crate::common::test_server_and_ctx().await;
        seed_first_user(&ctx.db).await;
        let response = request
            .get("/")
            .add_header("Cookie", "taverna_theme=dark")
            .await;
        assert_eq!(response.status_code(), 200);
    }

    async fn ensure_user(db: &sea_orm::DatabaseConnection, name: &str) -> uuid::Uuid {
        if let Some(u) = UserModel::find_by_username(db, name).await.unwrap() {
            return u.id;
        }
        UserModel::create_local(
            db,
            name,
            &format!("{name}@example.com"),
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap()
        .id
    }

    async fn login(server: &axum_test::TestServer, name: &str) -> Vec<Cookie<'static>> {
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
        resp.cookies()
            .iter()
            .filter(|c| !c.value().is_empty())
            .map(|c| Cookie::new(c.name().to_string(), c.value().to_string()))
            .collect()
    }

    async fn post_appearance(
        server: &axum_test::TestServer,
        cookies: &[Cookie<'static>],
        form: serde_json::Value,
    ) -> axum_test::TestResponse {
        let mut req = server.get("/settings");
        for c in cookies {
            req = req.add_cookie(c.clone());
        }
        let csrf = req.await.cookie("csrf_token").value().to_string();
        let mut form = form;
        form["csrf_token"] = serde_json::json!(csrf);
        let mut req = server
            .post("/settings/appearance")
            .add_cookie(Cookie::new("csrf_token", csrf));
        for c in cookies {
            req = req.add_cookie(c.clone());
        }
        req.form(&form).await
    }

    #[tokio::test]
    #[serial]
    async fn test_accent_cookie_injects_accent_css() {
        let (request, ctx) = crate::common::test_server_and_ctx().await;
        seed_first_user(&ctx.db).await;
        let html = request
            .get("/")
            .add_header("Cookie", "taverna_accent=2f80ed")
            .await
            .text();
        assert!(html.contains("--primary-container:#2f80ed"));
        for theme in ["light", "dark", "sepia"] {
            assert!(html.contains(&format!("[data-theme=\"{theme}\"]{{--primary:")));
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_invalid_accent_cookie_is_ignored() {
        let (request, ctx) = crate::common::test_server_and_ctx().await;
        seed_first_user(&ctx.db).await;
        let html = request
            .get("/")
            .add_header("Cookie", "taverna_accent=red;}body{display:none")
            .await
            .text();
        assert!(!html.contains("--primary-container"));
        assert!(!html.contains("display:none"));
    }

    #[tokio::test]
    #[serial]
    async fn test_save_appearance_persists_and_sets_cookies() {
        let (request, ctx) = crate::common::test_server_and_ctx().await;
        seed_first_user(&ctx.db).await;
        let id = ensure_user(&ctx.db, "appearance_user").await;
        let cookies = login(&request, "appearance_user").await;

        let resp = post_appearance(
            &request,
            &cookies,
            serde_json::json!({"theme": "dark", "accent": "#8B5CF6"}),
        )
        .await;
        assert_eq!(resp.status_code(), 303);
        assert_eq!(resp.cookie("taverna_theme").value(), "dark");
        assert_eq!(resp.cookie("taverna_accent").value(), "8b5cf6");

        let user = UserModel::find_by_id(&ctx.db, id).await.unwrap().unwrap();
        assert_eq!(user.theme.as_deref(), Some("dark"));
        assert_eq!(user.accent_color.as_deref(), Some("#8b5cf6"));

        // A new browser picks the saved appearance up on login.
        let fresh = login(&request, "appearance_user").await;
        let value = |name: &str| {
            fresh
                .iter()
                .find(|c| c.name() == name)
                .map(|c| c.value().to_string())
        };
        assert_eq!(value("taverna_theme").as_deref(), Some("dark"));
        assert_eq!(value("taverna_accent").as_deref(), Some("8b5cf6"));

        // Reset drops the custom accent and expires the cookie.
        let resp = post_appearance(
            &request,
            &cookies,
            serde_json::json!({"theme": "sepia", "accent": "#8b5cf6", "accent_reset": "1"}),
        )
        .await;
        assert_eq!(resp.status_code(), 303);
        let accent = resp.cookie("taverna_accent");
        assert_eq!(accent.value(), "");
        assert_eq!(accent.max_age().map(|d| d.whole_seconds()), Some(0));
        let user = UserModel::find_by_id(&ctx.db, id).await.unwrap().unwrap();
        assert_eq!(user.theme.as_deref(), Some("sepia"));
        assert_eq!(user.accent_color, None);

        // Saving with the untouched default swatch keeps the default accent.
        let resp = post_appearance(
            &request,
            &cookies,
            serde_json::json!({"theme": "light", "accent": "#d9782a"}),
        )
        .await;
        assert_eq!(resp.status_code(), 303);
        let user = UserModel::find_by_id(&ctx.db, id).await.unwrap().unwrap();
        assert_eq!(user.accent_color, None);
    }

    #[tokio::test]
    #[serial]
    async fn test_save_appearance_rejects_bad_values() {
        let (request, ctx) = crate::common::test_server_and_ctx().await;
        seed_first_user(&ctx.db).await;
        let id = ensure_user(&ctx.db, "appearance_bad").await;
        let cookies = login(&request, "appearance_bad").await;

        for form in [
            serde_json::json!({"theme": "hotdog", "accent": "#2f80ed"}),
            serde_json::json!({"theme": "dark", "accent": "red"}),
        ] {
            let resp = post_appearance(&request, &cookies, form).await;
            assert_eq!(resp.status_code(), 400);
        }
        let user = UserModel::find_by_id(&ctx.db, id).await.unwrap().unwrap();
        assert_eq!(user.theme, None);
        assert_eq!(user.accent_color, None);
    }
}
