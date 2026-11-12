//! RabbitMQ through its management HTTP API: databases = vhosts,
//! containers = queues, rows = messages peeked without being consumed.
use super::{DataSource, QueryResult};
use crate::value::Row;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::{Client, Url};
use serde_json::{json, Value};

const COLUMNS: [&str; 5] = [
    "payload",
    "routing_key",
    "exchange",
    "redelivered",
    "properties",
];

pub struct RabbitMqSource {
    client: Client,
    base: Url,
    user: String,
    password: String,
}

impl RabbitMqSource {
    /// `url` is the management API, e.g. `http://guest:guest@localhost:15672`.
    pub async fn connect(url: &str) -> Result<Self> {
        let mut base = Url::parse(url)?;
        let user = base.username().to_string();
        let password = base.password().unwrap_or_default().to_string();
        base.set_username("").ok();
        base.set_password(None).ok();
        let source = Self {
            client: Client::new(),
            base,
            user,
            password,
        };
        source.get(&["overview"]).await?;
        Ok(source)
    }

    /// Builds `/api/<segments>`, percent-encoding each one (the default vhost is `/`).
    fn url(&self, segments: &[&str]) -> Url {
        let mut url = self.base.clone();
        url.path_segments_mut()
            .expect("http url")
            .pop_if_empty()
            .push("api")
            .extend(segments);
        url
    }

    async fn send(&self, req: reqwest::RequestBuilder) -> Result<Value> {
        let resp = req
            .basic_auth(&self.user, Some(&self.password))
            .send()
            .await?;
        let status = resp.status();
        let body = resp.text().await?;
        if !status.is_success() {
            return Err(anyhow!("rabbitmq api {status}: {body}"));
        }
        Ok(if body.is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&body)?
        })
    }

    async fn get(&self, segments: &[&str]) -> Result<Value> {
        self.send(self.client.get(self.url(segments))).await
    }

    fn names(list: Value) -> Vec<String> {
        let mut names: Vec<String> = list
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| item["name"].as_str().map(str::to_string))
            .collect();
        names.sort();
        names
    }
}

#[async_trait]
impl DataSource for RabbitMqSource {
    fn is_queue(&self) -> bool {
        true
    }

    async fn list_databases(&self) -> Result<Vec<String>> {
        Ok(Self::names(self.get(&["vhosts"]).await?))
    }

    async fn list_containers(&self, vhost: &str) -> Result<Vec<String>> {
        Ok(Self::names(self.get(&["queues", vhost]).await?))
    }

    // shortcut: the API can't skip messages, so offset fetches limit+offset and drops the head; fine for peeking
    async fn fetch_rows(
        &self,
        vhost: &str,
        queue: &str,
        limit: i64,
        offset: i64,
    ) -> Result<QueryResult> {
        // ack_requeue_true puts every message back, flagged as redelivered
        let body = json!({
            "count": limit + offset,
            "ackmode": "ack_requeue_true",
            "encoding": "auto",
            "truncate": 50000,
        });
        let req = self
            .client
            .post(self.url(&["queues", vhost, queue, "get"]))
            .json(&body);
        let messages = self.send(req).await?;
        let rows = messages
            .as_array()
            .into_iter()
            .flatten()
            .skip(offset as usize)
            .map(|m| {
                COLUMNS
                    .iter()
                    .map(|c| (c.to_string(), m[*c].clone()))
                    .collect::<Row>()
            })
            .collect();
        Ok(QueryResult {
            columns: COLUMNS.iter().map(|c| c.to_string()).collect(),
            rows,
            affected: None,
        })
    }

    /// Publishes `raw` as a message to the queue through the default exchange.
    async fn run_query(&self, vhost: &str, queue: &str, raw: &str) -> Result<QueryResult> {
        let body = json!({
            "properties": {},
            "routing_key": queue,
            "payload": raw,
            "payload_encoding": "string",
        });
        let req = self
            .client
            .post(self.url(&["exchanges", vhost, "amq.default", "publish"]))
            .json(&body);
        let res = self.send(req).await?;
        if res["routed"] != json!(true) {
            return Err(anyhow!("message was not routed to queue '{queue}'"));
        }
        Ok(QueryResult {
            columns: vec![],
            rows: vec![],
            affected: Some(1),
        })
    }

    /// A single message can't be removed from a queue, so this purges the whole queue.
    async fn delete_row(&self, vhost: &str, queue: &str, _row: &Row) -> Result<()> {
        let req = self
            .client
            .delete(self.url(&["queues", vhost, queue, "contents"]));
        self.send(req).await?;
        Ok(())
    }

    async fn update_row(&self, _: &str, _: &str, _: &Row, _: &Row) -> Result<()> {
        Err(anyhow!("rabbitmq messages are read-only"))
    }
}
