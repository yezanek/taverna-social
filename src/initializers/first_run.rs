use axum::response::{IntoResponse, Redirect, Response};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter};
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::task::{Context, Poll};
use std::time::Instant;
use tower::{Layer, Service};

use crate::models::_entities::users;

static FIRST_RUN_CACHE: std::sync::OnceLock<Mutex<Option<(Instant, bool)>>> =
    std::sync::OnceLock::new();

pub(crate) static DB: std::sync::OnceLock<Mutex<Option<DatabaseConnection>>> =
    std::sync::OnceLock::new();

fn cache_db() -> &'static Mutex<Option<DatabaseConnection>> {
    DB.get_or_init(|| Mutex::new(None))
}

pub fn init(db: DatabaseConnection) {
    if let Ok(mut guard) = cache_db().lock() {
        *guard = Some(db);
    }
}

fn cache() -> &'static Mutex<Option<(Instant, bool)>> {
    FIRST_RUN_CACHE.get_or_init(|| Mutex::new(None))
}

pub async fn is_first_run(db: &DatabaseConnection) -> Result<bool, sea_orm::DbErr> {
    let count = users::Entity::find()
        .filter(users::Column::IsLocal.eq(true))
        .count(db)
        .await?;
    Ok(count == 0)
}

pub fn bust_cache() {
    if let Ok(mut guard) = cache().lock() {
        *guard = Some((Instant::now(), false));
    }
}

async fn cached_is_first_run(db: &DatabaseConnection) -> Result<bool, sea_orm::DbErr> {
    {
        let guard = cache()
            .lock()
            .map_err(|_| sea_orm::DbErr::Custom("Mutex poisoned".to_string()))?;

        if let Some((cached_at, cached_value)) = *guard
            && cached_at.elapsed().as_secs() < 60
        {
            return Ok(cached_value);
        }
    }

    let fresh = is_first_run(db).await?;

    if let Ok(mut guard) = cache().lock() {
        *guard = Some((Instant::now(), fresh));
    }

    Ok(fresh)
}

#[derive(Clone)]
pub struct FirstRunGuard<S> {
    inner: S,
}

impl<S> Service<axum::extract::Request> for FirstRunGuard<S>
where
    S: Service<axum::extract::Request, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: axum::extract::Request) -> Self::Future {
        let path = req.uri().path().to_string();

        if path.starts_with("/static/")
            || path.starts_with("/setup")
            || path.starts_with("/.well-known/")
            || path.starts_with("/nodeinfo/")
            || path.starts_with("/verify-email")
        {
            let future = self.inner.call(req);
            return Box::pin(future);
        }

        let db = cache_db().lock().ok().and_then(|g| g.clone());
        let mut inner = self.inner.clone();

        Box::pin(async move {
            match db {
                Some(db) => match cached_is_first_run(&db).await {
                    Ok(true) => Ok(Redirect::to("/setup").into_response()),
                    Ok(false) => inner.call(req).await,
                    Err(_) => inner.call(req).await,
                },
                None => inner.call(req).await,
            }
        })
    }
}

#[derive(Clone)]
pub struct FirstRunLayer;

impl<S> Layer<S> for FirstRunLayer {
    type Service = FirstRunGuard<S>;

    fn layer(&self, inner: S) -> Self::Service {
        FirstRunGuard { inner }
    }
}
