//! One generic engine for every SQL database (Postgres, MySQL, ...): listing, paging,
//! raw queries, and edit/delete by the table's real primary key. A new SQL backend only
//! implements `SqlDialect`: identifier quoting, placeholders, catalog queries, row decoding.
use super::{DataSource, QueryResult, Sort};
use crate::value::Row;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::Value;
use sqlx::{Column, Connection, Database, Pool, Row as _};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Mutex;

pub type ConnOptions<DB> = <<DB as Database>::Connection as Connection>::Options;

/// Writes the n-th (1-based) value's spot in a statement: a bind placeholder, or a literal.
type Mark<'a> = &'a (dyn Fn(usize, &Value) -> String + Sync);

/// A value as an SQL literal, to show a statement (never to run one: those bind values).
fn literal(v: &Value) -> String {
    match v {
        Value::Null => "NULL".into(),
        Value::Bool(b) => b.to_string().to_uppercase(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => format!("'{}'", s.replace('\'', "''")),
        other => format!("'{}'", other.to_string().replace('\'', "''")),
    }
}

pub trait SqlDialect: Send + Sync + 'static {
    type DB: Database;
    /// Lists browsable schemas/databases, one text column.
    const LIST_DATABASES: &'static str;
    /// Lists tables of the schema bound as the single parameter.
    const LIST_TABLES: &'static str;
    /// Primary key columns in key order, for (schema, table) bound as parameters.
    const PRIMARY_KEY: &'static str;
    /// Makes the session reject writes, run on each connection of a read-only source.
    const READ_ONLY_SESSION: &'static str;
    /// Columns of (schema, table) bound as parameters, in order, as text columns
    /// `column`, `type`, `nullable`, `default`.
    const DESCRIBE: &'static str;

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
    read_only: bool,
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
    for<'r> i64: sqlx::Decode<'r, D::DB>,
    usize: sqlx::ColumnIndex<<D::DB as Database>::Row>,
{
    pub async fn connect(url: &str, read_only: bool) -> Result<Self> {
        let options = ConnOptions::<D::DB>::from_str(url)?;
        let pool = Self::pool_options(5, read_only)
            .connect_with(options.clone())
            .await?;
        Ok(Self {
            pool,
            options,
            read_only,
            db_pools: Mutex::new(HashMap::new()),
        })
    }

    fn pool_options(max: u32, read_only: bool) -> sqlx::pool::PoolOptions<D::DB> {
        let options = sqlx::pool::PoolOptions::<D::DB>::new().max_connections(max);
        if !read_only {
            return options;
        }
        options.after_connect(|conn, _| {
            Box::pin(async move {
                sqlx::query::<D::DB>(D::READ_ONLY_SESSION)
                    .execute(&mut *conn)
                    .await?;
                Ok(())
            })
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
        let pool = Self::pool_options(2, self.read_only)
            .connect_with(D::with_default_db(self.options.clone(), database))
            .await?;
        self.db_pools
            .lock()
            .unwrap()
            .insert(database.to_string(), pool.clone());
        Ok(pool)
    }

    /// `WHERE pk1 = ? AND pk2 = ?` for `row`, numbering placeholders from `first`;
    /// `mark` writes each value's spot: a placeholder to run, a literal to show.
    async fn where_pk(
        &self,
        database: &str,
        table: &str,
        row: &Row,
        first: usize,
        mark: Mark<'_>,
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
            clauses.push(format!(
                "{} = {}",
                D::quote(col),
                mark(first + i, &row[col])
            ));
        }
        Ok((clauses.join(" AND "), values))
    }

    async fn page(
        &self,
        database: &str,
        table: &str,
        limit: i64,
        offset: i64,
        sort: Option<&Sort>,
    ) -> Result<QueryResult> {
        let order = sort.map_or(String::new(), |s| {
            let dir = if s.desc { "DESC" } else { "ASC" };
            format!(" ORDER BY {} {dir}", D::quote(&s.column))
        });
        let sql = format!(
            "SELECT * FROM {}{order} LIMIT {limit} OFFSET {offset}",
            Self::table(database, table)
        );
        let rows = sqlx::query::<D::DB>(&sql).fetch_all(&self.pool).await?;
        Ok(Self::to_result(&rows))
    }

    async fn delete_sql(
        &self,
        database: &str,
        table: &str,
        row: &Row,
        mark: Mark<'_>,
    ) -> Result<(String, Vec<Value>)> {
        let (clause, values) = self.where_pk(database, table, row, 1, mark).await?;
        let sql = format!(
            "DELETE FROM {} WHERE {clause}",
            Self::table(database, table)
        );
        Ok((sql, values))
    }

    /// The UPDATE for the fields that changed, or None if nothing did.
    async fn update_sql(
        &self,
        database: &str,
        table: &str,
        original: &Row,
        edited: &Row,
        mark: Mark<'_>,
    ) -> Result<Option<(String, Vec<Value>)>> {
        let changed: Vec<(&String, &Value)> = edited
            .iter()
            .filter(|(k, v)| original.get(*k) != Some(*v))
            .collect();
        if changed.is_empty() {
            return Ok(None);
        }
        let set = changed
            .iter()
            .enumerate()
            .map(|(i, (k, v))| format!("{} = {}", D::quote(k), mark(i + 1, v)))
            .collect::<Vec<_>>()
            .join(", ");
        let (clause, key_values) = self
            .where_pk(database, table, original, changed.len() + 1, mark)
            .await?;
        let sql = format!(
            "UPDATE {} SET {set} WHERE {clause}",
            Self::table(database, table)
        );
        let mut values: Vec<Value> = changed.into_iter().map(|(_, v)| v.clone()).collect();
        values.extend(key_values);
        Ok(Some((sql, values)))
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

/// One statement that only reads. The server's read-only session catches what this
/// can't see, like `WITH x AS (DELETE ...) SELECT`, but a later statement could turn
/// that session off, hence the single statement.
// shortcut: refuses a `;` inside a string literal too; parse statements if that gets in the way
fn is_read_sql(raw: &str) -> bool {
    returns_rows(raw) && !raw.trim_end().trim_end_matches(';').contains(';')
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
    for<'r> i64: sqlx::Decode<'r, D::DB>,
    usize: sqlx::ColumnIndex<<D::DB as Database>::Row>,
{
    fn is_read_query(&self, raw: &str) -> bool {
        is_read_sql(raw)
    }

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
        self.page(database, table, limit, offset, None).await
    }

    fn can_sort(&self) -> bool {
        true
    }

    async fn fetch_sorted(
        &self,
        database: &str,
        table: &str,
        limit: i64,
        offset: i64,
        sort: &Sort,
    ) -> Result<QueryResult> {
        self.page(database, table, limit, offset, Some(sort)).await
    }

    async fn count_rows(&self, database: &str, table: &str) -> Result<Option<u64>> {
        let sql = format!("SELECT COUNT(*) FROM {}", Self::table(database, table));
        let row = sqlx::query::<D::DB>(&sql).fetch_one(&self.pool).await?;
        Ok(Some(row.try_get::<i64, _>(0usize)? as u64))
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
        let (sql, values) = self
            .delete_sql(database, table, row, &|n, _| D::placeholder(n))
            .await?;
        self.execute(&sql, &values).await
    }

    async fn preview_delete(
        &self,
        database: &str,
        table: &str,
        row: &Row,
    ) -> Result<Option<String>> {
        let (sql, _) = self
            .delete_sql(database, table, row, &|_, v| literal(v))
            .await?;
        Ok(Some(sql))
    }

    async fn preview_update(
        &self,
        database: &str,
        table: &str,
        original: &Row,
        edited: &Row,
    ) -> Result<Option<String>> {
        let sql = self
            .update_sql(database, table, original, edited, &|_, v| literal(v))
            .await?;
        Ok(sql.map(|(sql, _)| sql))
    }

    async fn insert_row(&self, database: &str, table: &str, row: &Row) -> Result<()> {
        if row.is_empty() {
            return Err(anyhow!("set at least one field to insert a row"));
        }
        let cols = row
            .keys()
            .map(|k| D::quote(k))
            .collect::<Vec<_>>()
            .join(", ");
        let marks = (1..=row.len())
            .map(D::placeholder)
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "INSERT INTO {} ({cols}) VALUES ({marks})",
            Self::table(database, table)
        );
        let values: Vec<Value> = row.values().cloned().collect();
        self.execute(&sql, &values).await
    }

    async fn describe(&self, database: &str, table: &str) -> Result<QueryResult> {
        let mut q = sqlx::query::<D::DB>(D::DESCRIBE);
        q = q.bind(database.to_string()).bind(table.to_string());
        let mut res = Self::to_result(&q.fetch_all(&self.pool).await?);
        let pk = self.names(D::PRIMARY_KEY, &[database, table]).await?;
        for row in &mut res.rows {
            let is_pk = row
                .get("column")
                .and_then(Value::as_str)
                .is_some_and(|c| pk.iter().any(|k| k == c));
            row.insert(
                "key".into(),
                Value::String(if is_pk { "PK" } else { "" }.into()),
            );
        }
        res.columns.push("key".into());
        Ok(res)
    }

    async fn update_row(
        &self,
        database: &str,
        table: &str,
        original: &Row,
        edited: &Row,
    ) -> Result<()> {
        match self
            .update_sql(database, table, original, edited, &|n, _| D::placeholder(n))
            .await?
        {
            Some((sql, values)) => self.execute(&sql, &values).await,
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{is_read_sql, literal, returns_rows};
    use serde_json::json;

    #[test]
    fn literals_for_shown_statements() {
        assert_eq!(literal(&json!("O'Brien")), "'O''Brien'");
        assert_eq!(literal(&json!(null)), "NULL");
        assert_eq!(literal(&json!(4.5)), "4.5");
        assert_eq!(literal(&json!(true)), "TRUE");
        assert_eq!(literal(&json!({"a": "b'c"})), "'{\"a\":\"b''c\"}'");
    }

    #[test]
    fn detects_queries_that_return_rows() {
        assert!(returns_rows("  SELECT * from t"));
        assert!(returns_rows("with x as (select 1) select * from x"));
        assert!(returns_rows("show tables"));
        assert!(!returns_rows("update t set a = 1"));
        assert!(!returns_rows(""));
    }

    #[test]
    fn read_sql_is_one_read_statement() {
        assert!(is_read_sql("select * from t"));
        assert!(is_read_sql("  SELECT 1 ;  \n"));
        assert!(is_read_sql("explain select 1"));
        assert!(!is_read_sql("update t set a = 1"));
        assert!(!is_read_sql("delete from t"));
        assert!(!is_read_sql("drop table t"));
        assert!(!is_read_sql("set default_transaction_read_only = off"));
        assert!(!is_read_sql("select 1; delete from t"));
        assert!(!is_read_sql("select 1; commit; delete from t;"));
        assert!(!is_read_sql(""));
    }
}
