mod common;

#[cfg(test)]
mod tests {
    use serial_test::serial;
    use taverna::models::comments::CommentModel;
    use taverna::models::posts::PostModel;
    use taverna::models::users::UserModel;

    #[tokio::test]
    #[serial]
    async fn test_top_level_comment_path() {
        let boot = crate::common::shared_boot().await;
        let db = &boot.app_context.db;

        let user = UserModel::create_local(
            db,
            "cuser1",
            "cuser1@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();
        let post = PostModel::create(
            db,
            None,
            user.id,
            "Path test",
            Some("Body"),
            None,
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let comment = CommentModel::create(
            db,
            post.id,
            user.id,
            None,
            "Top-level comment",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        // Path format: c{post_8chars}.c{comment_8chars}
        let post_seg = format!("c{}", &post.id.to_string().replace('-', "")[..8]);
        let comment_seg = format!("c{}", &comment.id.to_string().replace('-', "")[..8]);
        let expected_path = format!("{}.{}", post_seg, comment_seg);

        assert_eq!(
            comment.path, expected_path,
            "Top-level comment path should be post_seg.comment_seg"
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_reply_path_extends_parent() {
        let boot = crate::common::shared_boot().await;
        let db = &boot.app_context.db;

        let user = UserModel::create_local(
            db,
            "cuser2",
            "cuser2@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();
        let post = PostModel::create(
            db,
            None,
            user.id,
            "Reply path",
            Some("Body"),
            None,
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let parent = CommentModel::create(
            db,
            post.id,
            user.id,
            None,
            "Parent",
            "http://localhost:5150",
        )
        .await
        .unwrap();
        let reply = CommentModel::create(
            db,
            post.id,
            user.id,
            Some(parent.id),
            "Reply to parent",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let reply_seg = format!("c{}", &reply.id.to_string().replace('-', "")[..8]);
        let expected_path = format!("{}.{}", parent.path, reply_seg);

        assert_eq!(
            reply.path, expected_path,
            "Reply path should be parent.path.reply_seg"
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_nested_reply_path() {
        let boot = crate::common::shared_boot().await;
        let db = &boot.app_context.db;

        let user = UserModel::create_local(
            db,
            "cuser3",
            "cuser3@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();
        let post = PostModel::create(
            db,
            None,
            user.id,
            "Nested path",
            Some("Body"),
            None,
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let c1 = CommentModel::create(
            db,
            post.id,
            user.id,
            None,
            "Level 1",
            "http://localhost:5150",
        )
        .await
        .unwrap();
        let c2 = CommentModel::create(
            db,
            post.id,
            user.id,
            Some(c1.id),
            "Level 2",
            "http://localhost:5150",
        )
        .await
        .unwrap();
        let c3 = CommentModel::create(
            db,
            post.id,
            user.id,
            Some(c2.id),
            "Level 3",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let seg3 = format!("c{}", &c3.id.to_string().replace('-', "")[..8]);
        let expected = format!("{}.{}", c2.path, seg3);

        assert_eq!(
            c3.path, expected,
            "Nested reply path should chain correctly"
        );

        // Verify path segment count (includes post segment)
        let segments = c3.path.matches('.').count() + 1;
        assert_eq!(
            segments, 4,
            "Path should have 4 segments for a 3-deep reply chain"
        );
    }
}
