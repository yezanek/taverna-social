use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            r#"
            CREATE TABLE outbound_activities (
              id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
              activity_json   JSONB NOT NULL,
              target_inboxes  TEXT[] NOT NULL,
              actor_ap_id     TEXT NOT NULL,
              state           TEXT NOT NULL DEFAULT 'pending' CHECK (state IN ('pending','sent','failed')),
              attempts        INT NOT NULL DEFAULT 0,
              next_attempt_at TIMESTAMPTZ DEFAULT NOW(),
              last_error      TEXT,
              created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );
            CREATE INDEX idx_outbound_pending ON outbound_activities(next_attempt_at)
              WHERE state = 'pending';

            CREATE TABLE federation_objects (
              id          UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
              ap_id       TEXT UNIQUE NOT NULL,
              object_type TEXT NOT NULL,
              object_json JSONB NOT NULL,
              fetched_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
              created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
              updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );
            "#,
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS outbound_activities CASCADE;")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS federation_objects CASCADE;")
            .await?;
        Ok(())
    }
}
