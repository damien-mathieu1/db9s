//! Redis: databases = logical DBs (`db0`, `db1`, ...), containers = keys,
//! rows = the key's content, shaped by its type (string, hash, list, set, zset, stream).
use super::{DataSource, QueryResult};
use crate::value::Row;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use redis::aio::MultiplexedConnection;
use redis::{cmd, Client, Value as RValue};
use serde_json::{json, Map, Value};

const MAX_KEYS: usize = 1000;

pub struct RedisSource {
    client: Client,
}

impl RedisSource {
    /// `url` like `redis://:pass@host:6379` or `rediss://...` for TLS.
    pub async fn connect(url: &str) -> Result<Self> {
        let client = Client::open(url)?;
        let mut con = client.get_multiplexed_async_connection().await?;
        cmd("PING").query_async::<()>(&mut con).await?;
        Ok(Self { client })
    }

    // shortcut: one connection per call because SELECT is per-connection state; pool per db if remote latency hurts
    async fn conn(&self, db: &str) -> Result<MultiplexedConnection> {
        let index: i64 = db
            .strip_prefix("db")
            .and_then(|n| n.parse().ok())
            .ok_or_else(|| anyhow!("invalid redis database '{db}'"))?;
        let mut con = self.client.get_multiplexed_async_connection().await?;
        cmd("SELECT").arg(index).query_async::<()>(&mut con).await?;
        Ok(con)
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn to_json(v: RValue) -> Value {
    match v {
        RValue::Nil => Value::Null,
        RValue::Int(i) => json!(i),
        RValue::BulkString(b) => json!(text(&b)),
        RValue::SimpleString(s) => json!(s),
        RValue::Okay => json!("OK"),
        RValue::Double(d) => json!(d),
        RValue::Boolean(b) => json!(b),
        RValue::Array(items) | RValue::Set(items) => {
            Value::Array(items.into_iter().map(to_json).collect())
        }
        RValue::Map(pairs) => Value::Object(
            pairs
                .into_iter()
                .map(|(k, v)| (key_string(to_json(k)), to_json(v)))
                .collect(),
        ),
        other => json!(format!("{other:?}")),
    }
}

/// How a JSON value from the UI is written back to Redis (strings unquoted).
fn key_string(v: Value) -> String {
    match v {
        Value::String(s) => s,
        other => other.to_string(),
    }
}

fn field(row: &Row, name: &str) -> Result<String> {
    row.get(name)
        .cloned()
        .map(key_string)
        .ok_or_else(|| anyhow!("row has no '{name}'"))
}

fn rows(columns: &[&str], data: Vec<Vec<Value>>) -> QueryResult {
    QueryResult {
        columns: columns.iter().map(|c| c.to_string()).collect(),
        rows: data
            .into_iter()
            .map(|vals| columns.iter().map(|c| c.to_string()).zip(vals).collect())
            .collect(),
        affected: None,
    }
}

/// Splits a command line like a shell would for the simple cases: spaces separate
/// arguments, single or double quotes group them.
fn split_command(raw: &str) -> Result<Vec<String>> {
    let mut args = vec![];
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut in_arg = false;
    for c in raw.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                in_arg = true;
            }
            (None, c) if c.is_whitespace() => {
                if in_arg {
                    args.push(std::mem::take(&mut cur));
                    in_arg = false;
                }
            }
            (None, c) => {
                cur.push(c);
                in_arg = true;
            }
        }
    }
    if quote.is_some() {
        return Err(anyhow!("unterminated quote"));
    }
    if in_arg {
        args.push(cur);
    }
    if args.is_empty() {
        return Err(anyhow!("empty command"));
    }
    Ok(args)
}

async fn key_type(con: &mut MultiplexedConnection, key: &str) -> Result<String> {
    Ok(cmd("TYPE").arg(key).query_async(con).await?)
}

#[async_trait]
impl DataSource for RedisSource {
    /// db0 plus every non-empty logical database (CONFIG is often disabled on managed Redis).
    async fn list_databases(&self) -> Result<Vec<String>> {
        let mut con = self.conn("db0").await?;
        let info: String = cmd("INFO").arg("keyspace").query_async(&mut con).await?;
        let mut dbs: Vec<i64> = info
            .lines()
            .filter_map(|l| l.strip_prefix("db")?.split(':').next()?.parse().ok())
            .chain([0])
            .collect();
        dbs.sort();
        dbs.dedup();
        Ok(dbs.into_iter().map(|n| format!("db{n}")).collect())
    }

    // shortcut: lists at most MAX_KEYS keys via SCAN; add a key filter if people browse huge keyspaces
    async fn list_containers(&self, db: &str) -> Result<Vec<String>> {
        let mut con = self.conn(db).await?;
        let mut cursor = "0".to_string();
        let mut keys = vec![];
        loop {
            let (next, batch): (String, Vec<Vec<u8>>) = cmd("SCAN")
                .arg(&cursor)
                .arg("COUNT")
                .arg(500)
                .query_async(&mut con)
                .await?;
            keys.extend(batch.iter().map(|k| text(k)));
            cursor = next;
            if cursor == "0" || keys.len() >= MAX_KEYS {
                break;
            }
        }
        keys.sort();
        keys.dedup();
        keys.truncate(MAX_KEYS);
        Ok(keys)
    }

    // shortcut: hash/set/stream load the whole key then page in memory; fine for browsing, not for million-entry keys
    async fn fetch_rows(
        &self,
        db: &str,
        key: &str,
        limit: i64,
        offset: i64,
    ) -> Result<QueryResult> {
        let mut con = self.conn(db).await?;
        let (skip, take) = (offset as usize, limit as usize);
        let page = |data: Vec<Vec<Value>>| data.into_iter().skip(skip).take(take).collect();
        Ok(match key_type(&mut con, key).await?.as_str() {
            "string" => {
                let v: Vec<u8> = cmd("GET").arg(key).query_async(&mut con).await?;
                rows(&["value"], page(vec![vec![json!(text(&v))]]))
            }
            "hash" => {
                let mut pairs: Vec<(Vec<u8>, Vec<u8>)> =
                    cmd("HGETALL").arg(key).query_async(&mut con).await?;
                pairs.sort();
                let data = pairs
                    .iter()
                    .map(|(f, v)| vec![json!(text(f)), json!(text(v))])
                    .collect();
                rows(&["field", "value"], page(data))
            }
            "list" => {
                let items: Vec<Vec<u8>> = cmd("LRANGE")
                    .arg(key)
                    .arg(offset)
                    .arg(offset + limit - 1)
                    .query_async(&mut con)
                    .await?;
                let data = items
                    .iter()
                    .enumerate()
                    .map(|(i, v)| vec![json!(offset + i as i64), json!(text(v))])
                    .collect();
                rows(&["index", "value"], data)
            }
            "set" => {
                let mut members: Vec<Vec<u8>> =
                    cmd("SMEMBERS").arg(key).query_async(&mut con).await?;
                members.sort();
                let data = members.iter().map(|m| vec![json!(text(m))]).collect();
                rows(&["member"], page(data))
            }
            "zset" => {
                let items: Vec<(Vec<u8>, f64)> = cmd("ZRANGE")
                    .arg(key)
                    .arg(offset)
                    .arg(offset + limit - 1)
                    .arg("WITHSCORES")
                    .query_async(&mut con)
                    .await?;
                let data = items
                    .iter()
                    .map(|(m, s)| vec![json!(text(m)), json!(s)])
                    .collect();
                rows(&["member", "score"], data)
            }
            "stream" => {
                let entries: RValue = cmd("XRANGE")
                    .arg(key)
                    .arg("-")
                    .arg("+")
                    .arg("COUNT")
                    .arg(offset + limit)
                    .query_async(&mut con)
                    .await?;
                let data = match to_json(entries) {
                    Value::Array(entries) => entries
                        .into_iter()
                        .filter_map(|e| {
                            let [id, kv] = <[Value; 2]>::try_from(e.as_array()?.clone()).ok()?;
                            let kv = kv.as_array()?.clone();
                            let fields: Map<String, Value> = kv
                                .chunks(2)
                                .map(|p| (key_string(p[0].clone()), p[1].clone()))
                                .collect();
                            Some(vec![id, Value::Object(fields)])
                        })
                        .collect(),
                    _ => vec![],
                };
                rows(&["id", "fields"], page(data))
            }
            "none" => rows(&["value"], vec![]),
            other => return Err(anyhow!("redis type '{other}' is not supported yet")),
        })
    }

    /// Runs any Redis command, e.g. `HGETALL user:1` or `SET greeting "hello world"`.
    async fn run_query(&self, db: &str, _key: &str, raw: &str) -> Result<QueryResult> {
        let args = split_command(raw)?;
        let mut con = self.conn(db).await?;
        let reply: RValue = cmd(&args[0]).arg(&args[1..]).query_async(&mut con).await?;
        Ok(match to_json(reply) {
            Value::Array(items) => rows(&["value"], items.into_iter().map(|v| vec![v]).collect()),
            Value::Object(map) => rows(
                &["field", "value"],
                map.into_iter().map(|(k, v)| vec![json!(k), v]).collect(),
            ),
            single => rows(&["result"], vec![vec![single]]),
        })
    }

    async fn delete_row(&self, db: &str, key: &str, row: &Row) -> Result<()> {
        let mut con = self.conn(db).await?;
        match key_type(&mut con, key).await?.as_str() {
            "string" => cmd("DEL").arg(key).query_async::<()>(&mut con).await?,
            "hash" => {
                cmd("HDEL")
                    .arg(key)
                    .arg(field(row, "field")?)
                    .query_async::<()>(&mut con)
                    .await?
            }
            "list" => {
                // LREM removes by value, so mark the exact index first, then remove the marker
                let marker = "__db9s_deleted__";
                cmd("LSET")
                    .arg(key)
                    .arg(field(row, "index")?)
                    .arg(marker)
                    .query_async::<()>(&mut con)
                    .await?;
                cmd("LREM")
                    .arg(key)
                    .arg(1)
                    .arg(marker)
                    .query_async::<()>(&mut con)
                    .await?
            }
            "set" => {
                cmd("SREM")
                    .arg(key)
                    .arg(field(row, "member")?)
                    .query_async::<()>(&mut con)
                    .await?
            }
            "zset" => {
                cmd("ZREM")
                    .arg(key)
                    .arg(field(row, "member")?)
                    .query_async::<()>(&mut con)
                    .await?
            }
            "stream" => {
                cmd("XDEL")
                    .arg(key)
                    .arg(field(row, "id")?)
                    .query_async::<()>(&mut con)
                    .await?
            }
            other => return Err(anyhow!("can't delete from redis type '{other}'")),
        }
        Ok(())
    }

    async fn update_row(&self, db: &str, key: &str, original: &Row, edited: &Row) -> Result<()> {
        let mut con = self.conn(db).await?;
        let mut pipe = redis::pipe();
        pipe.atomic();
        match key_type(&mut con, key).await?.as_str() {
            "string" => {
                pipe.cmd("SET")
                    .arg(key)
                    .arg(field(edited, "value")?)
                    .arg("KEEPTTL");
            }
            "hash" => {
                let (old, new) = (field(original, "field")?, field(edited, "field")?);
                if old != new {
                    pipe.cmd("HDEL").arg(key).arg(&old);
                }
                pipe.cmd("HSET")
                    .arg(key)
                    .arg(new)
                    .arg(field(edited, "value")?);
            }
            "list" => {
                pipe.cmd("LSET")
                    .arg(key)
                    .arg(field(original, "index")?)
                    .arg(field(edited, "value")?);
            }
            "set" => {
                pipe.cmd("SREM").arg(key).arg(field(original, "member")?);
                pipe.cmd("SADD").arg(key).arg(field(edited, "member")?);
            }
            "zset" => {
                let (old, new) = (field(original, "member")?, field(edited, "member")?);
                if old != new {
                    pipe.cmd("ZREM").arg(key).arg(&old);
                }
                pipe.cmd("ZADD")
                    .arg(key)
                    .arg(field(edited, "score")?)
                    .arg(new);
            }
            other => return Err(anyhow!("redis type '{other}' entries are read-only")),
        }
        pipe.query_async::<()>(&mut con).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::split_command;

    #[test]
    fn splits_commands_like_a_shell() {
        assert_eq!(
            split_command(r#"SET "a b"  'c "d"' e"#).unwrap(),
            vec!["SET", "a b", "c \"d\"", "e"]
        );
        assert_eq!(
            split_command("HSET k f ''").unwrap(),
            vec!["HSET", "k", "f", ""]
        );
        assert!(split_command("GET \"oops").is_err());
        assert!(split_command("   ").is_err());
    }
}
