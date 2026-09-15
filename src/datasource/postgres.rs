use super::{DataSource, QueryResult};
use crate::value::{pg_row_to_json, Row};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use sqlx::{postgres::PgPoolOptions, Column, Pool, Postgres, Row as _};

pub struct PostgresSource {
    pool: Pool<Postgres>,
}

impl PostgresSource {
    pub async fn connect(url: &str) -> Result<Self> {
        let pool = PgPoolOptions::new().max_connections(5).connect(url).await?;
        Ok(Self { pool })
    }

    fn rows_to_result(rows: Vec<sqlx::postgres::PgRow>) -> QueryResult {
        let columns = rows
            .first()
            .map(|r| r.columns().iter().map(|c| c.name().to_string()).collect())
            .unwrap_or_default();
        let rows: Vec<Row> = rows.iter().map(pg_row_to_json).collect();
        QueryResult {
            columns,
            rows,
            affected: None,
        }
    }

    fn primary_key_cols(&self) -> [&'static str; 1] {
        ["id"]
    }
}

#[async_trait]
impl DataSource for PostgresSource {
    async fn list_databases(&self) -> Result<Vec<String>> {
        let rows = sqlx::query("SELECT schema_name FROM information_schema.schemata WHERE schema_name NOT IN ('pg_catalog','information_schema') ORDER BY 1")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.iter().map(|r| r.get::<String, _>(0)).collect())
    }

    async fn list_containers(&self, database: &str) -> Result<Vec<String>> {
        let rows = sqlx::query(
            "SELECT table_name FROM information_schema.tables WHERE table_schema = $1 ORDER BY 1",
        )
        .bind(database)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(|r| r.get::<String, _>(0)).collect())
    }

    async fn fetch_rows(
        &self,
        database: &str,
        container: &str,
        limit: i64,
        offset: i64,
    ) -> Result<QueryResult> {
        let sql = format!(
            "SELECT * FROM \"{}\".\"{}\" LIMIT {} OFFSET {}",
            database, container, limit, offset
        );
        let rows = sqlx::query(&sql).fetch_all(&self.pool).await?;
        Ok(Self::rows_to_result(rows))
    }

    async fn run_query(&self, _database: &str, _container: &str, raw: &str) -> Result<QueryResult> {
        let trimmed = raw.trim_start().to_lowercase();
        if trimmed.starts_with("select")
            || trimmed.starts_with("with")
            || trimmed.starts_with("show")
        {
            let rows = sqlx::query(raw).fetch_all(&self.pool).await?;
            Ok(Self::rows_to_result(rows))
        } else {
            let result = sqlx::query(raw).execute(&self.pool).await?;
            Ok(QueryResult {
                columns: vec![],
                rows: vec![],
                affected: Some(result.rows_affected()),
            })
        }
    }

    async fn delete_row(&self, database: &str, container: &str, row: &Row) -> Result<()> {
        let pk = self.primary_key_cols();
        let key = pk.iter().find(|k| row.contains_key(**k)).ok_or_else(|| {
            anyhow!(
                "no primary key column found among {:?}; edit not supported for this table",
                pk
            )
        })?;
        let val = row.get(*key).unwrap();
        let sql = format!(
            "DELETE FROM \"{}\".\"{}\" WHERE \"{}\" = $1",
            database, container, key
        );
        bind_and_exec(&self.pool, &sql, val).await?;
        Ok(())
    }

    async fn update_row(
        &self,
        database: &str,
        container: &str,
        original: &Row,
        edited: &Row,
    ) -> Result<()> {
        let pk = self.primary_key_cols();
        let key = pk
            .iter()
            .find(|k| original.contains_key(**k))
            .ok_or_else(|| {
                anyhow!(
                    "no primary key column found among {:?}; edit not supported for this table",
                    pk
                )
            })?;
        let key_val = original.get(*key).unwrap();

        let mut set_clauses = vec![];
        let mut values = vec![];
        for (k, v) in edited {
            if k == key {
                continue;
            }
            if original.get(k) != Some(v) {
                set_clauses.push(k.clone());
                values.push(v.clone());
            }
        }
        if set_clauses.is_empty() {
            return Ok(());
        }
        let set_sql: Vec<String> = set_clauses
            .iter()
            .enumerate()
            .map(|(i, c)| format!("\"{}\" = ${}", c, i + 1))
            .collect();
        let sql = format!(
            "UPDATE \"{}\".\"{}\" SET {} WHERE \"{}\" = ${}",
            database,
            container,
            set_sql.join(", "),
            key,
            values.len() + 1
        );
        let mut q = sqlx::query(&sql);
        for v in &values {
            q = bind_json(q, v);
        }
        q = bind_json(q, key_val);
        q.execute(&self.pool).await?;
        Ok(())
    }
}

async fn bind_and_exec(pool: &Pool<Postgres>, sql: &str, val: &serde_json::Value) -> Result<()> {
    let q = bind_json(sqlx::query(sql), val);
    q.execute(pool).await?;
    Ok(())
}

fn bind_json<'a>(
    q: sqlx::query::Query<'a, Postgres, sqlx::postgres::PgArguments>,
    v: &'a serde_json::Value,
) -> sqlx::query::Query<'a, Postgres, sqlx::postgres::PgArguments> {
    match v {
        serde_json::Value::String(s) => q.bind(s.clone()),
        serde_json::Value::Number(n) if n.is_i64() => q.bind(n.as_i64()),
        serde_json::Value::Number(n) => q.bind(n.as_f64()),
        serde_json::Value::Bool(b) => q.bind(*b),
        serde_json::Value::Null => q.bind(Option::<String>::None),
        other => q.bind(other.to_string()),
    }
}
