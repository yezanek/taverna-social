mod common;

#[cfg(test)]
mod tests {
    use serial_test::serial;
    use taverna::models::comments::{CommentError, CommentModel};
    use taverna::models::communities::CommunityModel;
    use taverna::models::community_bans::Entity as CommunityBans;
    use taverna::models::posts::{PostError, PostModel};
    use taverna::models::users::UserModel;

    #[tokio::test]
    #[serial]
    async fn test_moderator_can_remove_post() {
        let boot = crate::common::shared_boot().await;
        let db = &boot.app_context.db;

        let mod_user = UserModel::create_local(
            db,
            "mod_can_remove",
            "mod_can_remove@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let community = CommunityModel::create(
            db,
            "mod-remove-test",
            "Mod Remove Test",
            None,
            mod_user.id,
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let post = PostModel::create(
            db,
            Some(community.id),
            mod_user.id,
            "Test post",
            Some("Body"),
            None,
            "http://localhost:5150",
        )
        .await
        .unwrap();

        PostModel::soft_remove(db, post.id).await.unwrap();

        let updated = PostModel::find_by_id(db, post.id).await.unwrap().unwrap();
        assert!(updated.is_removed);
    }

    #[tokio::test]
    #[serial]
    async fn test_banned_user_cannot_post() {
        let boot = crate::common::shared_boot().await;
        let db = &boot.app_context.db;

        let admin = UserModel::create_local(
            db,
            "ban_test_admin",
            "ban_admin@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let banned_user = UserModel::create_local(
            db,
            "banned_user",
            "banned@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let community = CommunityModel::create(
            db,
            "ban-test-comm",
            "Ban Test",
            None,
            admin.id,
            "http://localhost:5150",
        )
        .await
        .unwrap();

        CommunityBans::create_ban(db, community.id, banned_user.id, admin.id, Some("Spam"))
            .await
            .unwrap();

        let is_banned = CommunityBans::is_banned(db, community.id, banned_user.id)
            .await
            .unwrap();
        assert!(is_banned);

        let err = PostModel::create(
            db,
            Some(community.id),
            banned_user.id,
            "Banned post",
            Some("Body"),
            None,
            "http://localhost:5150",
        )
        .await
        .unwrap_err();
        assert!(matches!(err, PostError::Banned));
    }

    #[tokio::test]
    #[serial]
    async fn test_locked_post_rejects_comments() {
        let boot = crate::common::shared_boot().await;
        let db = &boot.app_context.db;

        let user = UserModel::create_local(
            db,
            "lock_test_user",
            "lock_test@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let community = CommunityModel::create(
            db,
            "lock-test-comm",
            "Lock Test",
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
            "Locked post",
            Some("Body"),
            None,
            "http://localhost:5150",
        )
        .await
        .unwrap();

        PostModel::toggle_lock(db, post.id).await.unwrap();

        let locked_post = PostModel::find_by_id(db, post.id).await.unwrap().unwrap();
        assert!(locked_post.is_locked);

        let err = CommentModel::create(
            db,
            post.id,
            user.id,
            None,
            "This should fail",
            "http://localhost:5150",
        )
        .await
        .unwrap_err();
        assert!(matches!(err, CommentError::PostLocked));
    }

    #[tokio::test]
    #[serial]
    async fn test_admin_can_ban_user() {
        let boot = crate::common::shared_boot().await;
        let db = &boot.app_context.db;

        let target = UserModel::create_local(
            db,
            "target_ban",
            "target_ban@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        UserModel::set_banned(db, target.id, true).await.unwrap();

        let user = UserModel::find_by_id(db, target.id).await.unwrap().unwrap();
        assert!(user.is_banned);

        UserModel::set_banned(db, target.id, false).await.unwrap();

        let user = UserModel::find_by_id(db, target.id).await.unwrap().unwrap();
        assert!(!user.is_banned);
    }
}
