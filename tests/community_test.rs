#[cfg(test)]
mod tests {
    use loco_rs::testing::prelude::*;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    use serial_test::serial;
    use taverna::app::App;
    use taverna::models::_entities::community_members;
    use taverna::models::communities::CommunityModel;
    use taverna::models::users::UserModel;

    fn setup() {
        dotenvy::dotenv().ok();
    }

    #[tokio::test]
    #[serial]
    async fn test_create_community_sets_ap_id() {
        setup();
        let boot = boot_test::<App>().await.unwrap();
        let db = &boot.app_context.db;

        let user = UserModel::create_local(
            db,
            "testuser",
            "test@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let community = CommunityModel::create(
            db,
            "test-community",
            "Test Community",
            None,
            user.id,
            "http://localhost:5150",
        )
        .await
        .unwrap();

        assert_eq!(community.ap_id, "http://localhost:5150/c/test-community");
        assert!(community.public_key.contains("-----BEGIN PUBLIC KEY-----"));
        assert!(community.private_key.is_some());
        assert!(community.is_local);
    }

    #[tokio::test]
    #[serial]
    async fn test_duplicate_slug_fails() {
        setup();
        let boot = boot_test::<App>().await.unwrap();
        let db = &boot.app_context.db;

        let user = UserModel::create_local(
            db,
            "testuser2",
            "test2@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        CommunityModel::create(
            db,
            "dup-slug",
            "First",
            None,
            user.id,
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let result = CommunityModel::create(
            db,
            "dup-slug",
            "Second",
            None,
            user.id,
            "http://localhost:5150",
        )
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    #[serial]
    async fn test_creator_is_moderator() {
        setup();
        let boot = boot_test::<App>().await.unwrap();
        let db = &boot.app_context.db;

        let user = UserModel::create_local(
            db,
            "moduser",
            "mod@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let community = CommunityModel::create(
            db,
            "mod-comm",
            "Mod Community",
            None,
            user.id,
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let role = community_members::Entity::find()
            .filter(community_members::Column::CommunityId.eq(community.id))
            .filter(community_members::Column::UserId.eq(user.id))
            .one(db)
            .await
            .unwrap();

        assert!(role.is_some());
        assert_eq!(role.unwrap().role, "moderator");
    }

    #[tokio::test]
    #[serial]
    async fn test_join_community() {
        setup();
        let boot = boot_test::<App>().await.unwrap();
        let db = &boot.app_context.db;

        let creator = UserModel::create_local(
            db,
            "owner",
            "owner@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();
        let joiner = UserModel::create_local(
            db,
            "joiner",
            "joiner@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let community = CommunityModel::create(
            db,
            "join-test",
            "Join Test",
            None,
            creator.id,
            "http://localhost:5150",
        )
        .await
        .unwrap();

        CommunityModel::add_member(db, community.id, joiner.id, "member")
            .await
            .unwrap();

        let role = CommunityModel::get_member_role(db, community.id, joiner.id)
            .await
            .unwrap();
        assert_eq!(role, Some("member".to_string()));
    }
}
