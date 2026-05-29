mod common;

#[cfg(test)]
mod tests {
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
}
