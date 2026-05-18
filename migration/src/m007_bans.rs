use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            r#"
            CREATE TABLE community_bans (
              community_id  UUID NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
              user_id       UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
              banned_by     UUID NOT NULL REFERENCES users(id),
              reason        TEXT,
              created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
              PRIMARY KEY (community_id, user_id)
            );
            "#,
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS community_bans CASCADE;")
            .await?;
        Ok(())
    }
}
