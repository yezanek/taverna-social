mod common;

#[cfg(test)]
mod tests {
    use axum_extra::extract::cookie::Cookie;
    use sea_orm::{ActiveModelTrait, EntityTrait, Set};
    use serial_test::serial;
    use taverna::models::_entities::users;
    use taverna::models::communities::CommunityModel;
    use taverna::models::users::UserModel;

    async fn seed_admin_user(db: &sea_orm::DatabaseConnection) {
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
        let user = UserModel::find_by_username(db, "admin")
            .await
            .unwrap()
            .unwrap();
        let mut active: users::ActiveModel = users::Entity::find_by_id(user.id)
            .one(db)
            .await
            .unwrap()
            .unwrap()
            .into();
        active.is_admin = Set(true);
        active.update(db).await.unwrap();
    }

    async fn get_csrf(server: &axum_test::TestServer, path: &str) -> String {
        let resp = server.get(path).await;
        resp.cookie("csrf_token").value().to_string()
    }

    async fn login(
        server: &axum_test::TestServer,
        identifier: &str,
        password: &str,
    ) -> Vec<Cookie<'static>> {
        let csrf = get_csrf(server, "/login").await;
        let resp = server
            .post("/login")
            .add_cookie(Cookie::new("csrf_token", csrf.clone()))
            .form(&serde_json::json!({
                "identifier": identifier,
                "password": password,
                "csrf_token": csrf,
            }))
            .save_cookies()
            .await;
        resp.cookies()
            .iter()
            .map(|c| Cookie::new(c.name().to_string(), c.value().to_string()))
            .collect()
    }

    #[tokio::test]
    #[serial]
    async fn test_admin_dashboard_shows_communities() {
        let (request, ctx) = crate::common::test_server_and_ctx().await;
        seed_admin_user(&ctx.db).await;
        let base_url = "http://localhost:5150";
        let admin = UserModel::find_by_username(&ctx.db, "admin")
            .await
            .unwrap()
            .unwrap();
        CommunityModel::create(
            &ctx.db,
            "admin-community",
            "Admin Community",
            Some("Managed by admin"),
            admin.id,
            base_url,
        )
        .await
        .unwrap();
        let cookies = login(&request, "admin", "password123").await;
        let mut get_req = request.get("/admin");
        for c in &cookies {
            get_req = get_req.add_cookie(c.clone());
        }
        let response = get_req.await;
        assert_eq!(response.status_code(), 200);
        let body = response.text();
        assert!(
            body.contains("admin-community"),
            "admin dashboard should list the community"
        );
        assert!(
            body.contains("member_count") || body.contains("Members"),
            "admin dashboard should show member count column"
        );
        assert!(
            body.contains("post_count") || body.contains("Posts"),
            "admin dashboard should show post count column"
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_admin_link_visible_for_admin() {
        let (request, ctx) = crate::common::test_server_and_ctx().await;
        seed_admin_user(&ctx.db).await;
        let _ = login(&request, "admin", "password123").await;
        let response = request.get("/").await;
        assert_eq!(response.status_code(), 200);
        let body = response.text();
        assert!(
            body.contains(r#"href="/admin""#),
            "admin user should see the admin nav link"
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_admin_link_hidden_for_regular_user() {
        let (request, ctx) = crate::common::test_server_and_ctx().await;
        seed_admin_user(&ctx.db).await;
        let base_url = "http://localhost:5150";
        UserModel::create_local(
            &ctx.db,
            "regularuser",
            "regular@example.com",
            "password123",
            base_url,
        )
        .await
        .unwrap();
        let _ = login(&request, "regularuser", "password123").await;
        let response = request.get("/").await;
        assert_eq!(response.status_code(), 200);
        let body = response.text();
        assert!(
            !body.contains(r#"href="/admin""#),
            "regular user should not see the admin nav link"
        );
    }
}
