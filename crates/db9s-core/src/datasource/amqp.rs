//! RabbitMQ over plain AMQP (lapin), for brokers without the management plugin.
//! AMQP can't list vhosts or queues: the vhost comes from the URL path and the
//! queues from a `queues=a,b` query parameter.
use super::{DataSource, QueryResult};
use crate::value::Row;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use lapin::options::{
    BasicGetOptions, BasicNackOptions, BasicPublishOptions, ConfirmSelectOptions, QueuePurgeOptions,
};
use lapin::{BasicProperties, Connection, ConnectionProperties};
use reqwest::Url;
use serde_json::{json, Map, Value};

const COLUMNS: [&str; 5] = [
    "payload",
    "routing_key",
    "exchange",
    "redelivered",
    "properties",
];

pub struct AmqpSource {
    conn: Connection,
    vhost: String,
    queues: Vec<String>,
}

impl AmqpSource {
    /// `url` like `amqp://user:pass@host:5672/%2F?queues=orders,emails`.
    pub async fn connect(url: &str) -> Result<Self> {
        let (amqp_url, vhost, queues) = parse_url(url)?;
        let conn = Connection::connect(&amqp_url, ConnectionProperties::default()).await?;
        Ok(Self {
            conn,
            vhost,
            queues,
        })
    }
}

/// Splits the db9s-only `queues` param off the URL and decodes the vhost.
fn parse_url(url: &str) -> Result<(String, String, Vec<String>)> {
    let mut parsed = Url::parse(url)?;
    let queues: Vec<String> = parsed
        .query_pairs()
        .filter(|(k, _)| k == "queues")
        .flat_map(|(_, v)| {
            v.split(',')
                .map(|q| q.trim().to_string())
                .collect::<Vec<_>>()
        })
        .filter(|q| !q.is_empty())
        .collect();
    if queues.is_empty() {
        return Err(anyhow!(
            "amqp:// urls need ?queues=a,b (AMQP can't list queues); \
             use the http:// management url to browse everything"
        ));
    }
    let others: Vec<(String, String)> = parsed
        .query_pairs()
        .filter(|(k, _)| k != "queues")
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    parsed.set_query(None);
    if !others.is_empty() {
        parsed.query_pairs_mut().extend_pairs(others);
    }
    let path = parsed.path().trim_start_matches('/');
    let vhost = if path.is_empty() {
        "/".to_string()
    } else {
        percent_decode(path)
    };
    Ok((parsed.to_string(), vhost, queues))
}

fn percent_decode(s: &str) -> String {
    reqwest::Url::parse(&format!("http://x/?v={s}"))
        .ok()
        .and_then(|u| u.query_pairs().next().map(|(_, v)| v.into_owned()))
        .unwrap_or_else(|| s.to_string())
}

fn delivery_to_row(d: &lapin::message::Delivery) -> Row {
    let p = &d.properties;
    let mut props = Map::new();
    let fields = [
        ("content_type", p.content_type()),
        ("message_id", p.message_id()),
        ("correlation_id", p.correlation_id()),
    ];
    for (k, v) in fields {
        if let Some(v) = v {
            props.insert(k.into(), json!(v.as_str()));
        }
    }
    if let Some(mode) = p.delivery_mode() {
        props.insert("delivery_mode".into(), json!(mode));
    }
    let values = [
        json!(String::from_utf8_lossy(&d.data)),
        json!(d.routing_key.as_str()),
        json!(d.exchange.as_str()),
        json!(d.redelivered),
        Value::Object(props),
    ];
    COLUMNS.iter().map(|c| c.to_string()).zip(values).collect()
}

#[async_trait]
impl DataSource for AmqpSource {
    fn is_queue(&self) -> bool {
        true
    }

    async fn list_databases(&self) -> Result<Vec<String>> {
        Ok(vec![self.vhost.clone()])
    }

    async fn list_containers(&self, _vhost: &str) -> Result<Vec<String>> {
        Ok(self.queues.clone())
    }

    // shortcut: AMQP can't skip messages, so offset gets limit+offset and drops the head; fine for peeking
    async fn fetch_rows(
        &self,
        _vhost: &str,
        queue: &str,
        limit: i64,
        offset: i64,
    ) -> Result<QueryResult> {
        let ch = self.conn.create_channel().await?;
        let mut deliveries = vec![];
        while (deliveries.len() as i64) < limit + offset {
            let opts = BasicGetOptions { no_ack: false };
            match ch.basic_get(queue.into(), opts).await? {
                Some(msg) => deliveries.push(msg.delivery),
                None => break,
            }
        }
        // nack everything back so peeking never consumes (messages get flagged redelivered)
        if let Some(last) = deliveries.last() {
            let opts = BasicNackOptions {
                multiple: true,
                requeue: true,
            };
            ch.basic_nack(last.delivery_tag, opts).await?;
        }
        ch.close(200, "peek done".into()).await.ok();
        Ok(QueryResult {
            columns: COLUMNS.iter().map(|c| c.to_string()).collect(),
            rows: deliveries
                .iter()
                .skip(offset as usize)
                .map(delivery_to_row)
                .collect(),
            affected: None,
        })
    }

    /// Publishes `raw` to the queue through the default exchange, waiting for the broker confirm.
    async fn run_query(&self, _vhost: &str, queue: &str, raw: &str) -> Result<QueryResult> {
        let ch = self.conn.create_channel().await?;
        ch.confirm_select(ConfirmSelectOptions::default()).await?;
        let opts = BasicPublishOptions {
            mandatory: true,
            ..Default::default()
        };
        let confirm = ch
            .basic_publish(
                "".into(),
                queue.into(),
                opts,
                raw.as_bytes(),
                BasicProperties::default(),
            )
            .await?
            .await?;
        ch.close(200, "publish done".into()).await.ok();
        if !confirm.is_ack() || confirm.take_message().is_some() {
            return Err(anyhow!("message was not routed to queue '{queue}'"));
        }
        Ok(QueryResult {
            columns: vec![],
            rows: vec![],
            affected: Some(1),
        })
    }

    /// A single message can't be removed from a queue, so this purges the whole queue.
    async fn delete_row(&self, _vhost: &str, queue: &str, _row: &Row) -> Result<()> {
        let ch = self.conn.create_channel().await?;
        ch.queue_purge(queue.into(), QueuePurgeOptions::default())
            .await?;
        ch.close(200, "purge done".into()).await.ok();
        Ok(())
    }

    async fn update_row(&self, _: &str, _: &str, _: &Row, _: &Row) -> Result<()> {
        Err(anyhow!("rabbitmq messages are read-only"))
    }
}

#[cfg(test)]
mod tests {
    use super::parse_url;

    #[test]
    fn parses_vhost_and_queues_and_keeps_other_params() {
        let (url, vhost, queues) =
            parse_url("amqp://u:p@h:5672/%2F?queues=orders,%20emails&heartbeat=10").unwrap();
        assert_eq!(vhost, "/");
        assert_eq!(queues, vec!["orders", "emails"]);
        assert_eq!(url, "amqp://u:p@h:5672/%2F?heartbeat=10");

        let (_, vhost, _) = parse_url("amqp://h/my%20vhost?queues=q").unwrap();
        assert_eq!(vhost, "my vhost");
        let (_, vhost, _) = parse_url("amqp://h?queues=q").unwrap();
        assert_eq!(vhost, "/");

        assert!(parse_url("amqp://h/%2F").is_err());
    }
}
