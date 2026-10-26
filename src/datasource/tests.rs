//! Backend conformance tests: the same scenario runs against every supported
//! database through the `DataSource` trait, so all backends behave the same in the UI.
//!
//! Each backend is skipped unless its URL env var is set, e.g. with the dev stack:
//!   DB9S_TEST_POSTGRES_URL=postgres://db9s:db9s@localhost:55432/db9s \
//!   DB9S_TEST_MONGO_URL=mongodb://localhost:57017 cargo test
use super::{mongo::MongoSource, postgres::PostgresSource, DataSource};
use serde_json::json;

fn unique(prefix: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    format!("{prefix}_{}_{nanos}", std::process::id())
}

/// Expects `container` in `db` to hold exactly two rows named "a" and "b".
async fn exercise(source: &dyn DataSource, db: &str, container: &str, query_for_z: &str) {
    assert!(source
        .list_databases()
        .await
        .unwrap()
        .iter()
        .any(|d| d == db));
    assert!(source
        .list_containers(db)
        .await
        .unwrap()
        .iter()
        .any(|c| c == container));

    let res = source.fetch_rows(db, container, 10, 0).await.unwrap();
    assert_eq!(res.rows.len(), 2);
    assert!(res.columns.iter().any(|c| c == "name"));
    let paged = source.fetch_rows(db, container, 1, 1).await.unwrap();
    assert_eq!(paged.rows.len(), 1);

    let row_a = res.rows.iter().find(|r| r["name"] == json!("a")).unwrap();
    let mut edited = row_a.clone();
    edited.insert("name".into(), json!("z"));
    source
        .update_row(db, container, row_a, &edited)
        .await
        .unwrap();

    let found = source.run_query(db, container, query_for_z).await.unwrap();
    assert_eq!(found.rows.len(), 1);
    assert_eq!(found.rows[0]["name"], json!("z"));

    let res = source.fetch_rows(db, container, 10, 0).await.unwrap();
    let row_b = res.rows.iter().find(|r| r["name"] == json!("b")).unwrap();
    source.delete_row(db, container, row_b).await.unwrap();

    let res = source.fetch_rows(db, container, 10, 0).await.unwrap();
    assert_eq!(res.rows.len(), 1);
    assert_eq!(res.rows[0]["name"], json!("z"));
}

#[tokio::test]
async fn postgres_conformance() {
    let Ok(url) = std::env::var("DB9S_TEST_POSTGRES_URL") else {
        eprintln!("skipped: DB9S_TEST_POSTGRES_URL not set");
        return;
    };
    let table = unique("db9s_test");
    let pool = sqlx::PgPool::connect(&url).await.unwrap();
    sqlx::query(&format!(
        "CREATE TABLE public.{table} (id serial PRIMARY KEY, name text NOT NULL)"
    ))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "INSERT INTO public.{table} (name) VALUES ('a'), ('b')"
    ))
    .execute(&pool)
    .await
    .unwrap();

    let source = PostgresSource::connect(&url).await.unwrap();
    let query = format!("SELECT * FROM public.{table} WHERE name = 'z'");
    exercise(&source, "public", &table, &query).await;

    sqlx::query(&format!("DROP TABLE public.{table}"))
        .execute(&pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn mongo_conformance() {
    let Ok(url) = std::env::var("DB9S_TEST_MONGO_URL") else {
        eprintln!("skipped: DB9S_TEST_MONGO_URL not set");
        return;
    };
    let db = "db9s_test";
    let coll_name = unique("items");
    let client = mongodb::Client::with_uri_str(&url).await.unwrap();
    let coll = client
        .database(db)
        .collection::<mongodb::bson::Document>(&coll_name);
    coll.insert_many([
        mongodb::bson::doc! { "name": "a" },
        mongodb::bson::doc! { "name": "b" },
    ])
    .await
    .unwrap();

    let source = MongoSource::connect(&url).await.unwrap();
    exercise(&source, db, &coll_name, r#"{"name": "z"}"#).await;

    coll.drop().await.unwrap();
}
