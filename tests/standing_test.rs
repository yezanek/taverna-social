#[cfg(test)]
mod tests {
    use loco_rs::testing::prelude::*;
    use sea_orm::EntityTrait;
    use serial_test::serial;
    use taverna::app::App;
    use taverna::models::_entities::users;
    use taverna::models::posts::PostModel;
    use taverna::models::users::UserModel;
    use taverna::models::votes::VoteModel;

    fn setup() {
        dotenvy::dotenv().ok();
    }

    #[tokio::test]
    #[serial]
    async fn test_upvote_increases_standing() {
        setup();
        let boot = boot_test::<App>().await.unwrap();
        let db = &boot.app_context.db;

        let voter = UserModel::create_local(
            db,
            "voter_up",
            "voter_up@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();
        let author = UserModel::create_local(
            db,
            "author_up",
            "author_up@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let post = PostModel::create(
            db,
            None,
            author.id,
            "Standing upvote test",
            Some("Body"),
            None,
            "http://localhost:5150",
        )
        .await
        .unwrap();

        VoteModel::cast_vote(db, voter.id, Some(post.id), None, 1)
            .await
            .unwrap();

        let updated = users::Entity::find_by_id(author.id)
            .one(db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.standing_score, 1);
    }

    #[tokio::test]
    #[serial]
    async fn test_downvote_decreases_standing() {
        setup();
        let boot = boot_test::<App>().await.unwrap();
        let db = &boot.app_context.db;

        let voter = UserModel::create_local(
            db,
            "voter_down",
            "voter_down@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();
        let author = UserModel::create_local(
            db,
            "author_down",
            "author_down@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let post = PostModel::create(
            db,
            None,
            author.id,
            "Standing downvote test",
            Some("Body"),
            None,
            "http://localhost:5150",
        )
        .await
        .unwrap();

        VoteModel::cast_vote(db, voter.id, Some(post.id), None, -1)
            .await
            .unwrap();

        let updated = users::Entity::find_by_id(author.id)
            .one(db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.standing_score, -1);
    }

    #[tokio::test]
    #[serial]
    async fn test_vote_removal_reverses_delta() {
        setup();
        let boot = boot_test::<App>().await.unwrap();
        let db = &boot.app_context.db;

        let voter = UserModel::create_local(
            db,
            "voter_rem",
            "voter_rem@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();
        let author = UserModel::create_local(
            db,
            "author_rem",
            "author_rem@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let post = PostModel::create(
            db,
            None,
            author.id,
            "Standing removal test",
            Some("Body"),
            None,
            "http://localhost:5150",
        )
        .await
        .unwrap();

        VoteModel::cast_vote(db, voter.id, Some(post.id), None, 1)
            .await
            .unwrap();

        let updated = users::Entity::find_by_id(author.id)
            .one(db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.standing_score, 1);

        VoteModel::cast_vote(db, voter.id, Some(post.id), None, 1)
            .await
            .unwrap();

        let updated = users::Entity::find_by_id(author.id)
            .one(db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.standing_score, 0);
    }

    #[tokio::test]
    #[serial]
    async fn test_moderation_removal_deducts_standing() {
        setup();
        let boot = boot_test::<App>().await.unwrap();
        let db = &boot.app_context.db;

        let user = UserModel::create_local(
            db,
            "author_modded",
            "author_modded@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let community = taverna::models::communities::CommunityModel::create(
            db,
            "standing-mod-test",
            "Standing Mod Test",
            None,
            user.id,
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let post = PostModel::create(
            db,
            Some(community.id),
            user.id,
            "Mod standing test",
            Some("Body"),
            None,
            "http://localhost:5150",
        )
        .await
        .unwrap();

        assert_eq!(user.standing_score, 0);

        PostModel::soft_remove(db, post.id).await.unwrap();

        let updated = users::Entity::find_by_id(user.id)
            .one(db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.standing_score, -5);
    }
}
