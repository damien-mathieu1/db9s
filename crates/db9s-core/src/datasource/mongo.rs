use super::{DataSource, QueryResult};
use crate::value::{bson_to_json, json_to_bson, Row};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use futures_util::TryStreamExt;
use mongodb::{bson::doc, options::FindOptions, Client};

pub struct MongoSource {
    client: Client,
}

impl MongoSource {
    pub async fn connect(url: &str) -> Result<Self> {
        let client = Client::with_uri_str(url).await?;
        client.list_database_names().await?;
        Ok(Self { client })
    }

    fn rows_to_result(docs: Vec<mongodb::bson::Document>) -> QueryResult {
        let mut columns_order: Vec<String> = vec![];
        let mut rows = vec![];
        for doc in docs {
            let json = bson_to_json(mongodb::bson::Bson::Document(doc));
            if let serde_json::Value::Object(map) = json {
                for k in map.keys() {
                    if !columns_order.contains(k) {
                        columns_order.push(k.clone());
                    }
                }
                rows.push(map);
            }
        }
        QueryResult {
            columns: columns_order,
            rows,
            affected: None,
        }
    }
}

/// A find filter can't write; a pipeline can, but only through a `$out` or `$merge` stage.
fn is_read_query(raw: &str) -> bool {
    let raw = raw.trim();
    if !raw.starts_with('[') {
        return true;
    }
    serde_json::from_str::<Vec<serde_json::Value>>(raw).is_ok_and(|stages| {
        !stages
            .iter()
            .any(|s| s.get("$out").is_some() || s.get("$merge").is_some())
    })
}

#[async_trait]
impl DataSource for MongoSource {
    fn is_read_query(&self, raw: &str) -> bool {
        is_read_query(raw)
    }

    async fn list_databases(&self) -> Result<Vec<String>> {
        Ok(self.client.list_database_names().await?)
    }

    async fn list_containers(&self, database: &str) -> Result<Vec<String>> {
        Ok(self
            .client
            .database(database)
            .list_collection_names()
            .await?)
    }

    async fn fetch_rows(
        &self,
        database: &str,
        container: &str,
        limit: i64,
        offset: i64,
    ) -> Result<QueryResult> {
        let coll = self
            .client
            .database(database)
            .collection::<mongodb::bson::Document>(container);
        let opts = FindOptions::builder()
            .limit(limit)
            .skip(offset as u64)
            .build();
        let mut cursor = coll.find(doc! {}).with_options(opts).await?;
        let mut docs = vec![];
        while let Some(d) = cursor.try_next().await? {
            docs.push(d);
        }
        Ok(Self::rows_to_result(docs))
    }

    async fn run_query(&self, database: &str, container: &str, raw: &str) -> Result<QueryResult> {
        let coll = self
            .client
            .database(database)
            .collection::<mongodb::bson::Document>(container);
        let trimmed = raw.trim();
        if trimmed.starts_with('[') {
            let pipeline: Vec<mongodb::bson::Document> =
                serde_json::from_str::<Vec<serde_json::Value>>(trimmed)?
                    .into_iter()
                    .map(|v| mongodb::bson::to_document(&v))
                    .collect::<Result<_, _>>()?;
            let mut cursor = coll.aggregate(pipeline).await?;
            let mut docs = vec![];
            while let Some(d) = cursor.try_next().await? {
                docs.push(d);
            }
            Ok(Self::rows_to_result(docs))
        } else {
            let filter_json: serde_json::Value = if trimmed.is_empty() {
                serde_json::json!({})
            } else {
                serde_json::from_str(trimmed)?
            };
            let filter: mongodb::bson::Document = mongodb::bson::to_document(&filter_json)?;
            let mut cursor = coll.find(filter).limit(500).await?;
            let mut docs = vec![];
            while let Some(d) = cursor.try_next().await? {
                docs.push(d);
            }
            Ok(Self::rows_to_result(docs))
        }
    }

    async fn delete_row(&self, database: &str, container: &str, row: &Row) -> Result<()> {
        let id = row
            .get("_id")
            .ok_or_else(|| anyhow!("document has no _id"))?;
        let coll = self
            .client
            .database(database)
            .collection::<mongodb::bson::Document>(container);
        coll.delete_one(doc! { "_id": id_filter(id)? }).await?;
        Ok(())
    }

    async fn update_row(
        &self,
        database: &str,
        container: &str,
        original: &Row,
        edited: &Row,
    ) -> Result<()> {
        let id = original
            .get("_id")
            .ok_or_else(|| anyhow!("document has no _id"))?;
        let coll = self
            .client
            .database(database)
            .collection::<mongodb::bson::Document>(container);
        let mut new_doc = mongodb::bson::to_document(&serde_json::Value::Object(edited.clone()))?;
        new_doc.remove("_id");
        coll.update_one(doc! { "_id": id_filter(id)? }, doc! { "$set": new_doc })
            .await?;
        Ok(())
    }
}

fn id_filter(id: &serde_json::Value) -> Result<mongodb::bson::Bson> {
    if let serde_json::Value::String(s) = id {
        if let Ok(oid) = mongodb::bson::oid::ObjectId::parse_str(s) {
            return Ok(mongodb::bson::Bson::ObjectId(oid));
        }
    }
    Ok(json_to_bson(id))
}

#[cfg(test)]
mod tests {
    use super::is_read_query;

    #[test]
    fn pipelines_that_write_are_not_read_queries() {
        assert!(is_read_query(""));
        assert!(is_read_query(r#"{"name": "$out"}"#));
        assert!(is_read_query(r#"[{"$match": {"name": "$out"}}]"#));
        assert!(!is_read_query(r#"[{"$match": {}}, {"$out": "t"}]"#));
        assert!(!is_read_query(r#" [{"$merge": {"into": "t"}}]"#));
        assert!(!is_read_query(r#"[{"\u0024out": "t"}]"#));
        assert!(!is_read_query("[not json"));
    }
}
