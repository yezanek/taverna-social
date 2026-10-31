//! Phase 8 timing bounds for the two hottest queries (PHASE_8.md Task 8.11).
//! Rows are bulk-inserted with SQL so setup stays fast.
mod common;

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Statement};
    use serial_test::serial;
    use std::time::{Duration, Instant};
    use taverna::models::communities::CommunityModel;
    use taverna::models::users::UserModel;

    const BASE_URL: &str = "http://localhost:5150";

    async fn setup_community(db: &sea_orm::DatabaseConnection) -> (uuid::Uuid, uuid::Uuid) {
        let user =
            UserModel::create_local(db, "perfuser", "perf@test.com", "password123", BASE_URL)
                .await
                .unwrap();
        let community = CommunityModel::create(db, "perf", "Perf", None, user.id, BASE_URL)
            .await
            .unwrap();
        (user.id, community.id)
    }

    async fn exec(db: &sea_orm::DatabaseConnection, sql: &str, values: Vec<sea_orm::Value>) {
        db.execute_raw(Statement::from_sql_and_values(
            db.get_database_backend(),
            sql,
            values,
        ))
        .await
        .unwrap();
    }

    /// Best of a few runs, so one cold cache or scheduler hiccup doesn't fail the test.
    async fn best_of<F, Fut>(runs: usize, mut f: F) -> Duration
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = ()>,
    {
        let mut best = Duration::MAX;
        for _ in 0..runs {
            let start = Instant::now();
            f().await;
            best = best.min(start.elapsed());
        }
        best
    }

    #[tokio::test]
    #[serial]
    async fn test_feed_query_under_100ms_with_1000_posts() {
        let boot = crate::common::shared_boot().await;
        let db = &boot.app_context.db;
        let (user_id, community_id) = setup_community(db).await;

        exec(
            db,
            r#"INSERT INTO posts (ap_id, community_id, author_id, title, body, score, published_at)
               SELECT 'http://localhost:5150/posts/perf-' || g, $1, $2, 'Post ' || g, 'body',
                      (g % 50), NOW() - (g || ' minutes')::interval
                 FROM generate_series(1, 1000) g"#,
            vec![community_id.into(), user_id.into()],
        )
        .await;
        exec(db, "ANALYZE posts", vec![]).await;

        let elapsed = best_of(3, || async {
            let page = taverna::models::feed::community_feed(db, community_id, None, 25)
                .await
                .unwrap();
            assert_eq!(page.items.len(), 25);
        })
        .await;
        assert!(
            elapsed < Duration::from_millis(100),
            "feed query took {elapsed:?}"
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_follower_inbox_query_under_50ms_with_10000_followers() {
        let boot = crate::common::shared_boot().await;
        let db = &boot.app_context.db;
        let (_, community_id) = setup_community(db).await;

        // 10,000 followers spread over 200 instances, each with a shared inbox.
        exec(
            db,
            r#"INSERT INTO remote_actors (ap_id, actor_type, username, public_key, inbox_url,
                 shared_inbox_url, followers_url, instance_domain, raw_json)
               SELECT 'https://i' || (g % 200) || '.example/u/' || g, 'Person', 'u' || g, '',
                      'https://i' || (g % 200) || '.example/u/' || g || '/inbox',
                      'https://i' || (g % 200) || '.example/inbox',
                      'https://i' || (g % 200) || '.example/u/' || g || '/followers',
                      'i' || (g % 200) || '.example', '{}'::jsonb
                 FROM generate_series(1, 10000) g"#,
            vec![],
        )
        .await;
        exec(
            db,
            r#"INSERT INTO follows (remote_follower_ap_id, community_id, accepted)
               SELECT ap_id, $1, true FROM remote_actors"#,
            vec![community_id.into()],
        )
        .await;
        exec(db, "ANALYZE follows", vec![]).await;
        exec(db, "ANALYZE remote_actors", vec![]).await;

        let elapsed = best_of(3, || async {
            let inboxes = taverna::workers::deliver_activity::get_follower_inboxes_deduplicated(
                db,
                community_id,
            )
            .await
            .unwrap();
            assert_eq!(inboxes.len(), 200, "one shared inbox per instance");
        })
        .await;
        assert!(
            elapsed < Duration::from_millis(50),
            "follower inbox query took {elapsed:?}"
        );
    }
}
