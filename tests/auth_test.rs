#[cfg(test)]
mod tests {
    use axum_extra::extract::cookie::Cookie;
    use loco_rs::testing::prelude::*;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    use serial_test::serial;
    use taverna::app::App;
    use taverna::models::_entities::users;
    use taverna::models::users::UserModel;

    fn setup() {
        dotenvy::dotenv().ok();
    }

    /// Seed a default admin user so is_first_run returns false.
    /// Call this once before any request-based test.
    async fn seed_first_user(db: &sea_orm::DatabaseConnection) {
        if UserModel::find_by_username(db, "admin")
            .await
            .ok()
            .flatten()
            .is_none()
        {
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
    }

    async fn get_csrf(server: &axum_test::TestServer, path: &str) -> String {
        let resp = server.get(path).await;
        resp.cookie("csrf_token").value().to_string()
    }

    #[tokio::test]
    #[serial]
    async fn test_register_form_returns_200() {
        setup();
        request::<App, _, _>(|request, ctx| async move {
            seed_first_user(&ctx.db).await;
            let response = request.get("/register").await;
            assert_eq!(response.status_code(), 200);
        })
        .await;
    }

    #[tokio::test]
    #[serial]
    async fn test_register_user_success() {
        setup();
        request::<App, _, _>(|request, ctx| async move {
            seed_first_user(&ctx.db).await;
            let csrf = get_csrf(&request, "/register").await;
            let response = request
                .post("/register")
                .add_cookie(Cookie::new("csrf_token", &csrf))
                .form(&serde_json::json!({
                    "username": "testuser",
                    "email": "test@example.com",
                    "password": "password123",
                    "password_confirm": "password123",
                    "csrf_token": csrf,
                }))
                .await;
            assert!(response.status_code() == 302 || response.status_code() == 303);
            let loc_hdr = response.header("location");
            assert_eq!(loc_hdr.to_str().unwrap(), "/login");

            let user = users::Entity::find()
                .filter(users::Column::Username.eq("testuser"))
                .one(&ctx.db)
                .await
                .unwrap();
            assert!(user.is_some());
            let user = user.unwrap();
            assert_eq!(user.email.unwrap(), "test@example.com");
            assert!(user.is_local);
            assert!(!user.is_admin);
            assert!(user.password_hash.is_some());
            assert!(!user.public_key.is_empty());
            assert!(user.private_key.is_some());
            assert!(user.ap_id.contains("/u/testuser"));
        })
        .await;
    }

    #[tokio::test]
    #[serial]
    async fn test_register_duplicate_username() {
        setup();
        request::<App, _, _>(|request, ctx| async move {
            seed_first_user(&ctx.db).await;
            let base_url = "http://localhost:5150";
            UserModel::create_local(
                &ctx.db,
                "testuser",
                "first@example.com",
                "password123",
                base_url,
            )
            .await
            .unwrap();

            let csrf = get_csrf(&request, "/register").await;
            let response = request
                .post("/register")
                .add_cookie(Cookie::new("csrf_token", &csrf))
                .form(&serde_json::json!({
                    "username": "testuser",
                    "email": "other@example.com",
                    "password": "password123",
                    "password_confirm": "password123",
                    "csrf_token": csrf,
                }))
                .await;
            assert_eq!(response.status_code(), 200);
            let body = response.text();
            assert!(body.contains("Username is already taken"));
        })
        .await;
    }

    #[tokio::test]
    #[serial]
    async fn test_register_duplicate_email() {
        setup();
        request::<App, _, _>(|request, ctx| async move {
            seed_first_user(&ctx.db).await;
            let base_url = "http://localhost:5150";
            UserModel::create_local(
                &ctx.db,
                "firstuser",
                "dup@example.com",
                "password123",
                base_url,
            )
            .await
            .unwrap();

            let csrf = get_csrf(&request, "/register").await;
            let response = request
                .post("/register")
                .add_cookie(Cookie::new("csrf_token", &csrf))
                .form(&serde_json::json!({
                    "username": "seconduser",
                    "email": "dup@example.com",
                    "password": "password123",
                    "password_confirm": "password123",
                    "csrf_token": csrf,
                }))
                .await;
            assert_eq!(response.status_code(), 200);
            let body = response.text();
            assert!(body.contains("Email is already registered"));
        })
        .await;
    }

    #[tokio::test]
    #[serial]
    async fn test_register_password_mismatch() {
        setup();
        request::<App, _, _>(|request, ctx| async move {
            seed_first_user(&ctx.db).await;
            let csrf = get_csrf(&request, "/register").await;
            let response = request
                .post("/register")
                .add_cookie(Cookie::new("csrf_token", &csrf))
                .form(&serde_json::json!({
                    "username": "testuser",
                    "email": "test@example.com",
                    "password": "password123",
                    "password_confirm": "different",
                    "csrf_token": csrf,
                }))
                .await;
            assert_eq!(response.status_code(), 200);
            let body = response.text();
            assert!(body.contains("Passwords do not match"));
        })
        .await;
    }

    #[tokio::test]
    #[serial]
    async fn test_register_password_too_short() {
        setup();
        request::<App, _, _>(|request, ctx| async move {
            seed_first_user(&ctx.db).await;
            let csrf = get_csrf(&request, "/register").await;
            let response = request
                .post("/register")
                .add_cookie(Cookie::new("csrf_token", &csrf))
                .form(&serde_json::json!({
                    "username": "testuser",
                    "email": "test@example.com",
                    "password": "short",
                    "password_confirm": "short",
                    "csrf_token": csrf,
                }))
                .await;
            assert_eq!(response.status_code(), 200);
            let body = response.text();
            assert!(body.contains("Password must be at least 8 characters"));
        })
        .await;
    }

    #[tokio::test]
    #[serial]
    async fn test_login_form_returns_200() {
        setup();
        request::<App, _, _>(|request, ctx| async move {
            seed_first_user(&ctx.db).await;
            let response = request.get("/login").await;
            assert_eq!(response.status_code(), 200);
        })
        .await;
    }

    #[tokio::test]
    #[serial]
    async fn test_login_success() {
        setup();
        request::<App, _, _>(|request, ctx| async move {
            seed_first_user(&ctx.db).await;
            let base_url = "http://localhost:5150";
            UserModel::create_local(
                &ctx.db,
                "testuser",
                "test@example.com",
                "password123",
                base_url,
            )
            .await
            .unwrap();

            let csrf = get_csrf(&request, "/login").await;
            let response = request
                .post("/login")
                .add_cookie(Cookie::new("csrf_token", &csrf))
                .form(&serde_json::json!({
                    "identifier": "test@example.com",
                    "password": "password123",
                    "csrf_token": csrf,
                }))
                .await;
            assert!(response.status_code() == 302 || response.status_code() == 303);
            let loc_hdr = response.header("location");
            assert_eq!(loc_hdr.to_str().unwrap(), "/");
            let cookies = response.cookies();
            let access_cookie = cookies.iter().find(|c| c.name() == "access_token");
            assert!(access_cookie.is_some());
            let refresh_cookie = cookies.iter().find(|c| c.name() == "refresh_token");
            assert!(refresh_cookie.is_some());
        })
        .await;
    }

    #[tokio::test]
    #[serial]
    async fn test_login_invalid_password() {
        setup();
        request::<App, _, _>(|request, ctx| async move {
            seed_first_user(&ctx.db).await;
            let base_url = "http://localhost:5150";
            UserModel::create_local(
                &ctx.db,
                "testuser",
                "test@example.com",
                "password123",
                base_url,
            )
            .await
            .unwrap();

            let csrf = get_csrf(&request, "/login").await;
            let response = request
                .post("/login")
                .add_cookie(Cookie::new("csrf_token", &csrf))
                .form(&serde_json::json!({
                    "identifier": "test@example.com",
                    "password": "wrongpassword",
                    "csrf_token": csrf,
                }))
                .await;
            assert_eq!(response.status_code(), 200);
            let body = response.text();
            assert!(body.contains("Invalid credentials"));
        })
        .await;
    }

    #[tokio::test]
    #[serial]
    async fn test_login_nonexistent_user() {
        setup();
        request::<App, _, _>(|request, ctx| async move {
            seed_first_user(&ctx.db).await;
            let csrf = get_csrf(&request, "/login").await;
            let response = request
                .post("/login")
                .add_cookie(Cookie::new("csrf_token", &csrf))
                .form(&serde_json::json!({
                    "identifier": "nobody@example.com",
                    "password": "password123",
                    "csrf_token": csrf,
                }))
                .await;
            assert_eq!(response.status_code(), 200);
            let body = response.text();
            assert!(body.contains("Invalid credentials"));
        })
        .await;
    }

    #[tokio::test]
    #[serial]
    async fn test_login_with_username() {
        setup();
        request::<App, _, _>(|request, ctx| async move {
            seed_first_user(&ctx.db).await;
            let base_url = "http://localhost:5150";
            UserModel::create_local(
                &ctx.db,
                "testuser",
                "test@example.com",
                "password123",
                base_url,
            )
            .await
            .unwrap();

            let csrf = get_csrf(&request, "/login").await;
            let response = request
                .post("/login")
                .add_cookie(Cookie::new("csrf_token", &csrf))
                .form(&serde_json::json!({
                    "identifier": "testuser",
                    "password": "password123",
                    "csrf_token": csrf,
                }))
                .await;
            assert!(response.status_code() == 302 || response.status_code() == 303);
            let loc_hdr = response.header("location");
            assert_eq!(loc_hdr.to_str().unwrap(), "/");
            let cookies = response.cookies();
            let access_cookie = cookies.iter().find(|c| c.name() == "access_token");
            assert!(access_cookie.is_some());
            let refresh_cookie = cookies.iter().find(|c| c.name() == "refresh_token");
            assert!(refresh_cookie.is_some());
        })
        .await;
    }

    #[tokio::test]
    #[serial]
    async fn test_login_with_email_still_works() {
        setup();
        request::<App, _, _>(|request, ctx| async move {
            seed_first_user(&ctx.db).await;
            let base_url = "http://localhost:5150";
            UserModel::create_local(
                &ctx.db,
                "emailuser",
                "email@example.com",
                "password123",
                base_url,
            )
            .await
            .unwrap();

            let csrf = get_csrf(&request, "/login").await;
            let response = request
                .post("/login")
                .add_cookie(Cookie::new("csrf_token", &csrf))
                .form(&serde_json::json!({
                    "identifier": "email@example.com",
                    "password": "password123",
                    "csrf_token": csrf,
                }))
                .await;
            assert!(response.status_code() == 302 || response.status_code() == 303);
            let loc_hdr = response.header("location");
            assert_eq!(loc_hdr.to_str().unwrap(), "/");
            let cookies = response.cookies();
            let access_cookie = cookies.iter().find(|c| c.name() == "access_token");
            assert!(access_cookie.is_some());
        })
        .await;
    }

    #[tokio::test]
    #[serial]
    async fn test_logout_clears_cookies() {
        setup();
        request::<App, _, _>(|request, ctx| async move {
            seed_first_user(&ctx.db).await;
            let base_url = "http://localhost:5150";
            UserModel::create_local(
                &ctx.db,
                "testuser",
                "test@example.com",
                "password123",
                base_url,
            )
            .await
            .unwrap();

            let csrf = get_csrf(&request, "/login").await;
            let login_response = request
                .post("/login")
                .add_cookie(Cookie::new("csrf_token", &csrf))
                .form(&serde_json::json!({
                    "identifier": "test@example.com",
                    "password": "password123",
                    "csrf_token": csrf,
                }))
                .await;
            assert!(login_response.status_code() == 302 || login_response.status_code() == 303);

            let logout_response = request.post("/logout").await;
            assert!(logout_response.status_code() == 302 || logout_response.status_code() == 303);
            let loc_hdr = logout_response.header("location");
            assert_eq!(loc_hdr.to_str().unwrap(), "/login");
            let cookies = logout_response.cookies();
            let access_cookie = cookies.iter().find(|c| c.name() == "access_token");
            assert!(access_cookie.is_some());
            assert_eq!(access_cookie.unwrap().value(), "");
        })
        .await;
    }

    #[tokio::test]
    #[serial]
    async fn test_create_local_user_model() {
        setup();
        let boot = boot_test::<App>().await.unwrap();
        let db = &boot.app_context.db;

        let user = UserModel::create_local(
            db,
            "modeluser",
            "model@example.com",
            "securepass123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        assert_eq!(user.username, "modeluser");
        assert_eq!(user.email.unwrap(), "model@example.com");
        assert!(user.is_local);
        assert!(!user.is_admin);
        assert!(user.password_hash.is_some());
        assert!(!user.public_key.is_empty());
        assert!(user.private_key.is_some());
        assert!(user.ap_id.contains("/u/modeluser"));
        assert!(user.inbox_url.contains("/u/modeluser/inbox"));
    }

    #[tokio::test]
    #[serial]
    async fn test_verify_password() {
        setup();
        let boot = boot_test::<App>().await.unwrap();
        let db = &boot.app_context.db;

        let user = UserModel::create_local(
            db,
            "passuser",
            "pass@example.com",
            "correct-horse-battery-staple",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        assert!(UserModel::verify_password(&user, "correct-horse-battery-staple").await);
        assert!(!UserModel::verify_password(&user, "wrong-password").await);
    }

    #[tokio::test]
    #[serial]
    async fn test_username_validation() {
        setup();
        let boot = boot_test::<App>().await.unwrap();
        let db = &boot.app_context.db;

        let result = UserModel::create_local(
            db,
            "ab",
            "short@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await;
        assert!(result.is_err());

        let result = UserModel::create_local(
            db,
            "invalid username!",
            "invalid@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    #[serial]
    async fn test_jwt_token_generation() {
        setup();
        use taverna::common::jwt::{create_access_token, create_refresh_token, verify_token};

        let boot = boot_test::<App>().await.unwrap();
        let db = &boot.app_context.db;

        let user = UserModel::create_local(
            db,
            "jwtuser",
            "jwt@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let secret = "test-jwt-secret";
        let access_token = create_access_token(&user, secret).unwrap();
        let refresh_token = create_refresh_token(&user, secret).unwrap();

        let access_claims = verify_token(&access_token, secret).unwrap();
        assert_eq!(access_claims.sub, user.id.to_string());
        assert_eq!(access_claims.is_admin, user.is_admin);

        let refresh_claims = verify_token(&refresh_token, secret).unwrap();
        assert_eq!(refresh_claims.sub, user.id.to_string());
    }
}
