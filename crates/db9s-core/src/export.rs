//! `:export csv|json`: the rows on screen to a file in the Downloads folder.
use crate::datasource::QueryResult;
use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::path::PathBuf;

/// RFC 4180: a header line, then one line per row; null is an empty cell.
pub fn to_csv(res: &QueryResult) -> String {
    fn cell(v: Option<&Value>) -> String {
        let s = match v {
            None | Some(Value::Null) => return String::new(),
            Some(Value::String(s)) => s.clone(),
            Some(other) => other.to_string(),
        };
        if s.contains([',', '"', '\n', '\r']) {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s
        }
    }
    let mut out = res
        .columns
        .iter()
        .map(|c| cell(Some(&Value::String(c.clone()))))
        .collect::<Vec<_>>()
        .join(",");
    out.push_str("\r\n");
    for row in &res.rows {
        let line: Vec<String> = res.columns.iter().map(|c| cell(row.get(c))).collect();
        out.push_str(&line.join(","));
        out.push_str("\r\n");
    }
    out
}

/// Writes `res` as `csv` or `json` to `<Downloads>/<name>-<timestamp>.<ext>` (the
/// current directory if there is no Downloads folder) and returns the path.
pub fn write(res: &QueryResult, format: &str, name: &str) -> Result<PathBuf> {
    let body = match format {
        "csv" => to_csv(res),
        "json" => serde_json::to_string_pretty(&res.rows)?,
        other => bail!("export format '{other}' is not csv or json"),
    };
    let dir = directories::UserDirs::new()
        .and_then(|d| d.download_dir().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    // table names may hold anything a path can't
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || "-_.".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let path = dir.join(format!("{safe}-{stamp}.{format}"));
    std::fs::write(&path, body).with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_quotes_only_what_needs_it() {
        let res = QueryResult {
            columns: vec!["id".into(), "note".into(), "meta".into()],
            rows: vec![
                serde_json::from_str(r#"{"id": 1, "note": "plain", "meta": null}"#).unwrap(),
                serde_json::from_str(r#"{"id": 2, "note": "a, \"b\"\nc", "meta": {"k": 1}}"#)
                    .unwrap(),
            ],
            affected: None,
        };
        assert_eq!(
            to_csv(&res),
            "id,note,meta\r\n1,plain,\r\n2,\"a, \"\"b\"\"\nc\",\"{\"\"k\"\":1}\"\r\n"
        );
    }
}
