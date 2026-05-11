use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            r#"
            CREATE TABLE votes (
              id          UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
              user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
              post_id     UUID REFERENCES posts(id) ON DELETE CASCADE,
              comment_id  UUID REFERENCES comments(id) ON DELETE CASCADE,
              value       SMALLINT NOT NULL CHECK (value IN (1, -1)),
              created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
              CONSTRAINT vote_target CHECK ((post_id IS NULL) != (comment_id IS NULL)),
              CONSTRAINT vote_unique_post    UNIQUE (user_id, post_id),
              CONSTRAINT vote_unique_comment UNIQUE (user_id, comment_id)
            );

            CREATE TABLE community_members (
              community_id  UUID NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
              user_id       UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
              role          TEXT NOT NULL DEFAULT 'member' CHECK (role IN ('member', 'moderator')),
              joined_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
              PRIMARY KEY (community_id, user_id)
            );

            CREATE TABLE follows (
              id            UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
              follower_id   UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
              following_id  UUID REFERENCES users(id) ON DELETE CASCADE,
              community_id  UUID REFERENCES communities(id) ON DELETE CASCADE,
              ap_id         TEXT,
              accepted      BOOL NOT NULL DEFAULT false,
              created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
              CONSTRAINT follow_target CHECK ((following_id IS NULL) != (community_id IS NULL)),
              CONSTRAINT follow_unique_user      UNIQUE (follower_id, following_id),
              CONSTRAINT follow_unique_community UNIQUE (follower_id, community_id)
            );
            "#,
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS follows CASCADE;")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS community_members CASCADE;")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS votes CASCADE;")
            .await?;
        Ok(())
    }
}
