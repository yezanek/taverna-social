use crate::models::_entities::federation_objects;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use serde_json::Value;
use tracing::instrument;

const CACHE_TTL_MINUTES: i64 = 30;

pub struct FederationObjectModel;

impl FederationObjectModel {
    #[instrument(skip(db))]
    pub async fn get_fresh(
        db: &DatabaseConnection,
        ap_id: &str,
    ) -> Result<Option<Value>, sea_orm::DbErr> {
        let cached = federation_objects::Entity::find()
            .filter(federation_objects::Column::ApId.eq(ap_id))
            .one(db)
            .await?;

        match cached {
            Some(obj) => {
                let now = chrono::Utc::now();
                let fetched = obj.fetched_at.with_timezone(&chrono::Utc);
                let age = now.signed_duration_since(fetched).num_minutes().abs();
                if age < CACHE_TTL_MINUTES {
                    Ok(Some(obj.object_json))
                } else {
                    Ok(None)
                }
            }
            None => Ok(None),
        }
    }

    #[instrument(skip(db))]
    pub async fn upsert(
        db: &DatabaseConnection,
        ap_id: &str,
        object_type: &str,
        json: &Value,
    ) -> Result<(), sea_orm::DbErr> {
        let existing = federation_objects::Entity::find()
            .filter(federation_objects::Column::ApId.eq(ap_id))
            .one(db)
            .await?;

        let now = chrono::Utc::now().into();
        if let Some(row) = existing {
            let mut active: federation_objects::ActiveModel = row.into();
            active.object_type = Set(object_type.to_string());
            active.object_json = Set(json.clone());
            active.fetched_at = Set(now);
            active.updated_at = Set(now);
            active.update(db).await?;
        } else {
            federation_objects::ActiveModel {
                id: Set(uuid::Uuid::new_v4()),
                ap_id: Set(ap_id.to_string()),
                object_type: Set(object_type.to_string()),
                object_json: Set(json.clone()),
                fetched_at: Set(now),
                created_at: Set(now),
                updated_at: Set(now),
            }
            .insert(db)
            .await?;
        }
        Ok(())
    }
}
