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
            std::env::set_var("DOMAIN", "localhost:5150");
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
        assert!(ct.contains("application/json"));

        let body: serde_json::Value = response.json();
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
        assert_eq!(body["content"], "A test comment");
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
        assert_eq!(body["software"]["version"], "0.1.0");
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
        let _community = create_test_community(&ctx.db, user.id).await;

        let response = request
            .get("/c/fed-community/outbox")
            .add_header("Accept", AP_JSON)
            .await;
        assert_eq!(response.status_code(), 200);

        let body: serde_json::Value = response.json();
        assert_eq!(body["type"], "OrderedCollection");
    }
}
