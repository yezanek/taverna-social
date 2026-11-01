//! Purges non-public remote Notes from the federated cache.
//!
//! Before this fix the inbox stored every incoming `Create{Note}`, including
//! Mastodon direct messages and followers-only posts, and the public feeds
//! showed them. The inbox now drops them; this migration removes the copies
//! already cached. A Note is public when its `to` or `cc` (kept in
//! `raw_json`) names the `as:Public` collection.
//!
//! Comment counts bumped by purged remote replies are decremented again.
//! The rows are a cache of remote content (ADR-017), so `down` is a no-op.
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

/// SQL predicate: `raw_json` of the row is not addressed to the public.
const NOT_PUBLIC: &str = r#"
  NOT EXISTS (
    SELECT 1
    FROM jsonb_array_elements_text(
      COALESCE(raw_json->'to', '[]'::jsonb) || COALESCE(raw_json->'cc', '[]'::jsonb)
    ) AS target(v)
    WHERE target.v IN ('https://www.w3.org/ns/activitystreams#Public', 'as:Public', 'Public')
  )
"#;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(&format!(
            r#"
            WITH gone AS (
              DELETE FROM remote_comments WHERE {NOT_PUBLIC}
              RETURNING local_post_id, post_ap_id
            ),
            local_counts AS (
              SELECT local_post_id, COUNT(*) AS n FROM gone
              WHERE local_post_id IS NOT NULL GROUP BY local_post_id
            ),
            fix_local AS (
              UPDATE posts p SET comment_count = GREATEST(p.comment_count - c.n, 0)
              FROM local_counts c WHERE p.id = c.local_post_id
              RETURNING p.id
            ),
            remote_counts AS (
              SELECT post_ap_id, COUNT(*) AS n FROM gone
              WHERE local_post_id IS NULL AND post_ap_id IS NOT NULL GROUP BY post_ap_id
            )
            UPDATE remote_posts rp SET comment_count = GREATEST(rp.comment_count - c.n, 0)
            FROM remote_counts c WHERE rp.ap_id = c.post_ap_id;

            DELETE FROM remote_posts WHERE {NOT_PUBLIC};
            "#
        ))
        .await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Purged rows were a cache of remote content and cannot be restored.
        Ok(())
    }
}
