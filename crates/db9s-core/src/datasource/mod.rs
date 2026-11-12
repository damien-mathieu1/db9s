pub mod amqp;
pub mod mongo;
pub mod mysql;
pub mod postgres;
pub mod rabbitmq;
pub mod redis;
pub mod sql;
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

    /// Raw query: SQL for postgres, a mongo shell-ish JSON filter `{"find": ...}` not required —
    /// for mongo this runs an aggregation pipeline given as a JSON array string, or a plain
    /// filter document JSON for a `find`.
    async fn run_query(&self, database: &str, container: &str, raw: &str) -> Result<QueryResult>;

    async fn delete_row(&self, database: &str, container: &str, row: &Row) -> Result<()>;

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

    async fn update_row(&self, _: &str, _: &str, _: &Row, _: &Row) -> Result<()> {
        Err(anyhow!("read-only connection: update refused"))
    }
}
