#[cfg(test)]
mod tests {
    use loco_rs::testing::prelude::*;
    use serial_test::serial;
    use taverna::app::App;
    use taverna::models::posts::PostModel;
    use taverna::models::users::UserModel;
    use taverna::models::votes::VoteModel;

    fn setup() {
        dotenvy::dotenv().ok();
    }

    #[tokio::test]
    #[serial]
    async fn test_create_text_post() {
        setup();
        let boot = boot_test::<App>().await.unwrap();
        let db = &boot.app_context.db;

        let user = UserModel::create_local(
            db,
            "postuser",
            "post@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let post = PostModel::create(
            db,
            None,
            user.id,
            "Test Post",
            Some("Hello world"),
            None,
            "http://localhost:5150",
        )
        .await
        .unwrap();

        assert_eq!(post.title, "Test Post");
        assert_eq!(post.body, Some("Hello world".to_string()));
        assert!(post.url.is_none());
        assert_eq!(post.score, 0);
        assert!(!post.is_deleted);
    }

    #[tokio::test]
    #[serial]
    async fn test_create_link_post() {
        setup();
        let boot = boot_test::<App>().await.unwrap();
        let db = &boot.app_context.db;

        let user = UserModel::create_local(
            db,
            "linkuser",
            "link@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let post = PostModel::create(
            db,
            None,
            user.id,
            "Link Post",
            None,
            Some("https://example.com"),
            "http://localhost:5150",
        )
        .await
        .unwrap();

        assert_eq!(post.title, "Link Post");
        assert!(post.body.is_none());
        assert_eq!(post.url, Some("https://example.com".to_string()));
    }

    #[tokio::test]
    #[serial]
    async fn test_post_requires_body_or_url() {
        setup();
        let boot = boot_test::<App>().await.unwrap();
        let db = &boot.app_context.db;

        let user = UserModel::create_local(
            db,
            "badpostuser",
            "badpost@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let result = PostModel::create(
            db,
            None,
            user.id,
            "No content",
            None,
            None,
            "http://localhost:5150",
        )
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    #[serial]
    async fn test_vote_up() {
        setup();
        let boot = boot_test::<App>().await.unwrap();
        let db = &boot.app_context.db;

        let user = UserModel::create_local(
            db,
            "voter1",
            "voter1@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();
        let author = UserModel::create_local(
            db,
            "author1",
            "author1@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let post = PostModel::create(
            db,
            None,
            author.id,
            "Vote test",
            Some("Vote me"),
            None,
            "http://localhost:5150",
        )
        .await
        .unwrap();

        VoteModel::cast_vote(db, user.id, Some(post.id), None, 1)
            .await
            .unwrap();

        let updated = PostModel::find_by_id(db, post.id).await.unwrap().unwrap();
        assert_eq!(updated.score, 1);
    }

    #[tokio::test]
    #[serial]
    async fn test_vote_flip() {
        setup();
        let boot = boot_test::<App>().await.unwrap();
        let db = &boot.app_context.db;

        let user = UserModel::create_local(
            db,
            "voter2",
            "voter2@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();
        let author = UserModel::create_local(
            db,
            "author2",
            "author2@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let post = PostModel::create(
            db,
            None,
            author.id,
            "Flip test",
            Some("Flip me"),
            None,
            "http://localhost:5150",
        )
        .await
        .unwrap();

        VoteModel::cast_vote(db, user.id, Some(post.id), None, 1)
            .await
            .unwrap();
        let updated = PostModel::find_by_id(db, post.id).await.unwrap().unwrap();
        assert_eq!(updated.score, 1);

        VoteModel::cast_vote(db, user.id, Some(post.id), None, -1)
            .await
            .unwrap();
        let updated = PostModel::find_by_id(db, post.id).await.unwrap().unwrap();
        assert_eq!(updated.score, -1);
    }

    #[tokio::test]
    #[serial]
    async fn test_vote_toggle_removes() {
        setup();
        let boot = boot_test::<App>().await.unwrap();
        let db = &boot.app_context.db;

        let user = UserModel::create_local(
            db,
            "voter3",
            "voter3@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();
        let author = UserModel::create_local(
            db,
            "author3",
            "author3@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let post = PostModel::create(
            db,
            None,
            author.id,
            "Toggle test",
            Some("Toggle me"),
            None,
            "http://localhost:5150",
        )
        .await
        .unwrap();

        VoteModel::cast_vote(db, user.id, Some(post.id), None, 1)
            .await
            .unwrap();
        let updated = PostModel::find_by_id(db, post.id).await.unwrap().unwrap();
        assert_eq!(updated.score, 1);

        VoteModel::cast_vote(db, user.id, Some(post.id), None, 1)
            .await
            .unwrap();
        let updated = PostModel::find_by_id(db, post.id).await.unwrap().unwrap();
        assert_eq!(updated.score, 0);
    }

    #[tokio::test]
    #[serial]
    async fn test_score_recalculates() {
        setup();
        let boot = boot_test::<App>().await.unwrap();
        let db = &boot.app_context.db;

        let user1 = UserModel::create_local(
            db,
            "voter4a",
            "voter4a@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();
        let user2 = UserModel::create_local(
            db,
            "voter4b",
            "voter4b@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();
        let author = UserModel::create_local(
            db,
            "author4",
            "author4@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let post = PostModel::create(
            db,
            None,
            author.id,
            "Recalc test",
            Some("Recalc me"),
            None,
            "http://localhost:5150",
        )
        .await
        .unwrap();

        VoteModel::cast_vote(db, user1.id, Some(post.id), None, 1)
            .await
            .unwrap();
        VoteModel::cast_vote(db, user2.id, Some(post.id), None, -1)
            .await
            .unwrap();

        let score = PostModel::recalculate_score(db, post.id).await.unwrap();
        assert_eq!(score, 0);

        VoteModel::cast_vote(db, user1.id, Some(post.id), None, -1)
            .await
            .unwrap();

        let score = PostModel::recalculate_score(db, post.id).await.unwrap();
        assert_eq!(score, -2);
    }
}
