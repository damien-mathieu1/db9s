pub mod mongo;
pub mod postgres;

use crate::value::Row;
use anyhow::Result;
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
