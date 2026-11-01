mod common;

#[cfg(test)]
mod tests {
    use activitypub_federation::config::FederationConfig;
    use activitypub_federation::traits::Object;

    use serial_test::serial;
    use taverna::federation::protocol::Person;
    use taverna::federation::traits::DbUser;
    use taverna::models::_entities::communities;
    use taverna::models::_entities::users;
    use taverna::models::communities::CommunityModel;
    use taverna::models::posts::PostModel;
    use taverna::models::users::UserModel;
    use url::Url;

    fn setup() {
        dotenvy::dotenv().ok();
        unsafe {
            std::env::set_var("TAVERNA_DOMAIN", "localhost:5150");
        }
    }

    const BASE_URL: &str = "http://localhost:5150";
    const AP_JSON: &str = "application/activity+json";

    async fn create_test_user(db: &sea_orm::DatabaseConnection) -> users::Model {
        UserModel::create_local(db, "feduser", "fed@test.com", "password123", BASE_URL)
            .await
            .unwrap()
    }

    async fn create_test_community(
        db: &sea_orm::DatabaseConnection,
        user_id: uuid::Uuid,
    ) -> communities::Model {
        CommunityModel::create(
            db,
            "fed-community",
            "Fed Community",
            None,
            user_id,
            BASE_URL,
        )
        .await
        .unwrap()
    }

    async fn make_fed_data(
        db: &sea_orm::DatabaseConnection,
    ) -> activitypub_federation::config::Data<sea_orm::DatabaseConnection> {
        let fc = FederationConfig::builder()
            .domain("localhost:5150")
            .app_data(db.clone())
            .debug(true)
            .build()
            .await
            .unwrap();
        fc.to_request_data()
    }

    // ── WebFinger tests ──────────────────────────────────────

    #[tokio::test]
    #[serial]
    async fn test_webfinger_user() {
        setup();
        let (request, ctx) = crate::common::test_server_and_ctx().await;
        let _user = create_test_user(&ctx.db).await;

        let response = request
            .get("/.well-known/webfinger?resource=acct:feduser@localhost:5150")
            .await;
        assert_eq!(response.status_code(), 200);
        let ct = response
            .header("content-type")
            .to_str()
            .unwrap()
            .to_string();
        assert!(ct.contains("application/jrd+json") || ct.contains("application/json"));

        let body: serde_json::Value = response.json();
        assert_eq!(body["subject"], "acct:feduser@localhost:5150");
        let links = body["links"].as_array().unwrap();
        assert!(!links.is_empty());
    }

    #[tokio::test]
    #[serial]
    async fn test_webfinger_community() {
        setup();
        let (request, ctx) = crate::common::test_server_and_ctx().await;
        let user = create_test_user(&ctx.db).await;
        let _community = create_test_community(&ctx.db, user.id).await;

        let response = request
            .get("/.well-known/webfinger?resource=acct:fed-community@localhost:5150")
            .await;
        assert_eq!(response.status_code(), 200);

        let body: serde_json::Value = response.json();
        assert_eq!(body["subject"], "acct:fed-community@localhost:5150");
    }

    #[tokio::test]
    #[serial]
    async fn test_webfinger_unknown_returns_404() {
        setup();
        let (request, _ctx) = crate::common::test_server_and_ctx().await;
        let response = request
            .get("/.well-known/webfinger?resource=acct:nonexistent@localhost:5150")
            .await;
        assert_eq!(response.status_code(), 404);
    }

    // ── Actor JSON tests ─────────────────────────────────────

    #[tokio::test]
    #[serial]
    async fn test_user_actor_json_via_accept_header() {
        setup();
        let (request, ctx) = crate::common::test_server_and_ctx().await;
        let _user = create_test_user(&ctx.db).await;

        let response = request
            .get("/u/feduser")
            .add_header("Accept", AP_JSON)
            .await;
        assert_eq!(response.status_code(), 200);

        let ct = response
            .header("content-type")
            .to_str()
            .unwrap()
            .to_string();
        assert!(ct.contains("application/activity+json"), "{ct}");

        let body: serde_json::Value = response.json();
        assert_eq!(
            body["@context"],
            serde_json::json!(taverna::federation::protocol::AP_CONTEXT)
        );
        assert_eq!(body["type"], "Person");
        assert_eq!(body["preferredUsername"], "feduser");
        assert!(body["publicKey"].is_object());
        assert!(
            body["inbox"]
                .as_str()
                .unwrap_or("")
                .contains("/u/feduser/inbox")
        );
        assert!(
            body["followers"]
                .as_str()
                .unwrap_or("")
                .contains("/u/feduser/followers")
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_user_actor_html_redirect() {
        setup();
        let (request, ctx) = crate::common::test_server_and_ctx().await;
        let _user = create_test_user(&ctx.db).await;

        let response = request
            .get("/u/feduser")
            .add_header("Accept", "text/html")
            .await;
        let status = response.status_code();
        let body = response.text();
        if status != 200 {
            eprintln!("BODY: {}", body);
            panic!("Expected 200, got {}: {}", status, body);
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_community_feed_html_via_accept() {
        setup();
        let (request, ctx) = crate::common::test_server_and_ctx().await;
        let user = create_test_user(&ctx.db).await;
        let _community = create_test_community(&ctx.db, user.id).await;

        let response = request
            .get("/c/fed-community")
            .add_header("Accept", "text/html")
            .await;
        assert_eq!(response.status_code(), 200);
    }

    // ── Note endpoints ───────────────────────────────────────

    #[tokio::test]
    #[serial]
    async fn test_post_note_endpoint() {
        setup();
        let (request, ctx) = crate::common::test_server_and_ctx().await;
        let user = create_test_user(&ctx.db).await;
        let community = create_test_community(&ctx.db, user.id).await;

        let post = PostModel::create(
            &ctx.db,
            Some(community.id),
            user.id,
            "Test Post Title",
            Some("Test body content"),
            None,
            BASE_URL,
        )
        .await
        .unwrap();

        let response = request
            .get(&format!("/posts/{}", post.id))
            .add_header("Accept", AP_JSON)
            .await;
        assert_eq!(response.status_code(), 200);
        let body: serde_json::Value = response.json();
        assert_eq!(body["type"], "Note");
        assert_eq!(body["name"], "Test Post Title");
        assert_eq!(body["attributedTo"], user.ap_id);
    }

    #[tokio::test]
    #[serial]
    async fn test_comment_note_endpoint() {
        setup();
        let (request, ctx) = crate::common::test_server_and_ctx().await;
        let user = create_test_user(&ctx.db).await;
        let community = create_test_community(&ctx.db, user.id).await;

        let post = PostModel::create(
            &ctx.db,
            Some(community.id),
            user.id,
            "Post for comment",
            None,
            Some("https://example.com"),
            BASE_URL,
        )
        .await
        .unwrap();

        let comment = taverna::models::comments::CommentModel::create(
            &ctx.db,
            post.id,
            user.id,
            None,
            "A test comment",
            BASE_URL,
        )
        .await
        .unwrap();

        let url = format!("/comments/{}", comment.id);

        let response = request.get(&url).add_header("Accept", AP_JSON).await;
        let status = response.status_code();
        let body_text = response.text();
        if status != 200 {
            panic!(
                "Expected 200, got {}. URL: {}. Body: {}",
                status, url, body_text
            );
        }
        let body: serde_json::Value = serde_json::from_str(&body_text).unwrap();
        assert_eq!(body["type"], "Note");
        // `content` is HTML (ActivityPub); the markdown travels in `source`.
        assert_eq!(body["content"], "<p>A test comment</p>\n");
        assert_eq!(body["source"]["content"], "A test comment");
        assert_eq!(body["inReplyTo"], post.ap_id);
    }

    // ── NodeInfo tests ───────────────────────────────────────

    #[tokio::test]
    #[serial]
    async fn test_well_known_nodeinfo() {
        setup();
        let (request, _ctx) = crate::common::test_server_and_ctx().await;
        let response = request.get("/.well-known/nodeinfo").await;
        assert_eq!(response.status_code(), 200);

        let body: serde_json::Value = response.json();
        let links = body["links"].as_array().unwrap();
        assert!(!links.is_empty());
        assert_eq!(
            links[0]["rel"],
            "http://nodeinfo.diaspora.software/ns/schema/2.0"
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_nodeinfo_v2() {
        setup();
        let (request, _ctx) = crate::common::test_server_and_ctx().await;
        let response = request.get("/nodeinfo/2.0").await;
        assert_eq!(response.status_code(), 200);

        let body: serde_json::Value = response.json();
        assert_eq!(body["version"], "2.0");
        assert_eq!(body["software"]["name"], "taverna");
        assert_eq!(body["software"]["version"], env!("CARGO_PKG_VERSION"));
        assert!(
            body["protocols"]
                .as_array()
                .unwrap()
                .contains(&"activitypub".into())
        );
        assert!(body["openRegistrations"].as_bool().unwrap());
    }

    // ── Object trait tests ───────────────────────────────────

    #[tokio::test]
    #[serial]
    async fn test_dbuser_object_read_from_id() {
        setup();
        let boot = crate::common::shared_boot().await;
        let db = &boot.app_context.db;

        let user = create_test_user(db).await;
        let ap_id: Url = user.ap_id.parse().unwrap();
        let data = make_fed_data(db).await;

        let found = DbUser::read_from_id(ap_id, &data).await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().0.username, "feduser");
    }

    #[tokio::test]
    #[serial]
    async fn test_dbuser_into_json() {
        setup();
        let boot = crate::common::shared_boot().await;
        let db = &boot.app_context.db;

        let user = create_test_user(db).await;
        let db_user = DbUser::new(user).unwrap();
        let data = make_fed_data(db).await;

        let person: Person = db_user.into_json(&data).await.unwrap();
        assert_eq!(
            person.kind,
            taverna::federation::protocol::PersonKind::Person
        );
        assert_eq!(person.preferred_username.as_deref(), Some("feduser"));
    }

    #[tokio::test]
    #[serial]
    async fn test_dbuser_verify_domains_match() {
        setup();
        let boot = crate::common::shared_boot().await;
        let db = &boot.app_context.db;

        let user = create_test_user(db).await;
        let db_user = DbUser::new(user).unwrap();
        let data = make_fed_data(db).await;

        let person: Person = db_user.into_json(&data).await.unwrap();
        let expected_domain: Url = "http://localhost:5150".parse().unwrap();

        let result = DbUser::verify(&person, &expected_domain, &data).await;
        assert!(result.is_ok());

        let wrong_domain: Url = "http://evil.com".parse().unwrap();
        let result = DbUser::verify(&person, &wrong_domain, &data).await;
        assert!(result.is_err());
    }

    // ── Follower/collection endpoints ────────────────────────

    #[tokio::test]
    #[serial]
    async fn test_user_followers_endpoint() {
        setup();
        let (request, ctx) = crate::common::test_server_and_ctx().await;
        let _user = create_test_user(&ctx.db).await;

        let response = request
            .get("/u/feduser/followers")
            .add_header("Accept", AP_JSON)
            .await;
        assert_eq!(response.status_code(), 200);

        let body: serde_json::Value = response.json();
        assert_eq!(body["type"], "OrderedCollection");
    }

    #[tokio::test]
    #[serial]
    async fn test_user_outbox_endpoint() {
        setup();
        let (request, ctx) = crate::common::test_server_and_ctx().await;
        let _user = create_test_user(&ctx.db).await;

        let response = request
            .get("/u/feduser/outbox")
            .add_header("Accept", AP_JSON)
            .await;
        assert_eq!(response.status_code(), 200);

        let body: serde_json::Value = response.json();
        assert_eq!(body["type"], "OrderedCollection");
    }

    #[tokio::test]
    #[serial]
    async fn test_community_followers_endpoint() {
        setup();
        let (request, ctx) = crate::common::test_server_and_ctx().await;
        let user = create_test_user(&ctx.db).await;
        let _community = create_test_community(&ctx.db, user.id).await;

        let response = request
            .get("/c/fed-community/followers")
            .add_header("Accept", AP_JSON)
            .await;
        assert_eq!(response.status_code(), 200);

        let body: serde_json::Value = response.json();
        assert_eq!(body["type"], "OrderedCollection");
    }

    #[tokio::test]
    #[serial]
    async fn test_community_outbox_endpoint() {
        setup();
        let (request, ctx) = crate::common::test_server_and_ctx().await;
        let user = create_test_user(&ctx.db).await;
        let community = create_test_community(&ctx.db, user.id).await;
        PostModel::create(
            &ctx.db,
            Some(community.id),
            user.id,
            "Outbox post",
            Some("**bold**"),
            None,
            BASE_URL,
        )
        .await
        .unwrap();

        let response = request
            .get("/c/fed-community/outbox")
            .add_header("Accept", AP_JSON)
            .await;
        assert_eq!(response.status_code(), 200);

        let body: serde_json::Value = response.json();
        assert_eq!(body["type"], "OrderedCollection");
        assert_eq!(body["totalItems"], 1);
        let item = &body["orderedItems"][0];
        assert_eq!(item["type"], "Announce");
        assert_eq!(item["actor"], community.ap_id);
        // The announced Note stays attributed to its author and carries
        // rendered HTML, with the markdown in `source`.
        assert_eq!(item["object"]["attributedTo"], user.ap_id);
        assert!(
            item["object"]["content"]
                .as_str()
                .unwrap()
                .contains("<strong>bold</strong>")
        );
        assert_eq!(item["object"]["source"]["content"], "**bold**");
    }

    // ── Phase 8: remote content cache ────────────────────────

    const REMOTE_ALICE: &str = "https://remote.example/u/alice";

    async fn insert_remote_actor(
        db: &sea_orm::DatabaseConnection,
        ap_id: &str,
        shared_inbox: Option<&str>,
    ) -> taverna::models::_entities::remote_actors::Model {
        use sea_orm::{ActiveModelTrait, Set};
        let now: chrono::DateTime<chrono::FixedOffset> = chrono::Utc::now().into();
        taverna::models::_entities::remote_actors::ActiveModel {
            ap_id: Set(ap_id.to_string()),
            actor_type: Set("Person".to_string()),
            username: Set(ap_id.rsplit('/').next().unwrap_or("x").to_string()),
            display_name: Set(None),
            bio: Set(None),
            avatar_url: Set(None),
            public_key: Set(String::new()),
            inbox_url: Set(format!("{ap_id}/inbox")),
            shared_inbox_url: Set(shared_inbox.map(str::to_string)),
            followers_url: Set(format!("{ap_id}/followers")),
            instance_domain: Set(taverna::models::remote::instance_domain(ap_id)),
            raw_json: Set(serde_json::json!({})),
            fetched_at: Set(now),
            stale_after: Set((chrono::Utc::now() + chrono::Duration::days(1)).into()),
            created_at: Set(now),
        }
        .insert(db)
        .await
        .unwrap()
    }

    fn create_note_activity(note_id: &str, to: &str) -> serde_json::Value {
        serde_json::json!({
            "type": "Create",
            "id": format!("{note_id}/activity"),
            "actor": REMOTE_ALICE,
            "to": [to],
            "cc": [],
            "object": {
                "type": "Note",
                "id": note_id,
                "attributedTo": REMOTE_ALICE,
                "to": [to],
                "cc": [],
                "name": "Hello from afar",
                "content": "Remote body",
                "published": "2026-10-01T12:00:00Z"
            }
        })
    }

    async fn receive(db: &sea_orm::DatabaseConnection, json: serde_json::Value) {
        use activitypub_federation::traits::Activity;
        let activity: taverna::federation::protocol::TavernaActivity =
            serde_json::from_value(json).unwrap();
        let data = make_fed_data(db).await;
        activity.receive(&data).await.unwrap();
    }

    #[tokio::test]
    #[serial]
    async fn test_incoming_create_writes_to_remote_posts() {
        use sea_orm::{EntityTrait, PaginatorTrait};
        setup();
        let boot = crate::common::shared_boot().await;
        let db = &boot.app_context.db;
        let user = create_test_user(db).await;
        let community = create_test_community(db, user.id).await;

        let note_id = "https://remote.example/post/1";
        receive(db, create_note_activity(note_id, &community.ap_id)).await;

        let remote = taverna::models::remote::RemotePostModel::find(db, note_id)
            .await
            .unwrap()
            .expect("remote post stored");
        assert_eq!(remote.community_id, Some(community.id));
        assert_eq!(remote.author_ap_id, REMOTE_ALICE);
        assert_eq!(remote.title.as_deref(), Some("Hello from afar"));
        assert_eq!(
            taverna::models::_entities::posts::Entity::find()
                .count(db)
                .await
                .unwrap(),
            0,
            "remote notes must not be written to the local posts table"
        );
        assert_eq!(
            taverna::models::_entities::users::Entity::find()
                .count(db)
                .await
                .unwrap(),
            1,
            "remote authors must not be written to the local users table"
        );

        // Redelivery of the same activity is a no-op.
        receive(db, create_note_activity(note_id, &community.ap_id)).await;
        assert_eq!(
            taverna::models::_entities::remote_posts::Entity::find()
                .count(db)
                .await
                .unwrap(),
            1
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_incoming_reply_to_local_post_writes_to_remote_comments() {
        setup();
        let boot = crate::common::shared_boot().await;
        let db = &boot.app_context.db;
        let user = create_test_user(db).await;
        let community = create_test_community(db, user.id).await;
        let post = PostModel::create(
            db,
            Some(community.id),
            user.id,
            "Local post",
            Some("body"),
            None,
            BASE_URL,
        )
        .await
        .unwrap();

        let mut activity =
            create_note_activity("https://remote.example/comment/1", &community.ap_id);
        activity["object"]["inReplyTo"] = serde_json::json!(post.ap_id);
        receive(db, activity).await;

        let replies = taverna::models::remote::RemoteCommentModel::list_for_local_post(db, post.id)
            .await
            .unwrap();
        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0].0.body, "Remote body");
        let post = PostModel::find_by_id(db, post.id).await.unwrap().unwrap();
        assert_eq!(post.comment_count, 1);
    }

    #[tokio::test]
    #[serial]
    async fn test_remote_actor_upserted_on_receive() {
        use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
        setup();
        let boot = crate::common::shared_boot().await;
        let db = &boot.app_context.db;
        let user = create_test_user(db).await;

        assert!(
            taverna::models::remote::RemoteActorModel::find(db, REMOTE_ALICE)
                .await
                .unwrap()
                .is_none()
        );

        receive(
            db,
            serde_json::json!({
                "type": "Follow",
                "id": "https://remote.example/follows/1",
                "actor": REMOTE_ALICE,
                "object": user.ap_id,
            }),
        )
        .await;

        let actor = taverna::models::remote::RemoteActorModel::find(db, REMOTE_ALICE)
            .await
            .unwrap()
            .expect("remote actor created");
        assert_eq!(actor.instance_domain, "remote.example");

        let follow = taverna::models::_entities::follows::Entity::find()
            .filter(taverna::models::_entities::follows::Column::FollowingId.eq(user.id))
            .one(db)
            .await
            .unwrap()
            .expect("follow stored");
        assert_eq!(follow.follower_id, None);
        assert_eq!(follow.remote_follower_ap_id.as_deref(), Some(REMOTE_ALICE));
        assert!(follow.accepted);
    }

    #[tokio::test]
    #[serial]
    async fn test_update_from_other_actor_is_ignored() {
        setup();
        let boot = crate::common::shared_boot().await;
        let db = &boot.app_context.db;
        let user = create_test_user(db).await;
        let community = create_test_community(db, user.id).await;
        let note_id = "https://remote.example/post/2";
        receive(db, create_note_activity(note_id, &community.ap_id)).await;

        receive(
            db,
            serde_json::json!({
                "type": "Update",
                "id": "https://evil.example/updates/1",
                "actor": "https://evil.example/u/mallory",
                "to": [], "cc": [],
                "object": {
                    "type": "Note", "id": note_id,
                    "attributedTo": "https://evil.example/u/mallory",
                    "to": [], "cc": [], "content": "defaced"
                }
            }),
        )
        .await;

        let remote = taverna::models::remote::RemotePostModel::find(db, note_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(remote.body.as_deref(), Some("Remote body"));
    }

    #[tokio::test]
    #[serial]
    async fn test_expire_remote_cache_removes_stale() {
        use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, sea_query::Expr};
        setup();
        let boot = crate::common::shared_boot().await;
        let db = &boot.app_context.db;
        let user = create_test_user(db).await;
        let community = create_test_community(db, user.id).await;
        let local = PostModel::create(
            db,
            Some(community.id),
            user.id,
            "Local stays",
            Some("body"),
            None,
            BASE_URL,
        )
        .await
        .unwrap();

        receive(
            db,
            create_note_activity("https://remote.example/post/old", &community.ap_id),
        )
        .await;
        receive(
            db,
            create_note_activity("https://remote.example/post/new", &community.ap_id),
        )
        .await;

        let past: chrono::DateTime<chrono::FixedOffset> =
            (chrono::Utc::now() - chrono::Duration::days(1)).into();
        taverna::models::_entities::remote_posts::Entity::update_many()
            .filter(
                taverna::models::_entities::remote_posts::Column::ApId
                    .eq("https://remote.example/post/old"),
            )
            .col_expr(
                taverna::models::_entities::remote_posts::Column::StaleAfter,
                Expr::value(past),
            )
            .exec(db)
            .await
            .unwrap();

        let (_, posts_deleted, _, _) = taverna::models::remote::expire_remote_cache(db)
            .await
            .unwrap();
        assert_eq!(posts_deleted, 1);

        use taverna::models::remote::RemotePostModel;
        assert!(
            RemotePostModel::find(db, "https://remote.example/post/old")
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            RemotePostModel::find(db, "https://remote.example/post/new")
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            PostModel::find_by_id(db, local.id).await.unwrap().is_some(),
            "local posts are never expired"
        );
        // Alice still authors a cached post, so she is kept.
        assert!(
            taverna::models::remote::RemoteActorModel::find(db, REMOTE_ALICE)
                .await
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_shared_inbox_deduplication() {
        use sea_orm::{ActiveModelTrait, Set};
        setup();
        let boot = crate::common::shared_boot().await;
        let db = &boot.app_context.db;
        let user = create_test_user(db).await;
        let community = create_test_community(db, user.id).await;

        let followers = [
            ("https://big.example/u/a", Some("https://big.example/inbox")),
            ("https://big.example/u/b", Some("https://big.example/inbox")),
            ("https://big.example/u/c", Some("https://big.example/inbox")),
            ("https://small.example/u/d", None),
        ];
        for (ap_id, shared) in followers {
            insert_remote_actor(db, ap_id, shared).await;
            taverna::models::_entities::follows::ActiveModel {
                id: Set(uuid::Uuid::new_v4()),
                follower_id: Set(None),
                remote_follower_ap_id: Set(Some(ap_id.to_string())),
                following_id: Set(None),
                community_id: Set(Some(community.id)),
                remote_following_ap_id: Set(None),
                ap_id: Set(None),
                accepted: Set(true),
                created_at: Set(chrono::Utc::now().into()),
            }
            .insert(db)
            .await
            .unwrap();
        }

        let mut inboxes =
            taverna::workers::deliver_activity::get_follower_inboxes_deduplicated(db, community.id)
                .await
                .unwrap();
        inboxes.sort();
        assert_eq!(
            inboxes,
            vec![
                "https://big.example/inbox".to_string(),
                "https://small.example/u/d/inbox".to_string(),
            ]
        );
    }
}
