//! Phase 8 — federated content isolation.
//!
//! Remote content moves out of `users` / `posts` / `comments` into dedicated
//! `remote_*` cache tables that can be expired without touching local data
//! (see ADR-017).
//!
//! Beyond what PHASE_8.md spells out, this migration also:
//! - adds `remote_follower_ap_id` / `remote_following_ap_id` to `follows` and
//!   `remote_actor_ap_id` to `votes`, so follows and votes by remote actors
//!   survive the move out of `users`;
//! - moves existing remote posts and comments into `remote_posts` /
//!   `remote_comments` before the remote users are deleted (otherwise the
//!   `ON DELETE CASCADE` on `author_id` would silently drop them);
//! - replaces the idempotency use of `federation_objects` with a small
//!   `received_activities` table (ADR-009), since `federation_objects` is dropped.
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // ── New cache tables ────────────────────────────────────────────
        db.execute_unprepared(
            r#"
            CREATE TABLE remote_actors (
              ap_id             TEXT PRIMARY KEY,
              actor_type        TEXT NOT NULL CHECK (actor_type IN ('Person', 'Group', 'Service')),
              username          TEXT NOT NULL,
              display_name      TEXT,
              bio               TEXT,
              avatar_url        TEXT,
              public_key        TEXT NOT NULL DEFAULT '',
              inbox_url         TEXT NOT NULL DEFAULT '',
              shared_inbox_url  TEXT,
              followers_url     TEXT NOT NULL,
              instance_domain   TEXT NOT NULL,
              raw_json          JSONB NOT NULL,
              fetched_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
              stale_after       TIMESTAMPTZ NOT NULL DEFAULT NOW() + INTERVAL '1 day',
              created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );
            CREATE INDEX idx_remote_actors_domain   ON remote_actors(instance_domain);
            CREATE INDEX idx_remote_actors_stale    ON remote_actors(stale_after);
            CREATE INDEX idx_remote_actors_username ON remote_actors(username);

            CREATE TABLE remote_posts (
              ap_id             TEXT PRIMARY KEY,
              community_id      UUID REFERENCES communities(id) ON DELETE CASCADE,
              author_ap_id      TEXT NOT NULL REFERENCES remote_actors(ap_id) ON DELETE CASCADE,
              title             TEXT,
              body              TEXT,
              url               TEXT,
              score             INT NOT NULL DEFAULT 0,
              comment_count     INT NOT NULL DEFAULT 0,
              instance_domain   TEXT NOT NULL,
              raw_json          JSONB NOT NULL,
              published_at      TIMESTAMPTZ NOT NULL,
              fetched_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
              stale_after       TIMESTAMPTZ NOT NULL DEFAULT NOW() + INTERVAL '7 days',
              is_deleted        BOOL NOT NULL DEFAULT false,
              is_removed        BOOL NOT NULL DEFAULT false
            );
            CREATE INDEX idx_remote_posts_community  ON remote_posts(community_id);
            CREATE INDEX idx_remote_posts_author     ON remote_posts(author_ap_id);
            CREATE INDEX idx_remote_posts_domain     ON remote_posts(instance_domain);
            CREATE INDEX idx_remote_posts_stale      ON remote_posts(stale_after);
            CREATE INDEX idx_remote_posts_published  ON remote_posts(published_at DESC);
            CREATE INDEX idx_remote_posts_score      ON remote_posts(score DESC);

            CREATE TABLE remote_comments (
              ap_id             TEXT PRIMARY KEY,
              post_ap_id        TEXT,
              local_post_id     UUID REFERENCES posts(id) ON DELETE CASCADE,
              parent_ap_id      TEXT,
              author_ap_id      TEXT NOT NULL REFERENCES remote_actors(ap_id) ON DELETE CASCADE,
              body              TEXT NOT NULL,
              score             INT NOT NULL DEFAULT 0,
              instance_domain   TEXT NOT NULL,
              raw_json          JSONB NOT NULL,
              published_at      TIMESTAMPTZ NOT NULL,
              fetched_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
              stale_after       TIMESTAMPTZ NOT NULL DEFAULT NOW() + INTERVAL '7 days',
              is_deleted        BOOL NOT NULL DEFAULT false,
              is_removed        BOOL NOT NULL DEFAULT false
            );
            CREATE INDEX idx_remote_comments_post     ON remote_comments(local_post_id);
            CREATE INDEX idx_remote_comments_author   ON remote_comments(author_ap_id);
            CREATE INDEX idx_remote_comments_stale    ON remote_comments(stale_after);
            CREATE INDEX idx_remote_comments_post_ap  ON remote_comments(post_ap_id);

            CREATE TABLE received_activities (
              ap_id        TEXT PRIMARY KEY,
              received_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );
            CREATE INDEX idx_received_activities_at ON received_activities(received_at);
            "#,
        )
        .await?;

        // ── Follows and votes can reference remote actors ───────────────
        db.execute_unprepared(
            r#"
            ALTER TABLE follows ALTER COLUMN follower_id DROP NOT NULL;
            ALTER TABLE follows ADD COLUMN remote_follower_ap_id TEXT
              REFERENCES remote_actors(ap_id) ON DELETE CASCADE;
            ALTER TABLE follows ADD COLUMN remote_following_ap_id TEXT
              REFERENCES remote_actors(ap_id) ON DELETE CASCADE;
            ALTER TABLE follows DROP CONSTRAINT follow_target;

            ALTER TABLE votes ALTER COLUMN user_id DROP NOT NULL;
            ALTER TABLE votes ADD COLUMN remote_actor_ap_id TEXT
              REFERENCES remote_actors(ap_id) ON DELETE CASCADE;
            "#,
        )
        .await?;

        // ── Move existing remote data out of the local tables ───────────
        db.execute_unprepared(
            r#"
            INSERT INTO remote_actors (ap_id, actor_type, username, display_name, bio,
              avatar_url, public_key, inbox_url, shared_inbox_url, followers_url,
              instance_domain, raw_json, fetched_at)
            SELECT
              ap_id, 'Person',
              -- remote users got synthetic `remote_xxxx` usernames; use the
              -- last path segment of their AP id instead
              COALESCE(NULLIF(regexp_replace(rtrim(ap_id, '/'), '^.*/', ''), ''), username),
              display_name, bio,
              avatar_url, public_key, inbox_url, shared_inbox_url, followers_url,
              split_part(ap_id, '/', 3),
              '{}'::jsonb,
              COALESCE(last_fetched_at, NOW())
            FROM users
            WHERE is_local = false;

            INSERT INTO remote_posts (ap_id, community_id, author_ap_id, title, body, url,
              score, comment_count, instance_domain, raw_json, published_at,
              is_deleted, is_removed)
            SELECT
              p.ap_id, p.community_id, u.ap_id, p.title, p.body, p.url,
              p.score, p.comment_count, split_part(p.ap_id, '/', 3), '{}'::jsonb,
              p.published_at, p.is_deleted, p.is_removed
            FROM posts p
            JOIN users u ON u.id = p.author_id
            WHERE u.is_local = false;

            INSERT INTO remote_comments (ap_id, post_ap_id, local_post_id, parent_ap_id,
              author_ap_id, body, score, instance_domain, raw_json, published_at,
              is_deleted, is_removed)
            SELECT
              c.ap_id,
              p.ap_id,
              CASE WHEN pu.is_local THEN p.id ELSE NULL END,
              COALESCE(parent.ap_id, p.ap_id),
              u.ap_id, c.body, c.score, split_part(c.ap_id, '/', 3), '{}'::jsonb,
              c.published_at, c.is_deleted, c.is_removed
            FROM comments c
            JOIN users u ON u.id = c.author_id
            JOIN posts p ON p.id = c.post_id
            JOIN users pu ON pu.id = p.author_id
            LEFT JOIN comments parent ON parent.id = c.parent_id
            WHERE u.is_local = false;

            UPDATE follows f
               SET remote_follower_ap_id = u.ap_id, follower_id = NULL
              FROM users u
             WHERE u.id = f.follower_id AND u.is_local = false;

            UPDATE follows f
               SET remote_following_ap_id = u.ap_id, following_id = NULL
              FROM users u
             WHERE u.id = f.following_id AND u.is_local = false;

            -- A remote actor following another remote actor is not ours to track.
            DELETE FROM follows
             WHERE remote_follower_ap_id IS NOT NULL AND remote_following_ap_id IS NOT NULL;

            UPDATE votes v
               SET remote_actor_ap_id = u.ap_id, user_id = NULL
              FROM users u
             WHERE u.id = v.user_id AND u.is_local = false;

            -- Local replies to remote comments must survive the remote comment's
            -- removal from `comments`: detach them (their ltree path is unchanged,
            -- so they keep their place in the thread ordering).
            UPDATE comments c
               SET parent_id = NULL
              FROM comments parent
              JOIN users pu ON pu.id = parent.author_id
             WHERE c.parent_id = parent.id AND pu.is_local = false;

            DELETE FROM comments c USING users u
             WHERE u.id = c.author_id AND u.is_local = false;

            DELETE FROM posts p USING users u
             WHERE u.id = p.author_id AND u.is_local = false;

            DELETE FROM users WHERE is_local = false;

            DROP TABLE IF EXISTS federation_objects;
            "#,
        )
        .await?;

        // ── Constraints for the new follow/vote shapes ──────────────────
        db.execute_unprepared(
            r#"
            ALTER TABLE follows ADD CONSTRAINT follow_source
              CHECK (num_nonnulls(follower_id, remote_follower_ap_id) = 1);
            ALTER TABLE follows ADD CONSTRAINT follow_target
              CHECK (num_nonnulls(following_id, community_id, remote_following_ap_id) = 1);
            ALTER TABLE follows ADD CONSTRAINT follow_unique_remote_user
              UNIQUE (remote_follower_ap_id, following_id);
            ALTER TABLE follows ADD CONSTRAINT follow_unique_remote_community
              UNIQUE (remote_follower_ap_id, community_id);
            ALTER TABLE follows ADD CONSTRAINT follow_unique_local_to_remote
              UNIQUE (follower_id, remote_following_ap_id);
            CREATE INDEX idx_follows_remote_follower ON follows(remote_follower_ap_id);
            CREATE INDEX idx_follows_remote_following ON follows(remote_following_ap_id);

            ALTER TABLE votes ADD CONSTRAINT vote_voter
              CHECK (num_nonnulls(user_id, remote_actor_ap_id) = 1);
            ALTER TABLE votes ADD CONSTRAINT vote_unique_remote_post
              UNIQUE (remote_actor_ap_id, post_id);
            ALTER TABLE votes ADD CONSTRAINT vote_unique_remote_comment
              UNIQUE (remote_actor_ap_id, comment_id);
            CREATE INDEX idx_votes_remote_actor ON votes(remote_actor_ap_id);
            "#,
        )
        .await?;

        // ── Unified local + remote feed ─────────────────────────────────
        db.execute_unprepared(
            r#"
            CREATE VIEW community_feed AS
              SELECT
                p.id::text AS id, p.ap_id, p.community_id, u.ap_id AS author_ap_id,
                u.username AS author_name,
                p.title, p.body, p.url, p.score, p.comment_count,
                p.published_at, p.is_deleted, p.is_removed, p.is_locked, p.is_pinned,
                'local' AS origin
              FROM posts p
              JOIN users u ON u.id = p.author_id
              WHERE p.is_deleted = false AND p.is_removed = false

              UNION ALL

              SELECT
                md5(rp.ap_id) AS id,
                rp.ap_id, rp.community_id, rp.author_ap_id,
                ra.username || '@' || ra.instance_domain AS author_name,
                COALESCE(rp.title, '') AS title, rp.body, rp.url, rp.score, rp.comment_count,
                rp.published_at, rp.is_deleted, rp.is_removed,
                false AS is_locked, false AS is_pinned,
                'remote' AS origin
              FROM remote_posts rp
              JOIN remote_actors ra ON ra.ap_id = rp.author_ap_id
              WHERE rp.is_deleted = false AND rp.is_removed = false;
            "#,
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // Remote posts and comments are a cache and are not moved back; remote
        // actors are restored to `users` so follows and votes keep working.
        db.execute_unprepared(
            r#"
            DROP VIEW IF EXISTS community_feed;

            CREATE TABLE federation_objects (
              id          UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
              ap_id       TEXT UNIQUE NOT NULL,
              object_type TEXT NOT NULL,
              object_json JSONB NOT NULL,
              fetched_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
              created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
              updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );

            INSERT INTO users (id, username, display_name, bio, avatar_url, ap_id,
              public_key, is_local, inbox_url, shared_inbox_url, followers_url,
              last_fetched_at)
            SELECT uuid_generate_v4(),
              'remote_' || substr(md5(ra.ap_id), 1, 8),
              ra.display_name, ra.bio, ra.avatar_url, ra.ap_id,
              ra.public_key, false, ra.inbox_url, ra.shared_inbox_url, ra.followers_url,
              ra.fetched_at
            FROM remote_actors ra
            WHERE NOT EXISTS (SELECT 1 FROM users u WHERE u.ap_id = ra.ap_id);

            ALTER TABLE votes DROP CONSTRAINT IF EXISTS vote_voter;
            ALTER TABLE follows DROP CONSTRAINT IF EXISTS follow_source;
            ALTER TABLE follows DROP CONSTRAINT IF EXISTS follow_target;

            UPDATE follows f SET follower_id = u.id
              FROM users u WHERE u.ap_id = f.remote_follower_ap_id;
            UPDATE follows f SET following_id = u.id
              FROM users u WHERE u.ap_id = f.remote_following_ap_id;
            UPDATE votes v SET user_id = u.id
              FROM users u WHERE u.ap_id = v.remote_actor_ap_id;
            DELETE FROM follows WHERE follower_id IS NULL;
            DELETE FROM votes WHERE user_id IS NULL;

            ALTER TABLE votes DROP CONSTRAINT IF EXISTS vote_unique_remote_post;
            ALTER TABLE votes DROP CONSTRAINT IF EXISTS vote_unique_remote_comment;
            ALTER TABLE votes DROP COLUMN IF EXISTS remote_actor_ap_id;
            ALTER TABLE votes ALTER COLUMN user_id SET NOT NULL;

            ALTER TABLE follows DROP CONSTRAINT IF EXISTS follow_unique_remote_user;
            ALTER TABLE follows DROP CONSTRAINT IF EXISTS follow_unique_remote_community;
            ALTER TABLE follows DROP CONSTRAINT IF EXISTS follow_unique_local_to_remote;
            ALTER TABLE follows DROP COLUMN IF EXISTS remote_follower_ap_id;
            ALTER TABLE follows DROP COLUMN IF EXISTS remote_following_ap_id;
            ALTER TABLE follows ALTER COLUMN follower_id SET NOT NULL;
            ALTER TABLE follows ADD CONSTRAINT follow_target
              CHECK ((following_id IS NULL) != (community_id IS NULL));

            DROP TABLE IF EXISTS received_activities;
            DROP TABLE IF EXISTS remote_comments;
            DROP TABLE IF EXISTS remote_posts;
            DROP TABLE IF EXISTS remote_actors;
            "#,
        )
        .await?;

        Ok(())
    }
}
