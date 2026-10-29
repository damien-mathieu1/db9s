//! MySQL / MariaDB dialect for the generic SQL engine: databases = schemas, containers = tables.
use super::sql::{ConnOptions, SqlDialect, SqlSource};
use crate::value::{mysql_row_to_json, Row};

pub struct MySqlDialect;
pub type MySqlSource = SqlSource<MySqlDialect>;

// CAST(... AS CHAR): MySQL 8 returns information_schema names as binary strings
impl SqlDialect for MySqlDialect {
    type DB = sqlx::MySql;
    const LIST_DATABASES: &'static str = "SELECT CAST(schema_name AS CHAR) FROM information_schema.schemata \
         WHERE schema_name NOT IN ('information_schema','mysql','performance_schema','sys') ORDER BY 1";
    const LIST_TABLES: &'static str =
        "SELECT CAST(table_name AS CHAR) FROM information_schema.tables \
         WHERE table_schema = ? ORDER BY 1";
    const PRIMARY_KEY: &'static str =
        "SELECT CAST(column_name AS CHAR) FROM information_schema.key_column_usage \
         WHERE table_schema = ? AND table_name = ? AND constraint_name = 'PRIMARY' \
         ORDER BY ordinal_position";

    fn quote(ident: &str) -> String {
        format!("`{}`", ident.replace('`', "``"))
    }

    fn placeholder(_n: usize) -> String {
        "?".into()
    }

    fn row_to_json(row: &sqlx::mysql::MySqlRow) -> Row {
        mysql_row_to_json(row)
    }

    fn rows_affected(res: &sqlx::mysql::MySqlQueryResult) -> u64 {
        res.rows_affected()
    }

    fn with_default_db(opts: ConnOptions<sqlx::MySql>, db: &str) -> ConnOptions<sqlx::MySql> {
        opts.database(db)
    }
}

#[cfg(test)]
mod tests {
    use super::{MySqlDialect, SqlDialect};
    use crate::datasource::postgres::PgDialect;

    #[test]
    fn quotes_identifiers_per_dialect() {
        assert_eq!(MySqlDialect::quote("we`ird"), "`we``ird`");
        assert_eq!(PgDialect::quote("we\"ird"), "\"we\"\"ird\"");
        assert_eq!(MySqlDialect::placeholder(3), "?");
        assert_eq!(PgDialect::placeholder(3), "$3");
    }
}
