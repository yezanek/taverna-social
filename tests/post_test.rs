mod common;

#[cfg(test)]
mod tests {
    use axum_extra::extract::cookie::Cookie;
    use serial_test::serial;
    use taverna::models::communities::CommunityModel;
    use taverna::models::posts::PostModel;
    use taverna::models::users::UserModel;
    use taverna::models::votes::VoteModel;

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
            .await;
        resp.cookies()
            .iter()
            .map(|c| Cookie::new(c.name().to_string(), c.value().to_string()))
            .collect()
    }

    #[tokio::test]
    #[serial]
    async fn test_create_text_post() {
        let boot = crate::common::shared_boot().await;
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
        let boot = crate::common::shared_boot().await;
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
        let boot = crate::common::shared_boot().await;
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
        let boot = crate::common::shared_boot().await;
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
        let boot = crate::common::shared_boot().await;
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
        let boot = crate::common::shared_boot().await;
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
        let boot = crate::common::shared_boot().await;
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

    #[tokio::test]
    #[serial]
    async fn test_new_post_has_type_selector() {
        let (request, ctx) = crate::common::test_server_and_ctx().await;
        seed_first_user(&ctx.db).await;
        let base_url = "http://localhost:5150";
        let user = UserModel::create_local(
            &ctx.db,
            "selectoruser",
            "selector@example.com",
            "password123",
            base_url,
        )
        .await
        .unwrap();
        let community = CommunityModel::create(
            &ctx.db,
            "selector-comm",
            "Selector Comm",
            None,
            user.id,
            base_url,
        )
        .await
        .unwrap();
        let cookies = login(&request, "selectoruser", "password123").await;
        let path = format!("/c/{}/new", community.slug);
        let mut get_req = request.get(&path);
        for c in &cookies {
            get_req = get_req.add_cookie(c.clone());
        }
        let response = get_req.await;
        assert_eq!(response.status_code(), 200);
        let body = response.text();
        assert!(
            body.contains("name=\"url\""),
            "new post form should contain url input"
        );
        assert!(
            body.contains("name=\"body\""),
            "new post form should contain body textarea"
        );
        assert!(
            body.contains("name=\"post_type\""),
            "new post form should contain post_type radio buttons"
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_post_title_is_a_link() {
        let (request, ctx) = crate::common::test_server_and_ctx().await;
        seed_first_user(&ctx.db).await;
        let base_url = "http://localhost:5150";
        let user = UserModel::create_local(
            &ctx.db,
            "linkuser2",
            "link2@example.com",
            "password123",
            base_url,
        )
        .await
        .unwrap();
        let community = CommunityModel::create(
            &ctx.db,
            "link-comm2",
            "Link Community 2",
            None,
            user.id,
            base_url,
        )
        .await
        .unwrap();
        let post = PostModel::create(
            &ctx.db,
            Some(community.id),
            user.id,
            "Clickable Title",
            Some("Body text"),
            None,
            base_url,
        )
        .await
        .unwrap();
        let feed_path = format!("/c/{}", community.slug);
        let response = request.get(&feed_path).await;
        assert_eq!(response.status_code(), 200);
        let body = response.text();
        let expected = format!("/c/{}/posts/{}", community.slug, post.id);
        assert!(
            body.contains(&expected),
            "feed should contain link to post detail: {}",
            expected
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_post_detail_page_accessible() {
        let (request, ctx) = crate::common::test_server_and_ctx().await;
        seed_first_user(&ctx.db).await;
        let base_url = "http://localhost:5150";
        let user = UserModel::create_local(
            &ctx.db,
            "detailuser",
            "detail@example.com",
            "password123",
            base_url,
        )
        .await
        .unwrap();
        let community = CommunityModel::create(
            &ctx.db,
            "detail-comm",
            "Detail Community",
            None,
            user.id,
            base_url,
        )
        .await
        .unwrap();
        let post = PostModel::create(
            &ctx.db,
            Some(community.id),
            user.id,
            "Detail Page Title",
            Some("Detail body"),
            None,
            base_url,
        )
        .await
        .unwrap();
        let detail_path = format!("/c/{}/posts/{}", community.slug, post.id);
        let response = request.get(&detail_path).await;
        assert_eq!(response.status_code(), 200);
        let body = response.text();
        assert!(
            body.contains("Detail Page Title"),
            "post detail page should contain the post title"
        );
    }

    // ── Phase 8: unified feed and keyset pagination ──────────

    async fn remote_post_in(
        db: &sea_orm::DatabaseConnection,
        community_id: uuid::Uuid,
        title: &str,
    ) {
        let author = "https://remote.example/u/bob";
        taverna::models::remote::RemoteActorModel::ensure(db, author)
            .await
            .unwrap();
        let note: taverna::federation::protocol::Note = serde_json::from_value(serde_json::json!({
            "type": "Note",
            "id": format!("https://remote.example/post/{}", uuid::Uuid::new_v4()),
            "attributedTo": author,
            "to": [], "cc": [],
            "name": title,
            "content": "from another instance",
            "published": chrono::Utc::now().to_rfc3339(),
        }))
        .unwrap();
        taverna::models::remote::RemotePostModel::upsert(
            db,
            &note,
            Some(community_id),
            serde_json::json!({}),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    #[serial]
    async fn test_community_feed_includes_remote_posts() {
        let (server, ctx) = crate::common::test_server_and_ctx().await;
        let db = &ctx.db;
        seed_first_user(db).await;
        let user = UserModel::find_by_username(db, "admin")
            .await
            .unwrap()
            .unwrap();
        let community =
            CommunityModel::create(db, "mixed", "Mixed", None, user.id, "http://localhost:5150")
                .await
                .unwrap();
        PostModel::create(
            db,
            Some(community.id),
            user.id,
            "Local headline",
            Some("b"),
            None,
            "http://localhost:5150",
        )
        .await
        .unwrap();
        remote_post_in(db, community.id, "Remote headline").await;

        let body = server.get("/c/mixed").await.text();
        assert!(body.contains("Local headline"), "local post missing");
        assert!(body.contains("Remote headline"), "remote post missing");
        assert!(body.contains("bob@remote.example"), "remote author missing");
    }

    async fn seed_many_posts(db: &sea_orm::DatabaseConnection, n: usize) -> uuid::Uuid {
        seed_first_user(db).await;
        let user = UserModel::find_by_username(db, "admin")
            .await
            .unwrap()
            .unwrap();
        let community =
            CommunityModel::create(db, "busy", "Busy", None, user.id, "http://localhost:5150")
                .await
                .unwrap();
        for i in 0..n {
            PostModel::create(
                db,
                Some(community.id),
                user.id,
                &format!("Busy post {i:02}"),
                Some("b"),
                None,
                "http://localhost:5150",
            )
            .await
            .unwrap();
        }
        community.id
    }

    #[tokio::test]
    #[serial]
    async fn test_keyset_pagination_first_page() {
        let (server, ctx) = crate::common::test_server_and_ctx().await;
        let community_id = seed_many_posts(&ctx.db, 30).await;

        let page = taverna::models::feed::community_feed(&ctx.db, community_id, None, 25)
            .await
            .unwrap();
        assert_eq!(page.items.len(), 25);
        assert!(page.next_cursor.is_some());

        let body = server.get("/c/busy").await.text();
        assert!(
            body.contains("?cursor="),
            "first page should link to the next page"
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_keyset_pagination_second_page() {
        let (server, ctx) = crate::common::test_server_and_ctx().await;
        let community_id = seed_many_posts(&ctx.db, 30).await;

        let first = taverna::models::feed::community_feed(&ctx.db, community_id, None, 25)
            .await
            .unwrap();
        let cursor_raw = first.next_cursor.clone().unwrap();
        let cursor = taverna::models::feed::Cursor::decode(&cursor_raw).unwrap();
        let second =
            taverna::models::feed::community_feed(&ctx.db, community_id, Some(&cursor), 25)
                .await
                .unwrap();
        assert_eq!(second.items.len(), 5);
        assert!(second.next_cursor.is_none());

        let first_ids: std::collections::HashSet<_> =
            first.items.iter().map(|p| p.id.clone()).collect();
        assert!(
            second.items.iter().all(|p| !first_ids.contains(&p.id)),
            "pages overlap"
        );

        // Over HTTP: page size is 20, so page two of 30 has the remaining 10.
        let page1 = server.get("/c/busy").await.text();
        let start = page1.find("?cursor=").unwrap() + "?cursor=".len();
        let end = page1[start..].find('"').unwrap() + start;
        let page2 = server
            .get(&format!("/c/busy?cursor={}", &page1[start..end]))
            .await
            .text();
        let count = |html: &str| html.matches("class=\"card post-card\"").count();
        assert_eq!(count(&page1), 20);
        assert_eq!(count(&page2), 10);
    }
}
