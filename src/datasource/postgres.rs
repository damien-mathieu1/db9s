//! Postgres dialect for the generic SQL engine: databases = schemas, containers = tables.
use super::sql::{ConnOptions, SqlDialect, SqlSource};
use crate::value::{pg_row_to_json, Row};

pub struct PgDialect;
pub type PostgresSource = SqlSource<PgDialect>;

impl SqlDialect for PgDialect {
    type DB = sqlx::Postgres;
    const LIST_DATABASES: &'static str =
        "SELECT schema_name::text FROM information_schema.schemata \
         WHERE schema_name NOT IN ('pg_catalog','information_schema') \
         AND schema_name NOT LIKE 'pg\\_toast%' AND schema_name NOT LIKE 'pg\\_temp%' ORDER BY 1";
    const LIST_TABLES: &'static str = "SELECT table_name::text FROM information_schema.tables \
         WHERE table_schema = $1 ORDER BY 1";
    const PRIMARY_KEY: &'static str = "SELECT kcu.column_name::text \
         FROM information_schema.table_constraints tc \
         JOIN information_schema.key_column_usage kcu \
           ON kcu.constraint_name = tc.constraint_name \
          AND kcu.table_schema = tc.table_schema AND kcu.table_name = tc.table_name \
         WHERE tc.constraint_type = 'PRIMARY KEY' AND tc.table_schema = $1 AND tc.table_name = $2 \
         ORDER BY kcu.ordinal_position";

    fn quote(ident: &str) -> String {
        format!("\"{}\"", ident.replace('"', "\"\""))
    }

    fn placeholder(n: usize) -> String {
        format!("${n}")
    }

    fn row_to_json(row: &sqlx::postgres::PgRow) -> Row {
        pg_row_to_json(row)
    }

    fn rows_affected(res: &sqlx::postgres::PgQueryResult) -> u64 {
        res.rows_affected()
    }

    fn with_default_db(opts: ConnOptions<sqlx::Postgres>, db: &str) -> ConnOptions<sqlx::Postgres> {
        opts.options([("search_path", db)])
    }
}
