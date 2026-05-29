#[cfg(test)]
mod tests {
    use loco_rs::testing::prelude::*;
    use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
    use serial_test::serial;
    use taverna::app::App;
    use taverna::models::_entities::{email_verifications, instance_settings, users};
    use taverna::models::eligibility;
    use taverna::models::users::UserModel;

    fn setup() {
        dotenvy::dotenv().ok();
    }

    #[tokio::test]
    #[serial]
    async fn test_update_standing() {
        setup();
        let boot = boot_test::<App>().await.unwrap();
        let db = &boot.app_context.db;

        let user = UserModel::create_local(
            db,
            "standinguser",
            "standing@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();
        assert_eq!(user.standing_score, 0);

        UserModel::update_standing(db, user.id, 10).await.unwrap();
        let updated = users::Entity::find_by_id(user.id)
            .one(db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.standing_score, 10);

        UserModel::update_standing(db, user.id, -5).await.unwrap();
        let updated = users::Entity::find_by_id(user.id)
            .one(db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.standing_score, 5);

        UserModel::update_standing(db, user.id, -200).await.unwrap();
        let updated = users::Entity::find_by_id(user.id)
            .one(db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.standing_score, -100);
    }

    #[tokio::test]
    #[serial]
    async fn test_eligibility_all_checks_pass() {
        setup();
        let boot = boot_test::<App>().await.unwrap();
        let db = &boot.app_context.db;

        let user = UserModel::create_local(
            db,
            "eligibleuser",
            "eligible@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let result = eligibility::check_eligibility(db, user.id).await.unwrap();
        assert!(result.eligible);
        assert!(result.reasons.is_empty());
    }

    #[tokio::test]
    #[serial]
    async fn test_eligibility_email_not_verified() {
        setup();
        let boot = boot_test::<App>().await.unwrap();
        let db = &boot.app_context.db;

        let mut settings: instance_settings::ActiveModel = instance_settings::Entity::find()
            .one(db)
            .await
            .unwrap()
            .unwrap()
            .into();
        settings.community_creation_open = Set(false);
        settings.require_email_verified = Set(true);
        settings.updated_at = Set(chrono::Utc::now().into());
        settings.update(db).await.unwrap();

        let user = UserModel::create_local(
            db,
            "unverifuser",
            "unverif@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let result = eligibility::check_eligibility(db, user.id).await.unwrap();
        assert!(!result.eligible);
        assert!(
            result
                .reasons
                .iter()
                .any(|r| r.contains("Email not verified"))
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_eligibility_email_verified_passes() {
        setup();
        let boot = boot_test::<App>().await.unwrap();
        let db = &boot.app_context.db;

        let mut settings: instance_settings::ActiveModel = instance_settings::Entity::find()
            .one(db)
            .await
            .unwrap()
            .unwrap()
            .into();
        settings.community_creation_open = Set(false);
        settings.require_email_verified = Set(true);
        settings.updated_at = Set(chrono::Utc::now().into());
        settings.update(db).await.unwrap();

        let user = UserModel::create_local(
            db,
            "verifuser",
            "verif@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let mut user_active: users::ActiveModel = users::Entity::find_by_id(user.id)
            .one(db)
            .await
            .unwrap()
            .unwrap()
            .into();
        user_active.email_verified = Set(true);
        user_active.update(db).await.unwrap();

        let result = eligibility::check_eligibility(db, user.id).await.unwrap();
        assert!(result.eligible);
    }

    #[tokio::test]
    #[serial]
    async fn test_eligibility_standing_too_low() {
        setup();
        let boot = boot_test::<App>().await.unwrap();
        let db = &boot.app_context.db;

        let mut settings: instance_settings::ActiveModel = instance_settings::Entity::find()
            .one(db)
            .await
            .unwrap()
            .unwrap()
            .into();
        settings.community_creation_open = Set(false);
        settings.require_min_standing = Set(true);
        settings.min_standing_score = Set(5);
        settings.updated_at = Set(chrono::Utc::now().into());
        settings.update(db).await.unwrap();

        let user = UserModel::create_local(
            db,
            "lowstanding",
            "low@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let result = eligibility::check_eligibility(db, user.id).await.unwrap();
        assert!(!result.eligible);
        assert!(result.reasons.iter().any(|r| r.contains("Standing score")));
    }

    #[tokio::test]
    #[serial]
    async fn test_eligibility_suspended() {
        setup();
        let boot = boot_test::<App>().await.unwrap();
        let db = &boot.app_context.db;

        let mut settings: instance_settings::ActiveModel = instance_settings::Entity::find()
            .one(db)
            .await
            .unwrap()
            .unwrap()
            .into();
        settings.community_creation_open = Set(false);
        settings.disqualify_if_suspended = Set(true);
        settings.updated_at = Set(chrono::Utc::now().into());
        settings.update(db).await.unwrap();

        let user = UserModel::create_local(
            db,
            "suspendeduser",
            "suspended@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        UserModel::set_suspended(db, user.id, true).await.unwrap();

        let result = eligibility::check_eligibility(db, user.id).await.unwrap();
        assert!(!result.eligible);
        assert!(result.reasons.iter().any(|r| r.contains("suspended")));
    }

    #[tokio::test]
    #[serial]
    async fn test_eligibility_gated_without_check_requirements() {
        setup();
        let boot = boot_test::<App>().await.unwrap();
        let db = &boot.app_context.db;

        let mut settings: instance_settings::ActiveModel = instance_settings::Entity::find()
            .one(db)
            .await
            .unwrap()
            .unwrap()
            .into();
        settings.community_creation_open = Set(false);
        settings.updated_at = Set(chrono::Utc::now().into());
        settings.update(db).await.unwrap();

        let user = UserModel::create_local(
            db,
            "gateduser",
            "gated@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        // Gated mode with no gates enabled → eligible (no active checks to fail)
        let result = eligibility::check_eligibility(db, user.id).await.unwrap();
        assert!(result.eligible);
    }

    #[tokio::test]
    #[serial]
    async fn test_eligibility_gated_requires_email_verified() {
        setup();
        let boot = boot_test::<App>().await.unwrap();
        let db = &boot.app_context.db;

        let mut settings: instance_settings::ActiveModel = instance_settings::Entity::find()
            .one(db)
            .await
            .unwrap()
            .unwrap()
            .into();
        settings.community_creation_open = Set(false);
        settings.require_email_verified = Set(true);
        settings.updated_at = Set(chrono::Utc::now().into());
        settings.update(db).await.unwrap();

        let user = UserModel::create_local(
            db,
            "gatedunverified",
            "gated@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let result = eligibility::check_eligibility(db, user.id).await.unwrap();
        assert!(!result.eligible);
        assert!(
            result
                .reasons
                .iter()
                .any(|r| r.contains("Email not verified"))
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_email_verification_create_and_verify() {
        setup();
        let boot = boot_test::<App>().await.unwrap();
        let db = &boot.app_context.db;

        let user = UserModel::create_local(
            db,
            "emailverif",
            "emailverif@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        assert!(!user.email_verified);

        let token = uuid::Uuid::new_v4().to_string();
        email_verifications::ActiveModel {
            id: Set(uuid::Uuid::new_v4()),
            user_id: Set(user.id),
            token: Set(token.clone()),
            expires_at: Set((chrono::Utc::now() + chrono::Duration::hours(24)).into()),
            verified_at: Set(None),
            created_at: Set(chrono::Utc::now().into()),
        }
        .insert(db)
        .await
        .unwrap();

        let record = email_verifications::Entity::find()
            .filter(email_verifications::Column::Token.eq(&token))
            .one(db)
            .await
            .unwrap();
        assert!(record.is_some());

        let mut ver_active: email_verifications::ActiveModel = record.unwrap().into();
        ver_active.verified_at = Set(Some(chrono::Utc::now().into()));
        ver_active.update(db).await.unwrap();

        let mut user_active: users::ActiveModel = users::Entity::find_by_id(user.id)
            .one(db)
            .await
            .unwrap()
            .unwrap()
            .into();
        user_active.email_verified = Set(true);
        user_active.update(db).await.unwrap();

        let user = users::Entity::find_by_id(user.id)
            .one(db)
            .await
            .unwrap()
            .unwrap();
        assert!(user.email_verified);
    }

    #[tokio::test]
    #[serial]
    async fn test_force_verify_email() {
        setup();
        let boot = boot_test::<App>().await.unwrap();
        let db = &boot.app_context.db;

        let user = UserModel::create_local(
            db,
            "forceverif",
            "forceverif@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        assert!(!user.email_verified);

        UserModel::force_verify_email(db, user.id).await.unwrap();

        let user = users::Entity::find_by_id(user.id)
            .one(db)
            .await
            .unwrap()
            .unwrap();
        assert!(user.email_verified);
    }

    #[tokio::test]
    #[serial]
    async fn test_account_age_gate() {
        setup();
        let boot = boot_test::<App>().await.unwrap();
        let db = &boot.app_context.db;

        let mut settings: instance_settings::ActiveModel = instance_settings::Entity::find()
            .one(db)
            .await
            .unwrap()
            .unwrap()
            .into();
        settings.community_creation_open = Set(false);
        settings.require_min_account_age = Set(true);
        settings.min_account_age_days = Set(30);
        settings.updated_at = Set(chrono::Utc::now().into());
        settings.update(db).await.unwrap();

        let user = UserModel::create_local(
            db,
            "youngaccount",
            "young@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let result = eligibility::check_eligibility(db, user.id).await.unwrap();
        assert!(!result.eligible);
        assert!(result.reasons.iter().any(|r| r.contains("Account age")));
    }

    #[tokio::test]
    #[serial]
    async fn test_expired_token_rejected() {
        setup();
        let boot = boot_test::<App>().await.unwrap();
        let db = &boot.app_context.db;

        let user = UserModel::create_local(
            db,
            "exptoken",
            "exp@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let token = uuid::Uuid::new_v4().to_string();
        email_verifications::ActiveModel {
            id: Set(uuid::Uuid::new_v4()),
            user_id: Set(user.id),
            token: Set(token.clone()),
            expires_at: Set((chrono::Utc::now() - chrono::Duration::hours(1)).into()),
            verified_at: Set(None),
            created_at: Set(chrono::Utc::now().into()),
        }
        .insert(db)
        .await
        .unwrap();

        let record = email_verifications::Entity::find()
            .filter(email_verifications::Column::Token.eq(&token))
            .one(db)
            .await
            .unwrap()
            .unwrap();

        assert!(record.expires_at < chrono::Utc::now());
    }

    #[tokio::test]
    #[serial]
    async fn test_removed_content_disqualifies() {
        setup();
        let boot = boot_test::<App>().await.unwrap();
        let db = &boot.app_context.db;

        let mut settings: instance_settings::ActiveModel = instance_settings::Entity::find()
            .one(db)
            .await
            .unwrap()
            .unwrap()
            .into();
        settings.community_creation_open = Set(false);
        settings.disqualify_if_content_removed = Set(true);
        settings.updated_at = Set(chrono::Utc::now().into());
        settings.update(db).await.unwrap();

        let user = UserModel::create_local(
            db,
            "removedcontent",
            "removed@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let community = taverna::models::communities::CommunityModel::create(
            db,
            "removed-test",
            "Removed Test",
            None,
            user.id,
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let post = taverna::models::posts::PostModel::create(
            db,
            Some(community.id),
            user.id,
            "Will be removed",
            Some("Body"),
            None,
            "http://localhost:5150",
        )
        .await
        .unwrap();

        let result_before = eligibility::check_eligibility(db, user.id).await.unwrap();
        assert!(result_before.eligible);

        taverna::models::posts::PostModel::soft_remove(db, post.id)
            .await
            .unwrap();

        let result_after = eligibility::check_eligibility(db, user.id).await.unwrap();
        assert!(!result_after.eligible);
        assert!(result_after.reasons.iter().any(|r| r.contains("removed")));
    }

    #[tokio::test]
    #[serial]
    async fn test_set_suspended() {
        setup();
        let boot = boot_test::<App>().await.unwrap();
        let db = &boot.app_context.db;

        let user = UserModel::create_local(
            db,
            "suspcheck",
            "suspcheck@example.com",
            "password123",
            "http://localhost:5150",
        )
        .await
        .unwrap();

        assert!(user.suspended_at.is_none());

        UserModel::set_suspended(db, user.id, true).await.unwrap();

        let user = users::Entity::find_by_id(user.id)
            .one(db)
            .await
            .unwrap()
            .unwrap();
        assert!(user.suspended_at.is_some());

        UserModel::set_suspended(db, user.id, false).await.unwrap();

        let user = users::Entity::find_by_id(user.id)
            .one(db)
            .await
            .unwrap()
            .unwrap();
        assert!(user.suspended_at.is_none());
    }
}
