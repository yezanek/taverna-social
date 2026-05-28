#[cfg(test)]
mod tests {
    use loco_rs::testing::prelude::*;
    use serial_test::serial;
    use taverna::app::App;
    use taverna::models::users::UserModel;

    fn setup() {
        dotenvy::dotenv().ok();
    }

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
        setup();
        request::<App, _, _>(|request, ctx| async move {
            seed_first_user(&ctx.db).await;
            let response = request.get("/setup").await;
            assert_eq!(response.status_code(), 403);
        })
        .await;
    }

    #[tokio::test]
    #[serial]
    async fn test_setup_disabled_after_first_user_created() {
        setup();
        request::<App, _, _>(|request, ctx| async move {
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
        })
        .await;
    }
}
