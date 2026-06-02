mod common;

#[cfg(test)]
mod tests {
    use axum_extra::extract::cookie::Cookie;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    use serial_test::serial;
    use taverna::models::_entities::{community_members, votes};
    use taverna::models::comments::CommentModel;
    use taverna::models::communities::CommunityModel;
    use taverna::models::posts::PostModel;
    use taverna::models::users::UserModel;

    async fn get_csrf(server: &axum_test::TestServer, path: &str) -> String {
        let resp = server.get(path).await;
        resp.cookie("csrf_token").value().to_string()
    }

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
    async fn test_full_happy_path() {
        let (request, ctx) = crate::common::test_server_and_ctx().await;
        seed_first_user(&ctx.db).await;

        // Register
        let csrf = get_csrf(&request, "/register").await;
        let resp = request
            .post("/register")
            .add_cookie(Cookie::new("csrf_token", &csrf))
            .form(&serde_json::json!({
                "username": "happyuser",
                "email": "happy@example.com",
                "password": "password123",
                "password_confirm": "password123",
                "csrf_token": csrf,
            }))
            .await;
        assert!(resp.status_code() == 302 || resp.status_code() == 303);
        assert_eq!(resp.header("location").to_str().unwrap(), "/login");

        // Login
        let csrf = get_csrf(&request, "/login").await;
        let resp = request
            .post("/login")
            .add_cookie(Cookie::new("csrf_token", &csrf))
            .form(&serde_json::json!({
                "identifier": "happy@example.com",
                "password": "password123",
                "csrf_token": csrf,
            }))
            .await;
        assert!(resp.status_code() == 302 || resp.status_code() == 303);

        // Create community (via model directly since we need the user reference)
        let user = UserModel::find_by_username(&ctx.db, "happyuser")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(user.email.unwrap(), "happy@example.com");

        let community = CommunityModel::create(
            &ctx.db,
            "happy-community",
            "Happy Community",
            Some("A place for happy people"),
            user.id,
            "http://localhost:5150",
        )
        .await
        .unwrap();
        assert_eq!(community.slug, "happy-community");
        assert_eq!(community.name, "Happy Community");

        // Verify creator is moderator
        let role = community_members::Entity::find()
            .filter(community_members::Column::CommunityId.eq(community.id))
            .filter(community_members::Column::UserId.eq(user.id))
            .one(&ctx.db)
            .await
            .unwrap();
        assert!(role.is_some());
        assert_eq!(role.unwrap().role, "moderator");

        // Create post (via model directly)
        let post = PostModel::create(
            &ctx.db,
            Some(community.id),
            user.id,
            "A test post",
            Some("This is the body of the test post"),
            None,
            "http://localhost:5150",
        )
        .await
        .unwrap();
        assert_eq!(post.title, "A test post");
        assert!(!post.is_deleted);

        // Vote on post (via model directly — upsert logic)
        let _vote =
            taverna::models::votes::VoteModel::cast_vote(&ctx.db, user.id, Some(post.id), None, 1)
                .await
                .unwrap();
        let post_votes = votes::Entity::find()
            .filter(votes::Column::PostId.eq(post.id))
            .all(&ctx.db)
            .await
            .unwrap();
        assert_eq!(post_votes.len(), 1);
        assert_eq!(post_votes[0].value, 1);

        // Verify score recalculated
        let post = PostModel::find_by_id(&ctx.db, post.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(post.score, 1);

        // Create comment
        let comment = CommentModel::create(
            &ctx.db,
            post.id,
            user.id,
            None,
            "This is a test comment",
            "http://localhost:5150",
        )
        .await
        .unwrap();
        assert_eq!(comment.body, "This is a test comment");
        assert!(comment.path.contains("c"));

        let all_comments = CommentModel::list_for_post(&ctx.db, post.id).await.unwrap();
        assert_eq!(all_comments.len(), 1);

        // Verify comment count on post incremented
        let post = PostModel::find_by_id(&ctx.db, post.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(post.comment_count, 1);
    }
}
