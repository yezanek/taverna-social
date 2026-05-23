use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            r#"
            CREATE TABLE users (
              id                UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
              username          TEXT UNIQUE NOT NULL,
              email             TEXT UNIQUE,
              password_hash     TEXT,
              display_name      TEXT,
              bio               TEXT,
              avatar_url        TEXT,
              ap_id             TEXT UNIQUE NOT NULL,
              public_key        TEXT NOT NULL DEFAULT '',
              private_key       TEXT,
              is_local          BOOL NOT NULL DEFAULT true,
              is_admin          BOOL NOT NULL DEFAULT false,
              is_banned         BOOL NOT NULL DEFAULT false,
              inbox_url         TEXT NOT NULL DEFAULT '',
              shared_inbox_url  TEXT,
              followers_url     TEXT NOT NULL DEFAULT '',
              last_fetched_at   TIMESTAMPTZ,
              created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
              updated_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );
            CREATE INDEX idx_users_username ON users(username);
            CREATE INDEX idx_users_ap_id ON users(ap_id);
            "#,
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS users CASCADE;")
            .await?;
        Ok(())
    }
}
