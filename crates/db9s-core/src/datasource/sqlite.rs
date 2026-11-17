//! SQLite dialect for the generic SQL engine: databases = attached schemas (`main`,
//! plus any `ATTACH`ed), containers = tables. The url is `sqlite://path/to/file.db`.
use super::sql::{ConnOptions, SqlDialect, SqlSource};
use crate::value::{sqlite_row_to_json, Row};

pub struct SqliteDialect;
pub type SqliteSource = SqlSource<SqliteDialect>;

impl SqlDialect for SqliteDialect {
    type DB = sqlx::Sqlite;
    const LIST_DATABASES: &'static str = "SELECT name FROM pragma_database_list ORDER BY seq";
    const LIST_TABLES: &'static str = "SELECT name FROM pragma_table_list \
         WHERE schema = ?1 AND type = 'table' AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' \
         ORDER BY 1";
    const PRIMARY_KEY: &'static str =
        "SELECT name FROM pragma_table_info(?2, ?1) WHERE pk > 0 ORDER BY pk";
    const READ_ONLY_SESSION: &'static str = "PRAGMA query_only = ON";
    const DESCRIBE: &'static str = "SELECT name AS \"column\", type AS \"type\", \
         CASE \"notnull\" WHEN 1 THEN 'NO' ELSE 'YES' END AS nullable, \
         dflt_value AS \"default\" FROM pragma_table_info(?2, ?1) ORDER BY cid";

    fn quote(ident: &str) -> String {
        format!("\"{}\"", ident.replace('"', "\"\""))
    }

    fn placeholder(n: usize) -> String {
        format!("?{n}")
    }

    fn row_to_json(row: &sqlx::sqlite::SqliteRow) -> Row {
        sqlite_row_to_json(row)
    }

    fn rows_affected(res: &sqlx::sqlite::SqliteQueryResult) -> u64 {
        res.rows_affected()
    }

    /// One file holds every schema: nothing to switch.
    fn with_default_db(opts: ConnOptions<sqlx::Sqlite>, _db: &str) -> ConnOptions<sqlx::Sqlite> {
        opts
    }
}
