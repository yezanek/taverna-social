use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            r#"
            CREATE TABLE communities (
              id                  UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
              slug                TEXT UNIQUE NOT NULL,
              name                TEXT NOT NULL,
              description         TEXT,
              ap_id               TEXT UNIQUE NOT NULL,
              public_key          TEXT NOT NULL DEFAULT '',
              private_key         TEXT,
              inbox_url           TEXT NOT NULL DEFAULT '',
              followers_url       TEXT NOT NULL DEFAULT '',
              is_local            BOOL NOT NULL DEFAULT true,
              is_nsfw             BOOL NOT NULL DEFAULT false,
              posting_restricted  BOOL NOT NULL DEFAULT false,
              last_fetched_at     TIMESTAMPTZ,
              created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
              updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );
            "#,
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS communities CASCADE;")
            .await?;
        Ok(())
    }
}
