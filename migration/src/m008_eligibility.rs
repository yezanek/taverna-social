use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            r#"
            CREATE TABLE instance_settings (
              id                          BOOL PRIMARY KEY DEFAULT true
                                            CHECK (id = true),
              community_creation_open     BOOL NOT NULL DEFAULT true,
              require_min_account_age     BOOL NOT NULL DEFAULT false,
              min_account_age_days        INT  NOT NULL DEFAULT 30,
              require_email_verified      BOOL NOT NULL DEFAULT false,
              require_min_standing        BOOL NOT NULL DEFAULT false,
              min_standing_score          INT  NOT NULL DEFAULT 0,
              disqualify_if_content_removed  BOOL NOT NULL DEFAULT false,
              disqualify_if_suspended        BOOL NOT NULL DEFAULT false,
              instance_name               TEXT NOT NULL DEFAULT 'Taverna',
              open_registrations          BOOL NOT NULL DEFAULT true,
              updated_at                  TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );

            INSERT INTO instance_settings DEFAULT VALUES;
            "#,
        )
        .await?;

        db.execute_unprepared(
            r#"
            CREATE TABLE email_verifications (
              id          UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
              user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
              token       TEXT NOT NULL UNIQUE,
              expires_at  TIMESTAMPTZ NOT NULL DEFAULT NOW() + INTERVAL '24 hours',
              verified_at TIMESTAMPTZ,
              created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );
            CREATE INDEX idx_email_verif_token ON email_verifications(token);
            CREATE INDEX idx_email_verif_user  ON email_verifications(user_id);
            "#,
        )
        .await?;

        db.execute_unprepared(
            r#"
            ALTER TABLE users ADD COLUMN email_verified   BOOL NOT NULL DEFAULT false;
            ALTER TABLE users ADD COLUMN standing_score   INT  NOT NULL DEFAULT 0;
            ALTER TABLE users ADD COLUMN suspended_at     TIMESTAMPTZ;
            "#,
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            r#"
            ALTER TABLE users DROP COLUMN IF EXISTS suspended_at;
            ALTER TABLE users DROP COLUMN IF EXISTS standing_score;
            ALTER TABLE users DROP COLUMN IF EXISTS email_verified;
            "#,
        )
        .await?;

        db.execute_unprepared("DROP TABLE IF EXISTS email_verifications CASCADE;")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS instance_settings CASCADE;")
            .await?;

        Ok(())
    }
}
