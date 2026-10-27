//! Backend conformance tests: the same scenario runs against every supported
//! database through the `DataSource` trait, so all backends behave the same in the UI.
//!
//! Each backend is skipped unless its URL env var is set, e.g. with the dev stack:
//!   DB9S_TEST_POSTGRES_URL=postgres://db9s:db9s@localhost:55432/db9s \
//!   DB9S_TEST_MONGO_URL=mongodb://localhost:57017 \
//!   DB9S_TEST_RABBITMQ_URL=http://db9s:db9s@localhost:55672 cargo test
use super::{mongo::MongoSource, postgres::PostgresSource, rabbitmq::RabbitMqSource, DataSource};
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

/// Queues don't fit the row scenario (no per-message edit/delete), so RabbitMQ gets
/// its own: publish, peek without consuming, paginate, read-only edit, purge.
#[tokio::test]
async fn rabbitmq_conformance() {
    let Ok(url) = std::env::var("DB9S_TEST_RABBITMQ_URL") else {
        eprintln!("skipped: DB9S_TEST_RABBITMQ_URL not set");
        return;
    };
    // the management API comes up a few seconds after the broker
    let mut source = RabbitMqSource::connect(&url).await;
    for _ in 0..15 {
        if source.is_ok() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        source = RabbitMqSource::connect(&url).await;
    }
    let source = source.unwrap();

    let queue = unique("db9s_test");
    let mut base = reqwest::Url::parse(&url).unwrap();
    let auth = (
        base.username().to_string(),
        base.password().map(str::to_string),
    );
    // credentials go in basic_auth only; leaving them in the URL sends two Authorization headers
    base.set_username("").unwrap();
    base.set_password(None).unwrap();
    let queue_url = base.join(&format!("api/queues/%2F/{queue}")).unwrap();
    let http = reqwest::Client::new();
    http.put(queue_url.clone())
        .basic_auth(&auth.0, auth.1.as_ref())
        .json(&json!({ "durable": false }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();

    assert!(source
        .list_databases()
        .await
        .unwrap()
        .contains(&"/".to_string()));
    assert!(source.list_containers("/").await.unwrap().contains(&queue));

    for payload in ["hello", "world"] {
        let res = source.run_query("/", &queue, payload).await.unwrap();
        assert_eq!(res.affected, Some(1));
    }

    let res = source.fetch_rows("/", &queue, 10, 0).await.unwrap();
    assert!(res.columns.iter().any(|c| c == "payload"));
    let payloads: Vec<_> = res.rows.iter().map(|r| r["payload"].clone()).collect();
    assert_eq!(payloads, vec![json!("hello"), json!("world")]);
    // peeking requeues, so the messages are still there
    assert_eq!(
        source
            .fetch_rows("/", &queue, 10, 0)
            .await
            .unwrap()
            .rows
            .len(),
        2
    );
    let paged = source.fetch_rows("/", &queue, 1, 1).await.unwrap();
    assert_eq!(paged.rows[0]["payload"], json!("world"));

    let row = &res.rows[0];
    assert!(source.update_row("/", &queue, row, row).await.is_err());

    source.delete_row("/", &queue, row).await.unwrap();
    assert!(source
        .fetch_rows("/", &queue, 10, 0)
        .await
        .unwrap()
        .rows
        .is_empty());

    http.delete(queue_url)
        .basic_auth(&auth.0, auth.1.as_ref())
        .send()
        .await
        .unwrap();
}
