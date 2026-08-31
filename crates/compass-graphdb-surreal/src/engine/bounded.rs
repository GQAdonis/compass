//! Every RPC has a deadline, including generation validation and publication.
use std::borrow::Cow;
use std::future::{Future, IntoFuture};
use std::pin::Pin;
use std::time::Duration;
use surrealdb::engine::any::Any;
use surrealdb::method::{IntoVariables, Query};
use surrealdb::{IndexedResults, Surreal};

#[derive(Clone)]
pub(super) struct BoundedDatabase(Surreal<Any>);

impl BoundedDatabase {
    pub(super) async fn select(
        &self,
        record: surrealdb::types::RecordId,
    ) -> surrealdb::Result<Option<serde_json::Value>> {
        tokio::time::timeout(
            Duration::from_secs(120),
            self.0.select(record).into_future(),
        )
        .await
        .map_err(|_| {
            surrealdb::Error::internal("Compass Surreal record read exceeded 120 seconds".into())
        })?
    }
    pub(super) async fn delete(
        &self,
        record: surrealdb::types::RecordId,
    ) -> surrealdb::Result<Option<serde_json::Value>> {
        tokio::time::timeout(
            Duration::from_secs(120),
            self.0.delete(record).into_future(),
        )
        .await
        .map_err(|_| {
            surrealdb::Error::internal(
                "Compass Surreal record deletion exceeded 120 seconds".into(),
            )
        })?
    }
    pub(super) fn new(client: Surreal<Any>) -> Self {
        Self(client)
    }
    pub(super) fn query<'a>(&'a self, query: impl Into<Cow<'a, str>>) -> BoundedQuery<'a> {
        BoundedQuery(self.0.query(query))
    }
}

pub(super) struct BoundedQuery<'a>(Query<'a, Any>);
impl BoundedQuery<'_> {
    pub(super) fn bind(mut self, vars: impl IntoVariables) -> Self {
        self.0 = self.0.bind(vars);
        self
    }
}
impl<'a> IntoFuture for BoundedQuery<'a> {
    type Output = surrealdb::Result<IndexedResults>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send + 'a>>;
    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            tokio::time::timeout(Duration::from_secs(120), self.0.into_future())
                .await
                .map_err(|_| {
                    surrealdb::Error::internal(
                        "Compass Surreal operation exceeded its 120-second deadline".into(),
                    )
                })?
        })
    }
}
