use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            r#"
            CREATE TABLE posts (
              id            UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
              ap_id         TEXT UNIQUE NOT NULL,
              community_id  UUID REFERENCES communities(id) ON DELETE CASCADE,
              author_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
              title         TEXT NOT NULL,
              body          TEXT,
              url           TEXT,
              score         INT NOT NULL DEFAULT 0,
              comment_count INT NOT NULL DEFAULT 0,
              is_deleted    BOOL NOT NULL DEFAULT false,
              is_removed    BOOL NOT NULL DEFAULT false,
              is_locked     BOOL NOT NULL DEFAULT false,
              is_pinned     BOOL NOT NULL DEFAULT false,
              published_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
              created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
              updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
              CONSTRAINT post_has_content CHECK (body IS NOT NULL OR url IS NOT NULL)
            );
            CREATE INDEX idx_posts_community_id ON posts(community_id);
            CREATE INDEX idx_posts_author_id ON posts(author_id);
            CREATE INDEX idx_posts_published_at ON posts(published_at DESC);
            CREATE INDEX idx_posts_score ON posts(score DESC);

            CREATE TABLE comments (
              id            UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
              ap_id         TEXT UNIQUE NOT NULL,
              post_id       UUID NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
              author_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
              parent_id     UUID,
              UNIQUE (id, post_id),
              path          LTREE NOT NULL,
              body          TEXT NOT NULL,
              score         INT NOT NULL DEFAULT 0,
              is_deleted    BOOL NOT NULL DEFAULT false,
              is_removed    BOOL NOT NULL DEFAULT false,
              published_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
              created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
              updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );
            ALTER TABLE comments ADD FOREIGN KEY (parent_id, post_id) REFERENCES comments(id, post_id) ON DELETE CASCADE;
            CREATE INDEX idx_comments_post_id ON comments(post_id);
            CREATE INDEX idx_comments_path ON comments USING GIST(path);
            "#,
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS comments CASCADE;")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS posts CASCADE;")
            .await?;
        Ok(())
    }
}
