//! One generic engine for every SQL database (Postgres, MySQL, ...): listing, paging,
//! raw queries, and edit/delete by the table's real primary key. A new SQL backend only
//! implements `SqlDialect`: identifier quoting, placeholders, catalog queries, row decoding.
use super::{DataSource, QueryResult};
use crate::value::Row;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::Value;
use sqlx::{Column, Connection, Database, Pool, Row as _};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Mutex;

pub type ConnOptions<DB> = <<DB as Database>::Connection as Connection>::Options;

pub trait SqlDialect: Send + Sync + 'static {
    type DB: Database;
    /// Lists browsable schemas/databases, one text column.
    const LIST_DATABASES: &'static str;
    /// Lists tables of the schema bound as the single parameter.
    const LIST_TABLES: &'static str;
    /// Primary key columns in key order, for (schema, table) bound as parameters.
    const PRIMARY_KEY: &'static str;

    fn quote(ident: &str) -> String;
    /// Bind marker for the n-th parameter (1-based), e.g. `$1` or `?`.
    fn placeholder(n: usize) -> String;
    fn row_to_json(row: &<Self::DB as Database>::Row) -> Row;
    fn rows_affected(res: &<Self::DB as Database>::QueryResult) -> u64;
    /// Connection options whose unqualified names resolve inside `db` (raw queries).
    fn with_default_db(opts: ConnOptions<Self::DB>, db: &str) -> ConnOptions<Self::DB>;
}

pub struct SqlSource<D: SqlDialect> {
    pool: Pool<D::DB>,
    options: ConnOptions<D::DB>,
    /// One small pool per database for raw queries, so `select * from users` works
    /// in whatever database is being browsed without per-connection state.
    db_pools: Mutex<HashMap<String, Pool<D::DB>>>,
}

impl<D: SqlDialect> SqlSource<D>
where
    for<'q> <D::DB as Database>::Arguments<'q>: sqlx::IntoArguments<'q, D::DB>,
    for<'c> &'c mut <D::DB as Database>::Connection: sqlx::Executor<'c, Database = D::DB>,
    for<'q> String: sqlx::Encode<'q, D::DB> + sqlx::Type<D::DB>,
    for<'q> i64: sqlx::Encode<'q, D::DB> + sqlx::Type<D::DB>,
    for<'q> f64: sqlx::Encode<'q, D::DB> + sqlx::Type<D::DB>,
    for<'q> bool: sqlx::Encode<'q, D::DB> + sqlx::Type<D::DB>,
    for<'q> Option<String>: sqlx::Encode<'q, D::DB> + sqlx::Type<D::DB>,
    for<'r> String: sqlx::Decode<'r, D::DB>,
    usize: sqlx::ColumnIndex<<D::DB as Database>::Row>,
{
    pub async fn connect(url: &str) -> Result<Self> {
        let options = ConnOptions::<D::DB>::from_str(url)?;
        let pool = sqlx::pool::PoolOptions::<D::DB>::new()
            .max_connections(5)
            .connect_with(options.clone())
            .await?;
        Ok(Self {
            pool,
            options,
            db_pools: Mutex::new(HashMap::new()),
        })
    }

    fn table(database: &str, table: &str) -> String {
        format!("{}.{}", D::quote(database), D::quote(table))
    }

    async fn names(&self, sql: &str, params: &[&str]) -> Result<Vec<String>> {
        let mut q = sqlx::query::<D::DB>(sql);
        for p in params {
            q = q.bind(p.to_string());
        }
        let rows = q.fetch_all(&self.pool).await?;
        Ok(rows.iter().map(|r| r.get::<String, _>(0usize)).collect())
    }

    fn to_result(rows: &[<D::DB as Database>::Row]) -> QueryResult {
        let columns = rows
            .first()
            .map(|r| r.columns().iter().map(|c| c.name().to_string()).collect())
            .unwrap_or_default();
        QueryResult {
            columns,
            rows: rows.iter().map(D::row_to_json).collect(),
            affected: None,
        }
    }

    async fn db_pool(&self, database: &str) -> Result<Pool<D::DB>> {
        if let Some(pool) = self.db_pools.lock().unwrap().get(database) {
            return Ok(pool.clone());
        }
        let pool = sqlx::pool::PoolOptions::<D::DB>::new()
            .max_connections(2)
            .connect_with(D::with_default_db(self.options.clone(), database))
            .await?;
        self.db_pools
            .lock()
            .unwrap()
            .insert(database.to_string(), pool.clone());
        Ok(pool)
    }

    /// `WHERE pk1 = ? AND pk2 = ?` for `row`, numbering placeholders from `first`.
    async fn where_pk(
        &self,
        database: &str,
        table: &str,
        row: &Row,
        first: usize,
    ) -> Result<(String, Vec<Value>)> {
        let pk = self.names(D::PRIMARY_KEY, &[database, table]).await?;
        if pk.is_empty() {
            return Err(anyhow!(
                "table {table} has no primary key; edit/delete not supported"
            ));
        }
        let mut values = vec![];
        let mut clauses = vec![];
        for (i, col) in pk.iter().enumerate() {
            values.push(
                row.get(col)
                    .cloned()
                    .ok_or_else(|| anyhow!("row is missing primary key column {col}"))?,
            );
            clauses.push(format!("{} = {}", D::quote(col), D::placeholder(first + i)));
        }
        Ok((clauses.join(" AND "), values))
    }

    async fn execute(&self, sql: &str, values: &[Value]) -> Result<()> {
        let mut q = sqlx::query::<D::DB>(sql);
        for v in values {
            q = match v {
                Value::String(s) => q.bind(s.clone()),
                Value::Number(n) if n.is_i64() => q.bind(n.as_i64().unwrap()),
                Value::Number(n) => q.bind(n.as_f64().unwrap_or_default()),
                Value::Bool(b) => q.bind(*b),
                Value::Null => q.bind(Option::<String>::None),
                other => q.bind(other.to_string()),
            };
        }
        q.execute(&self.pool).await?;
        Ok(())
    }
}

fn returns_rows(raw: &str) -> bool {
    let first = raw
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_lowercase();
    [
        "select", "with", "show", "desc", "describe", "explain", "values", "table",
    ]
    .contains(&first.as_str())
}

#[async_trait]
impl<D: SqlDialect> DataSource for SqlSource<D>
where
    for<'q> <D::DB as Database>::Arguments<'q>: sqlx::IntoArguments<'q, D::DB>,
    for<'c> &'c mut <D::DB as Database>::Connection: sqlx::Executor<'c, Database = D::DB>,
    for<'q> String: sqlx::Encode<'q, D::DB> + sqlx::Type<D::DB>,
    for<'q> i64: sqlx::Encode<'q, D::DB> + sqlx::Type<D::DB>,
    for<'q> f64: sqlx::Encode<'q, D::DB> + sqlx::Type<D::DB>,
    for<'q> bool: sqlx::Encode<'q, D::DB> + sqlx::Type<D::DB>,
    for<'q> Option<String>: sqlx::Encode<'q, D::DB> + sqlx::Type<D::DB>,
    for<'r> String: sqlx::Decode<'r, D::DB>,
    usize: sqlx::ColumnIndex<<D::DB as Database>::Row>,
{
    async fn list_databases(&self) -> Result<Vec<String>> {
        self.names(D::LIST_DATABASES, &[]).await
    }

    async fn list_containers(&self, database: &str) -> Result<Vec<String>> {
        self.names(D::LIST_TABLES, &[database]).await
    }

    async fn fetch_rows(
        &self,
        database: &str,
        table: &str,
        limit: i64,
        offset: i64,
    ) -> Result<QueryResult> {
        let sql = format!(
            "SELECT * FROM {} LIMIT {limit} OFFSET {offset}",
            Self::table(database, table)
        );
        let rows = sqlx::query::<D::DB>(&sql).fetch_all(&self.pool).await?;
        Ok(Self::to_result(&rows))
    }

    async fn run_query(&self, database: &str, _table: &str, raw: &str) -> Result<QueryResult> {
        let pool = self.db_pool(database).await?;
        if returns_rows(raw) {
            let rows = sqlx::query::<D::DB>(raw).fetch_all(&pool).await?;
            Ok(Self::to_result(&rows))
        } else {
            let res = sqlx::query::<D::DB>(raw).execute(&pool).await?;
            Ok(QueryResult {
                columns: vec![],
                rows: vec![],
                affected: Some(D::rows_affected(&res)),
            })
        }
    }

    async fn delete_row(&self, database: &str, table: &str, row: &Row) -> Result<()> {
        let (clause, values) = self.where_pk(database, table, row, 1).await?;
        let sql = format!(
            "DELETE FROM {} WHERE {clause}",
            Self::table(database, table)
        );
        self.execute(&sql, &values).await
    }

    async fn update_row(
        &self,
        database: &str,
        table: &str,
        original: &Row,
        edited: &Row,
    ) -> Result<()> {
        let changed: Vec<(&String, &Value)> = edited
            .iter()
            .filter(|(k, v)| original.get(*k) != Some(*v))
            .collect();
        if changed.is_empty() {
            return Ok(());
        }
        let set = changed
            .iter()
            .enumerate()
            .map(|(i, (k, _))| format!("{} = {}", D::quote(k), D::placeholder(i + 1)))
            .collect::<Vec<_>>()
            .join(", ");
        let (clause, key_values) = self
            .where_pk(database, table, original, changed.len() + 1)
            .await?;
        let sql = format!(
            "UPDATE {} SET {set} WHERE {clause}",
            Self::table(database, table)
        );
        let mut values: Vec<Value> = changed.into_iter().map(|(_, v)| v.clone()).collect();
        values.extend(key_values);
        self.execute(&sql, &values).await
    }
}

#[cfg(test)]
mod tests {
    use super::returns_rows;

    #[test]
    fn detects_queries_that_return_rows() {
        assert!(returns_rows("  SELECT * from t"));
        assert!(returns_rows("with x as (select 1) select * from x"));
        assert!(returns_rows("show tables"));
        assert!(!returns_rows("update t set a = 1"));
        assert!(!returns_rows(""));
    }
}
