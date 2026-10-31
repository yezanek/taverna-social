//! First-run guard (SPEC §3.5 "Admin Onboarding"): while the instance has no
//! local users, every web request is redirected to `/setup`.
//!
//! The state lives in the layer (one per app), not in a process global, so
//! tests that boot several apps against fresh databases do not leak state.
use axum::extract::Request;
use axum::response::{IntoResponse, Redirect, Response};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};
use tower::{Layer, Service};

use crate::models::_entities::users;

/// Paths that stay reachable before setup is complete.
const ALWAYS_ALLOWED: &[&str] = &[
    "/static/",
    "/setup",
    "/.well-known/",
    "/nodeinfo/",
    "/verify-email",
];

/// Returns true when the instance has no local users yet.
pub async fn is_first_run(db: &DatabaseConnection) -> Result<bool, sea_orm::DbErr> {
    let count = users::Entity::find()
        .filter(users::Column::IsLocal.eq(true))
        .count(db)
        .await?;
    Ok(count == 0)
}

/// Shared first-run state. Inserted into every request's extensions so the
/// setup handler can mark setup as complete.
#[derive(Clone)]
pub struct FirstRunState {
    db: DatabaseConnection,
    setup_complete: Arc<AtomicBool>,
}

impl FirstRunState {
    #[must_use]
    pub fn new(db: DatabaseConnection) -> Self {
        Self {
            db,
            setup_complete: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Records that the first admin exists, so later requests skip the query.
    pub fn mark_complete(&self) {
        self.setup_complete.store(true, Ordering::Release);
    }

    /// True while the instance still needs its first admin. Once local users
    /// exist the answer is cached for the lifetime of the app.
    async fn needs_setup(&self) -> bool {
        if self.setup_complete.load(Ordering::Acquire) {
            return false;
        }
        match is_first_run(&self.db).await {
            Ok(true) => true,
            Ok(false) => {
                self.mark_complete();
                false
            }
            Err(e) => {
                // Fail open: a database hiccup must not lock everyone out.
                tracing::warn!(error = %e, "first-run check failed");
                false
            }
        }
    }
}

#[derive(Clone)]
pub struct FirstRunLayer {
    state: FirstRunState,
}

impl FirstRunLayer {
    #[must_use]
    pub fn new(db: DatabaseConnection) -> Self {
        Self {
            state: FirstRunState::new(db),
        }
    }
}

impl<S> Layer<S> for FirstRunLayer {
    type Service = FirstRunGuard<S>;

    fn layer(&self, inner: S) -> Self::Service {
        FirstRunGuard {
            inner,
            state: self.state.clone(),
        }
    }
}

#[derive(Clone)]
pub struct FirstRunGuard<S> {
    inner: S,
    state: FirstRunState,
}

impl<S> Service<Request> for FirstRunGuard<S>
where
    S: Service<Request, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request) -> Self::Future {
        req.extensions_mut().insert(self.state.clone());

        let path = req.uri().path();
        if ALWAYS_ALLOWED.iter().any(|p| path.starts_with(p)) {
            return Box::pin(self.inner.call(req));
        }

        // Take the service that was driven to readiness and leave a fresh
        // clone in its place (the tower "clone and swap" idiom).
        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);
        let state = self.state.clone();

        Box::pin(async move {
            if state.needs_setup().await {
                Ok(Redirect::to("/setup").into_response())
            } else {
                inner.call(req).await
            }
        })
    }
}
