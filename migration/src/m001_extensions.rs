use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        // Note: pgmq must be installed separately on the PostgreSQL server
        // up does NOT install pgmq (it requires superuser).
        db.execute_unprepared("CREATE EXTENSION IF NOT EXISTS \"uuid-ossp\";")
            .await?;
        db.execute_unprepared("CREATE EXTENSION IF NOT EXISTS ltree;")
            .await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Do not drop extensions (other things may use them)
        Ok(())
    }
}
