//! Phase 8 — indexes for the hot query paths (PHASE_8.md Part B1).
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            r#"
            -- Feed sorting: most common query pattern
            CREATE INDEX IF NOT EXISTS idx_posts_community_score
              ON posts(community_id, score DESC, published_at DESC)
              WHERE is_deleted = false AND is_removed = false;

            CREATE INDEX IF NOT EXISTS idx_remote_posts_community_score
              ON remote_posts(community_id, score DESC, published_at DESC)
              WHERE is_deleted = false AND is_removed = false;

            -- Home feed (all communities)
            CREATE INDEX IF NOT EXISTS idx_posts_score_published
              ON posts(score DESC, published_at DESC)
              WHERE is_deleted = false AND is_removed = false;

            CREATE INDEX IF NOT EXISTS idx_remote_posts_score_published
              ON remote_posts(score DESC, published_at DESC)
              WHERE is_deleted = false AND is_removed = false;

            -- User post history (keyset on published_at, id)
            CREATE INDEX IF NOT EXISTS idx_posts_author_published
              ON posts(author_id, published_at DESC, id DESC)
              WHERE is_deleted = false;

            -- Comment threading: btree for equality lookups alongside the GIST index
            CREATE INDEX IF NOT EXISTS idx_comments_post_path
              ON comments(post_id, path);

            -- Votes: checking a user's existing vote before casting
            CREATE INDEX IF NOT EXISTS idx_votes_user_post
              ON votes(user_id, post_id) WHERE post_id IS NOT NULL;
            CREATE INDEX IF NOT EXISTS idx_votes_user_comment
              ON votes(user_id, comment_id) WHERE comment_id IS NOT NULL;

            -- Follows: fetching follower inboxes for fan-out
            CREATE INDEX IF NOT EXISTS idx_follows_community_accepted
              ON follows(community_id, accepted)
              WHERE accepted = true AND community_id IS NOT NULL;
            CREATE INDEX IF NOT EXISTS idx_follows_user_accepted
              ON follows(following_id, accepted)
              WHERE accepted = true AND following_id IS NOT NULL;

            -- outbound_activities: worker polling query
            CREATE INDEX IF NOT EXISTS idx_outbound_state_next
              ON outbound_activities(state, next_attempt_at)
              WHERE state = 'pending';

            -- Remote actors: re-fetch scheduling
            CREATE INDEX IF NOT EXISTS idx_remote_actors_stale_domain
              ON remote_actors(stale_after, instance_domain);
            "#,
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            r#"
            DROP INDEX IF EXISTS idx_posts_community_score;
            DROP INDEX IF EXISTS idx_remote_posts_community_score;
            DROP INDEX IF EXISTS idx_posts_score_published;
            DROP INDEX IF EXISTS idx_remote_posts_score_published;
            DROP INDEX IF EXISTS idx_posts_author_published;
            DROP INDEX IF EXISTS idx_comments_post_path;
            DROP INDEX IF EXISTS idx_votes_user_post;
            DROP INDEX IF EXISTS idx_votes_user_comment;
            DROP INDEX IF EXISTS idx_follows_community_accepted;
            DROP INDEX IF EXISTS idx_follows_user_accepted;
            DROP INDEX IF EXISTS idx_outbound_state_next;
            DROP INDEX IF EXISTS idx_remote_actors_stale_domain;
            "#,
        )
        .await?;
        Ok(())
    }
}
