#[cfg(test)]
mod tests {
    use loco_rs::testing::prelude::*;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    use serial_test::serial;
    use taverna::app::App;
    use taverna::models::_entities::follows;
    use taverna::models::follows::Column;
    use taverna::models::users::UserModel;

    fn setup() {
        dotenvy::dotenv().ok();
    }

    #[tokio::test]
    #[serial]
    async fn test_follow_local_user() {
        setup();
        let boot = boot_test::<App>().await.unwrap();
        let db = &boot.app_context.db;

        let follower = UserModel::create_local(
            db,
            "follower1",
            "follower1@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let following = UserModel::create_local(
            db,
            "following1",
            "following1@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        follows::Entity::create_local_follow(db, follower.id, following.id)
            .await
            .unwrap();

        let follow = follows::Entity::find()
            .filter(Column::FollowerId.eq(follower.id))
            .filter(Column::FollowingId.eq(following.id))
            .one(db)
            .await
            .unwrap();

        assert!(follow.is_some());
        let follow = follow.unwrap();
        assert_eq!(follow.follower_id, follower.id);
        assert_eq!(follow.following_id, Some(following.id));
    }

    #[tokio::test]
    #[serial]
    async fn test_follow_sets_accepted_immediately_for_local() {
        setup();
        let boot = boot_test::<App>().await.unwrap();
        let db = &boot.app_context.db;

        let follower = UserModel::create_local(
            db,
            "follower2",
            "follower2@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let following = UserModel::create_local(
            db,
            "following2",
            "following2@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        follows::Entity::create_local_follow(db, follower.id, following.id)
            .await
            .unwrap();

        let follow = follows::Entity::find()
            .filter(Column::FollowerId.eq(follower.id))
            .filter(Column::FollowingId.eq(following.id))
            .one(db)
            .await
            .unwrap();

        assert!(follow.is_some());
        assert!(follow.unwrap().accepted);
    }

    #[tokio::test]
    #[serial]
    async fn test_unfollow_removes_row() {
        setup();
        let boot = boot_test::<App>().await.unwrap();
        let db = &boot.app_context.db;

        let follower = UserModel::create_local(
            db,
            "follower3",
            "follower3@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let following = UserModel::create_local(
            db,
            "following3",
            "following3@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        follows::Entity::create_local_follow(db, follower.id, following.id)
            .await
            .unwrap();

        follows::Entity::delete_follow(db, follower.id, following.id)
            .await
            .unwrap();

        let follow = follows::Entity::find()
            .filter(Column::FollowerId.eq(follower.id))
            .filter(Column::FollowingId.eq(following.id))
            .one(db)
            .await
            .unwrap();

        assert!(follow.is_none());
    }

    #[tokio::test]
    #[serial]
    async fn test_settings_update_display_name() {
        setup();
        let boot = boot_test::<App>().await.unwrap();
        let db = &boot.app_context.db;

        let user = UserModel::create_local(
            db,
            "settingsuser",
            "settings@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let updated =
            UserModel::update_profile(db, user.id, Some("New Display Name"), Some("My bio"), None)
                .await
                .unwrap();

        assert_eq!(updated.display_name.unwrap(), "New Display Name");
        assert_eq!(updated.bio.unwrap(), "My bio");
    }

    #[tokio::test]
    #[serial]
    async fn test_password_change_requires_current_password() {
        setup();
        let boot = boot_test::<App>().await.unwrap();
        let db = &boot.app_context.db;

        let user = UserModel::create_local(
            db,
            "passuser",
            "pass@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let result =
            UserModel::change_password(db, user.id, "wrongpassword", "newpassword123").await;

        assert!(result.is_err());
    }
}
