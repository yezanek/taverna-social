//! Per-user appearance preferences: the default theme and a custom accent
//! colour. Both are nullable; null means the instance default (light theme,
//! the theme's own amber accent). They are copied into the `taverna_theme`
//! and `taverna_accent` cookies on login and when saved in settings.
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            r#"
            ALTER TABLE users
              ADD COLUMN theme TEXT
                CHECK (theme IN ('light', 'dark', 'sepia')),
              ADD COLUMN accent_color TEXT
                CHECK (accent_color ~ '^#[0-9a-f]{6}$');
            "#,
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            "ALTER TABLE users DROP COLUMN IF EXISTS theme, DROP COLUMN IF EXISTS accent_color;",
        )
        .await?;
        Ok(())
    }
}
