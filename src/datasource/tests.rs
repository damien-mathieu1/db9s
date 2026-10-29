//! Backend conformance tests: the same scenario runs against every supported
//! database through the `DataSource` trait, so all backends behave the same in the UI.
//!
//! Each backend is skipped unless its URL env var is set, e.g. with the dev stack:
//!   DB9S_TEST_POSTGRES_URL=postgres://db9s:db9s@localhost:55432/db9s \
//!   DB9S_TEST_MYSQL_URL=mysql://db9s:db9s@localhost:53306/db9s \
//!   DB9S_TEST_MONGO_URL=mongodb://localhost:57017 \
//!   DB9S_TEST_RABBITMQ_URL=http://db9s:db9s@localhost:55672 \
//!   DB9S_TEST_AMQP_URL=amqp://db9s:db9s@localhost:55673/%2F \
//!   DB9S_TEST_REDIS_URL=redis://localhost:56379 cargo test
use super::{
    amqp::AmqpSource, mongo::MongoSource, mysql::MySqlSource, postgres::PostgresSource,
    rabbitmq::RabbitMqSource, redis::RedisSource, DataSource,
};
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
    let query = format!("SELECT * FROM {table} WHERE name = 'z'");
    exercise(&source, "public", &table, &query).await;

    // composite primary key: the engine must match on both columns
    let pair = unique("db9s_test_pair");
    sqlx::query(&format!(
        "CREATE TABLE public.{pair} (a int, b int, v text, PRIMARY KEY (a, b))"
    ))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "INSERT INTO public.{pair} VALUES (1, 1, 'x'), (1, 2, 'y')"
    ))
    .execute(&pool)
    .await
    .unwrap();
    let rows = source
        .fetch_rows("public", &pair, 10, 0)
        .await
        .unwrap()
        .rows;
    let row = rows.iter().find(|r| r["b"] == json!(2)).unwrap();
    let mut edited = row.clone();
    edited.insert("v".into(), json!("z"));
    source
        .update_row("public", &pair, row, &edited)
        .await
        .unwrap();
    let rows = source
        .fetch_rows("public", &pair, 10, 0)
        .await
        .unwrap()
        .rows;
    let vs: Vec<_> = rows.iter().map(|r| r["v"].clone()).collect();
    assert!(vs.contains(&json!("x")) && vs.contains(&json!("z")));
    sqlx::query(&format!("DROP TABLE public.{pair}"))
        .execute(&pool)
        .await
        .unwrap();

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

/// Same broker scenario for every queue backend: publish, peek without consuming,
/// paginate, read-only edit, purge. Expects `queue` to exist, empty, in vhost "/".
async fn exercise_queue(source: &dyn DataSource, queue: &str) {
    assert!(source.is_queue());
    assert!(source
        .list_databases()
        .await
        .unwrap()
        .contains(&"/".to_string()));
    assert!(source
        .list_containers("/")
        .await
        .unwrap()
        .iter()
        .any(|q| q == queue));

    for payload in ["hello", "world"] {
        let res = source.run_query("/", queue, payload).await.unwrap();
        assert_eq!(res.affected, Some(1));
    }

    let res = source.fetch_rows("/", queue, 10, 0).await.unwrap();
    assert!(res.columns.iter().any(|c| c == "payload"));
    let payloads: Vec<_> = res.rows.iter().map(|r| r["payload"].clone()).collect();
    assert_eq!(payloads, vec![json!("hello"), json!("world")]);
    // peeking requeues, so the messages are still there
    assert_eq!(
        source
            .fetch_rows("/", queue, 10, 0)
            .await
            .unwrap()
            .rows
            .len(),
        2
    );
    let paged = source.fetch_rows("/", queue, 1, 1).await.unwrap();
    assert_eq!(paged.rows[0]["payload"], json!("world"));

    let row = &res.rows[0];
    assert!(source.update_row("/", queue, row, row).await.is_err());

    source.delete_row("/", queue, row).await.unwrap();
    assert!(source
        .fetch_rows("/", queue, 10, 0)
        .await
        .unwrap()
        .rows
        .is_empty());
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

    exercise_queue(&source, &queue).await;

    http.delete(queue_url)
        .basic_auth(&auth.0, auth.1.as_ref())
        .send()
        .await
        .unwrap();
}

/// RabbitMQ without the management plugin: plain AMQP through lapin.
#[tokio::test]
async fn amqp_conformance() {
    let Ok(url) = std::env::var("DB9S_TEST_AMQP_URL") else {
        eprintln!("skipped: DB9S_TEST_AMQP_URL not set");
        return;
    };
    let queue = unique("db9s_test");
    let conn = lapin::Connection::connect(&url, lapin::ConnectionProperties::default())
        .await
        .unwrap();
    let ch = conn.create_channel().await.unwrap();
    ch.queue_declare(
        queue.as_str().into(),
        lapin::options::QueueDeclareOptions::default(),
        lapin::types::FieldTable::default(),
    )
    .await
    .unwrap();

    let sep = if url.contains('?') { '&' } else { '?' };
    let source = AmqpSource::connect(&format!("{url}{sep}queues={queue}"))
        .await
        .unwrap();
    exercise_queue(&source, &queue).await;

    ch.queue_delete(
        queue.as_str().into(),
        lapin::options::QueueDeleteOptions::default(),
    )
    .await
    .unwrap();
}

/// Redis rows depend on the key type, so each type gets its own browse / edit / delete check.
#[tokio::test]
async fn redis_conformance() {
    let Ok(url) = std::env::var("DB9S_TEST_REDIS_URL") else {
        eprintln!("skipped: DB9S_TEST_REDIS_URL not set");
        return;
    };
    let source = RedisSource::connect(&url).await.unwrap();
    let p = unique("db9s_test");
    let k = |t: &str| format!("{p}:{t}");
    let db = "db0";
    let run = |cmd: String| {
        let source = &source;
        async move { source.run_query(db, "", &cmd).await.unwrap() }
    };
    let fetch = |key: String, limit, offset| {
        let source = &source;
        async move { source.fetch_rows(db, &key, limit, offset).await.unwrap() }
    };

    run(format!("SET {} 'hello world'", k("str"))).await;
    run(format!("HSET {} a 1 b 2", k("hash"))).await;
    run(format!("RPUSH {} x y z", k("list"))).await;
    run(format!("SADD {} s1 s2", k("set"))).await;
    run(format!("ZADD {} 1 m1 2 m2", k("zset"))).await;
    run(format!("XADD {} * f v1", k("stream"))).await;
    run(format!("XADD {} * f v2", k("stream"))).await;

    assert!(source
        .list_databases()
        .await
        .unwrap()
        .contains(&db.to_string()));
    assert!(source
        .list_containers(db)
        .await
        .unwrap()
        .contains(&k("str")));
    assert_eq!(
        run(format!("GET {}", k("str"))).await.rows[0]["result"],
        json!("hello world")
    );

    // string
    let row = fetch(k("str"), 10, 0).await.rows[0].clone();
    let mut edited = row.clone();
    edited.insert("value".into(), json!("bye"));
    source
        .update_row(db, &k("str"), &row, &edited)
        .await
        .unwrap();
    assert_eq!(fetch(k("str"), 10, 0).await.rows[0]["value"], json!("bye"));

    // hash: rename a field, delete another
    let res = fetch(k("hash"), 10, 0).await;
    assert_eq!(res.columns, vec!["field", "value"]);
    let a = res.rows[0].clone();
    let mut edited = a.clone();
    edited.insert("field".into(), json!("z"));
    source
        .update_row(db, &k("hash"), &a, &edited)
        .await
        .unwrap();
    source
        .delete_row(db, &k("hash"), &res.rows[1])
        .await
        .unwrap();
    let res = fetch(k("hash"), 10, 0).await;
    assert_eq!(res.rows.len(), 1);
    assert_eq!(res.rows[0]["field"], json!("z"));
    assert_eq!(res.rows[0]["value"], json!("1"));

    // list: paging, set by index, delete by index
    let page = fetch(k("list"), 2, 1).await;
    assert_eq!(page.rows[0]["index"], json!(1));
    assert_eq!(page.rows[0]["value"], json!("y"));
    let mut edited = page.rows[0].clone();
    edited.insert("value".into(), json!("Y"));
    source
        .update_row(db, &k("list"), &page.rows[0], &edited)
        .await
        .unwrap();
    source
        .delete_row(db, &k("list"), &page.rows[1])
        .await
        .unwrap();
    let values: Vec<_> = fetch(k("list"), 10, 0)
        .await
        .rows
        .iter()
        .map(|r| r["value"].clone())
        .collect();
    assert_eq!(values, vec![json!("x"), json!("Y")]);

    // set
    let res = fetch(k("set"), 10, 0).await;
    let mut edited = res.rows[0].clone();
    edited.insert("member".into(), json!("s9"));
    source
        .update_row(db, &k("set"), &res.rows[0], &edited)
        .await
        .unwrap();
    source
        .delete_row(db, &k("set"), &res.rows[1])
        .await
        .unwrap();
    let res = fetch(k("set"), 10, 0).await;
    assert_eq!(res.rows.len(), 1);
    assert_eq!(res.rows[0]["member"], json!("s9"));

    // zset: change a score, delete a member
    let res = fetch(k("zset"), 10, 0).await;
    assert_eq!(res.rows[0]["member"], json!("m1"));
    let mut edited = res.rows[0].clone();
    edited.insert("score".into(), json!(5));
    source
        .update_row(db, &k("zset"), &res.rows[0], &edited)
        .await
        .unwrap();
    source
        .delete_row(db, &k("zset"), &res.rows[1])
        .await
        .unwrap();
    let res = fetch(k("zset"), 10, 0).await;
    assert_eq!(res.rows.len(), 1);
    assert_eq!(res.rows[0]["score"], json!(5.0));

    // stream: read-only entries, deletable
    let res = fetch(k("stream"), 10, 0).await;
    assert_eq!(res.rows.len(), 2);
    assert_eq!(res.rows[0]["fields"], json!({"f": "v1"}));
    assert!(source
        .update_row(db, &k("stream"), &res.rows[0], &res.rows[0])
        .await
        .is_err());
    source
        .delete_row(db, &k("stream"), &res.rows[0])
        .await
        .unwrap();
    assert_eq!(fetch(k("stream"), 10, 0).await.rows.len(), 1);

    // deleting the only row of a string removes the key
    let row = fetch(k("str"), 10, 0).await.rows[0].clone();
    source.delete_row(db, &k("str"), &row).await.unwrap();
    assert!(fetch(k("str"), 10, 0).await.rows.is_empty());

    for t in ["hash", "list", "set", "zset", "stream"] {
        run(format!("DEL {}", k(t))).await;
    }
}

#[tokio::test]
async fn mysql_conformance() {
    let Ok(url) = std::env::var("DB9S_TEST_MYSQL_URL") else {
        eprintln!("skipped: DB9S_TEST_MYSQL_URL not set");
        return;
    };
    let table = unique("db9s_test");
    let pool = sqlx::MySqlPool::connect(&url).await.unwrap();
    sqlx::query(&format!(
        "CREATE TABLE {table} (id INT AUTO_INCREMENT PRIMARY KEY, name VARCHAR(50) NOT NULL)"
    ))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(&format!("INSERT INTO {table} (name) VALUES ('a'), ('b')"))
        .execute(&pool)
        .await
        .unwrap();

    let source = MySqlSource::connect(&url).await.unwrap();
    let query = format!("SELECT * FROM {table} WHERE name = 'z'");
    exercise(&source, "db9s", &table, &query).await;

    sqlx::query(&format!("DROP TABLE {table}"))
        .execute(&pool)
        .await
        .unwrap();
}
