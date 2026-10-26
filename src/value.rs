use serde_json::{Map, Value};

pub type Row = Map<String, Value>;

pub fn bson_to_json(v: bson::Bson) -> Value {
    use bson::Bson::*;
    match v {
        ObjectId(oid) => Value::String(oid.to_hex()),
        DateTime(dt) => Value::String(dt.try_to_rfc3339_string().unwrap_or_default()),
        Decimal128(d) => Value::String(d.to_string()),
        Binary(b) => Value::String(format!("<binary {} bytes>", b.bytes.len())),
        Null => Value::Null,
        Document(doc) => {
            let mut map = Map::new();
            for (k, val) in doc {
                map.insert(k, bson_to_json(val));
            }
            Value::Object(map)
        }
        Array(arr) => Value::Array(arr.into_iter().map(bson_to_json).collect()),
        other => serde_json::to_value(&other).unwrap_or(Value::Null),
    }
}

pub fn json_to_bson(v: &Value) -> bson::Bson {
    bson::to_bson(v).unwrap_or(bson::Bson::Null)
}

pub fn pg_row_to_json(row: &sqlx::postgres::PgRow) -> Row {
    use sqlx::{Column, Row as _, ValueRef};
    let mut map = Map::new();
    for col in row.columns() {
        let name = col.name().to_string();
        let raw = row.try_get_raw(col.ordinal());
        let val = match raw {
            Ok(raw_val) if !raw_val.is_null() => decode_pg_value(row, col),
            _ => Value::Null,
        };
        map.insert(name, val);
    }
    map
}

fn decode_pg_value(row: &sqlx::postgres::PgRow, col: &sqlx::postgres::PgColumn) -> Value {
    use sqlx::{Column, Row as _};
    let idx = col.ordinal();
    let type_name = col.type_info().to_string().to_lowercase();

    macro_rules! try_as {
        ($t:ty) => {
            row.try_get::<$t, _>(idx)
                .ok()
                .map(|v| serde_json::to_value(v).unwrap_or(Value::Null))
        };
    }

    let result = match type_name.as_str() {
        "int2" => try_as!(i16),
        "int4" => try_as!(i32),
        "int8" => try_as!(i64),
        "float4" => try_as!(f32),
        "float8" => try_as!(f64),
        "bool" => try_as!(bool),
        "numeric" => row
            .try_get::<f64, _>(idx)
            .ok()
            .and_then(|v| serde_json::to_value(v).ok()),
        "json" | "jsonb" => try_as!(serde_json::Value),
        "uuid" => try_as!(uuid::Uuid),
        "timestamp" => row
            .try_get::<chrono::NaiveDateTime, _>(idx)
            .ok()
            .map(|v| Value::String(v.to_string())),
        "timestamptz" => row
            .try_get::<chrono::DateTime<chrono::Utc>, _>(idx)
            .ok()
            .map(|v| Value::String(v.to_rfc3339())),
        "date" => row
            .try_get::<chrono::NaiveDate, _>(idx)
            .ok()
            .map(|v| Value::String(v.to_string())),
        "_text" | "_varchar" => try_as!(Vec<String>),
        "_int4" => try_as!(Vec<i32>),
        _ => try_as!(String),
    };

    result.unwrap_or(Value::Null)
}

/// Text shown in the inline field editor: strings unquoted, everything else as JSON.
pub fn value_to_input(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Parses inline input back to a value: valid JSON (`42`, `true`, `null`, `"42"`, `{..}`)
/// keeps its type, anything else becomes a plain string.
pub fn input_to_value(input: &str) -> Value {
    serde_json::from_str(input).unwrap_or_else(|_| Value::String(input.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn inline_input_round_trips_and_infers_types() {
        assert_eq!(input_to_value("hello"), json!("hello"));
        assert_eq!(input_to_value("42"), json!(42));
        assert_eq!(input_to_value("\"42\""), json!("42"));
        assert_eq!(input_to_value("true"), json!(true));
        assert_eq!(input_to_value("null"), Value::Null);
        assert_eq!(input_to_value("{\"a\":1}"), json!({"a": 1}));
        assert_eq!(value_to_input(&json!("hi")), "hi");
        assert_eq!(value_to_input(&json!(3)), "3");
    }
}
