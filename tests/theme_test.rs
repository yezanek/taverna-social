#[cfg(test)]
mod tests {
    use loco_rs::testing::prelude::*;
    use serial_test::serial;
    use taverna::app::App;

    fn setup() {
        dotenvy::dotenv().ok();
    }

    #[tokio::test]
    #[serial]
    async fn test_set_theme_light() {
        setup();
        request::<App, _, _>(|request, _ctx| async move {
            let response = request
                .post("/theme")
                .form(&serde_json::json!({"theme": "light"}))
                .await;
            assert!(response.status_code() == 302 || response.status_code() == 303);
            let jar = response.cookies();
            let cookie = jar.iter().find(|c| c.name() == "taverna_theme");
            assert!(cookie.is_some());
            assert_eq!(cookie.unwrap().value(), "light");
        })
        .await;
    }

    #[tokio::test]
    #[serial]
    async fn test_set_theme_dark() {
        setup();
        request::<App, _, _>(|request, _ctx| async move {
            let response = request
                .post("/theme")
                .form(&serde_json::json!({"theme": "dark"}))
                .await;
            assert!(response.status_code() == 302 || response.status_code() == 303);
            let jar = response.cookies();
            let cookie = jar.iter().find(|c| c.name() == "taverna_theme");
            assert!(cookie.is_some());
            assert_eq!(cookie.unwrap().value(), "dark");
        })
        .await;
    }

    #[tokio::test]
    #[serial]
    async fn test_set_theme_sepia() {
        setup();
        request::<App, _, _>(|request, _ctx| async move {
            let response = request
                .post("/theme")
                .form(&serde_json::json!({"theme": "sepia"}))
                .await;
            assert!(response.status_code() == 302 || response.status_code() == 303);
            let jar = response.cookies();
            let cookie = jar.iter().find(|c| c.name() == "taverna_theme");
            assert!(cookie.is_some());
            assert_eq!(cookie.unwrap().value(), "sepia");
        })
        .await;
    }

    #[tokio::test]
    #[serial]
    async fn test_invalid_theme_defaults_to_light() {
        setup();
        request::<App, _, _>(|request, _ctx| async move {
            let response = request
                .post("/theme")
                .form(&serde_json::json!({"theme": "hotdog"}))
                .await;
            assert!(response.status_code() == 302 || response.status_code() == 303);
            let jar = response.cookies();
            let cookie = jar.iter().find(|c| c.name() == "taverna_theme");
            assert!(cookie.is_some());
            assert_eq!(cookie.unwrap().value(), "light");
        })
        .await;
    }

    #[tokio::test]
    #[serial]
    async fn test_theme_cookie_max_age() {
        setup();
        request::<App, _, _>(|request, _ctx| async move {
            let response = request
                .post("/theme")
                .form(&serde_json::json!({"theme": "dark"}))
                .await;
            let jar = response.cookies();
            let cookie = jar.iter().find(|c| c.name() == "taverna_theme");
            assert!(cookie.is_some());
            let cookie = cookie.unwrap();
            // Max-Age should be set (cookie.max_age() returns Option<Duration>)
            assert!(cookie.max_age().is_some());
            let max_age = cookie.max_age().unwrap();
            assert_eq!(max_age.whole_seconds(), 31536000);
        })
        .await;
    }

    #[tokio::test]
    #[serial]
    async fn test_theme_middleware_reads_cookie() {
        setup();
        request::<App, _, _>(|request, _ctx| async move {
            let response = request
                .get("/")
                .add_header("Cookie", "taverna_theme=dark")
                .await;
            // Should render normally (200) — middleware injects theme silently
            assert_eq!(response.status_code(), 200);
        })
        .await;
    }
}
