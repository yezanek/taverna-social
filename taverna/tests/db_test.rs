#[cfg(test)]
mod tests {
    use loco_rs::testing::request::boot_test;
    use sea_orm::ConnectionTrait;

    #[tokio::test]
    async fn test_db_connection() {
        dotenvy::dotenv().ok();
        let boot_result = boot_test::<taverna::app::App>().await.unwrap();
        let db = &boot_result.app_context.db;
        let result = sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT 1".to_string(),
        );
        db.execute(result).await.unwrap();
    }
}
