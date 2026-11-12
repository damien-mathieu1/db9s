pub mod amqp;
pub mod mongo;
pub mod mysql;
pub mod postgres;
pub mod rabbitmq;
pub mod redis;
pub mod sql;
pub mod sqlite;
#[cfg(test)]
mod tests;

use crate::value::Row;
use anyhow::{anyhow, Result};
use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Row>,
    pub affected: Option<u64>,
}

/// Server-side order for a page of rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sort {
    pub column: String,
    pub desc: bool,
}

/// Uniform interface over a database backend, browsable k9s-style:
/// database -> container (table/collection) -> rows/documents.
#[async_trait]
pub trait DataSource: Send + Sync {
    /// Message brokers (rows = messages): `/` publishes, `d` purges, edit is disabled.
    fn is_queue(&self) -> bool {
        false
    }

    /// Whether `run_query` is sure not to write `raw`. Read-only connections refuse
    /// everything else, so the default is `false`: a new backend starts out refusing.
    fn is_read_query(&self, _raw: &str) -> bool {
        false
    }

    async fn list_databases(&self) -> Result<Vec<String>>;

    /// Tables for postgres, collections for mongo.
    async fn list_containers(&self, database: &str) -> Result<Vec<String>>;

    async fn fetch_rows(
        &self,
        database: &str,
        container: &str,
        limit: i64,
        offset: i64,
    ) -> Result<QueryResult>;

    /// Whether `fetch_sorted` really sorts; queues and key stores have no column order.
    fn can_sort(&self) -> bool {
        false
    }

    /// `fetch_rows` ordered by `sort`; backends that can't sort return the plain page.
    async fn fetch_sorted(
        &self,
        database: &str,
        container: &str,
        limit: i64,
        offset: i64,
        _sort: &Sort,
    ) -> Result<QueryResult> {
        self.fetch_rows(database, container, limit, offset).await
    }

    /// Rows in the container, when the backend can count them cheaply; None hides paging.
    async fn count_rows(&self, _database: &str, _container: &str) -> Result<Option<u64>> {
        Ok(None)
    }

    /// Raw query: SQL for postgres, a mongo shell-ish JSON filter `{"find": ...}` not required —
    /// for mongo this runs an aggregation pipeline given as a JSON array string, or a plain
    /// filter document JSON for a `find`.
    async fn run_query(&self, database: &str, container: &str, raw: &str) -> Result<QueryResult>;

    async fn delete_row(&self, database: &str, container: &str, row: &Row) -> Result<()>;

    /// The statement a delete of `row` would run, for the confirm prompt; None when the
    /// backend has nothing clearer to show than "delete this row?".
    async fn preview_delete(
        &self,
        _database: &str,
        _container: &str,
        _row: &Row,
    ) -> Result<Option<String>> {
        Ok(None)
    }

    /// Same for an update from `original` to `edited`.
    async fn preview_update(
        &self,
        _database: &str,
        _container: &str,
        _original: &Row,
        _edited: &Row,
    ) -> Result<Option<String>> {
        Ok(None)
    }

    /// Adds `row`; fields left out get the database's defaults (serial ids, `_id`...).
    async fn insert_row(&self, _database: &str, _container: &str, _row: &Row) -> Result<()> {
        Err(anyhow!("this backend can't insert rows"))
    }

    /// The container's columns: one row per column (name, type, nullable, default, key),
    /// or per field seen in a sample for schemaless stores.
    async fn describe(&self, _database: &str, _container: &str) -> Result<QueryResult> {
        Err(anyhow!("this backend has no schema view"))
    }

    async fn update_row(
        &self,
        database: &str,
        container: &str,
        original: &Row,
        edited: &Row,
    ) -> Result<()>;
}

/// Read-only connection: wraps a source so nothing can write through it, whichever
/// part of the UI is asking.
pub struct ReadOnly(pub Box<dyn DataSource>);

#[async_trait]
impl DataSource for ReadOnly {
    fn is_queue(&self) -> bool {
        self.0.is_queue()
    }

    fn is_read_query(&self, raw: &str) -> bool {
        self.0.is_read_query(raw)
    }

    async fn list_databases(&self) -> Result<Vec<String>> {
        self.0.list_databases().await
    }

    async fn list_containers(&self, database: &str) -> Result<Vec<String>> {
        self.0.list_containers(database).await
    }

    async fn fetch_rows(
        &self,
        database: &str,
        container: &str,
        limit: i64,
        offset: i64,
    ) -> Result<QueryResult> {
        self.0.fetch_rows(database, container, limit, offset).await
    }

    fn can_sort(&self) -> bool {
        self.0.can_sort()
    }

    async fn fetch_sorted(
        &self,
        database: &str,
        container: &str,
        limit: i64,
        offset: i64,
        sort: &Sort,
    ) -> Result<QueryResult> {
        self.0
            .fetch_sorted(database, container, limit, offset, sort)
            .await
    }

    async fn count_rows(&self, database: &str, container: &str) -> Result<Option<u64>> {
        self.0.count_rows(database, container).await
    }

    async fn run_query(&self, database: &str, container: &str, raw: &str) -> Result<QueryResult> {
        if !self.0.is_read_query(raw) {
            return Err(anyhow!(
                "read-only connection: only read queries are allowed"
            ));
        }
        self.0.run_query(database, container, raw).await
    }

    async fn delete_row(&self, _: &str, _: &str, _: &Row) -> Result<()> {
        Err(anyhow!("read-only connection: delete refused"))
    }

    async fn insert_row(&self, _: &str, _: &str, _: &Row) -> Result<()> {
        Err(anyhow!("read-only connection: insert refused"))
    }

    async fn preview_delete(
        &self,
        database: &str,
        container: &str,
        row: &Row,
    ) -> Result<Option<String>> {
        self.0.preview_delete(database, container, row).await
    }

    async fn preview_update(
        &self,
        database: &str,
        container: &str,
        original: &Row,
        edited: &Row,
    ) -> Result<Option<String>> {
        self.0
            .preview_update(database, container, original, edited)
            .await
    }

    async fn describe(&self, database: &str, container: &str) -> Result<QueryResult> {
        self.0.describe(database, container).await
    }

    async fn update_row(&self, _: &str, _: &str, _: &Row, _: &Row) -> Result<()> {
        Err(anyhow!("read-only connection: update refused"))
    }
}
