mod common;

#[cfg(test)]
mod tests {
    use serial_test::serial;
    use taverna::models::users::UserModel;

    /// Seed a default admin user so is_first_run returns false.
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

    #[tokio::test]
    #[serial]
    async fn test_setup_wizard_redirects_when_not_first_run() {
        let (request, ctx) = crate::common::test_server_and_ctx().await;
        seed_first_user(&ctx.db).await;
        let response = request.get("/setup").await;
        assert_eq!(response.status_code(), 403);
    }

    #[tokio::test]
    #[serial]
    async fn test_setup_disabled_after_first_user_created() {
        let (request, ctx) = crate::common::test_server_and_ctx().await;
        seed_first_user(&ctx.db).await;
        let response = request
            .post("/setup")
            .form(&serde_json::json!({
                "instance_name": "Test Instance",
                "username": "admin",
                "email": "admin@test.com",
                "password": "password123",
                "password_confirm": "password123",
            }))
            .await;
        assert_eq!(response.status_code(), 403);
    }

    #[tokio::test]
    #[serial]
    async fn test_setup_page_uses_base_layout() {
        let (request, ctx) = crate::common::test_server_and_ctx().await;
        let is_first = taverna::initializers::first_run::is_first_run(&ctx.db)
            .await
            .unwrap();
        if !is_first {
            return;
        }
        let response = request.get("/setup").await;
        assert_eq!(response.status_code(), 200);
        let body = response.text();
        assert!(
            body.contains("data-theme=\""),
            "response should contain data-theme attribute"
        );
        assert!(
            body.contains("theme.css"),
            "response should contain theme.css"
        );
        assert!(
            !body.contains(">Communities<"),
            "setup page should not show nav links"
        );
    }
}
