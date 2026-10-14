//! Data files for data-driven runs: one iteration per CSV row or JSON object.

/// One iteration's variables, in file order.
pub type Row = Vec<(String, String)>;

#[derive(Debug, thiserror::Error)]
pub enum DataError {
    #[error("cannot read `{0}`: {1}")]
    Read(String, String),
    #[error("invalid JSON data file: {0}")]
    Json(String),
    #[error("a JSON data file must be an array of objects")]
    NotArray,
    #[error("CSV line {0}: unterminated quoted field")]
    Unterminated(usize),
}

/// Reads a `.json` or `.csv` file (anything not starting with `[` is CSV).
pub fn load(path: &str) -> Result<Vec<Row>, DataError> {
    let text = std::fs::read_to_string(path).map_err(|e| DataError::Read(path.to_string(), e.to_string()))?;
    parse(&text)
}

pub fn parse(text: &str) -> Result<Vec<Row>, DataError> {
    let text = text.trim_start_matches('\u{feff}');
    if text.trim_start().starts_with('[') {
        parse_json(text)
    } else {
        parse_csv(text)
    }
}

pub fn parse_json(text: &str) -> Result<Vec<Row>, DataError> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| DataError::Json(e.to_string()))?;
    let serde_json::Value::Array(items) = value else { return Err(DataError::NotArray) };
    items
        .into_iter()
        .map(|item| match item {
            serde_json::Value::Object(map) => Ok(map
                .into_iter()
                .map(|(k, v)| {
                    let v = match v {
                        serde_json::Value::String(s) => s,
                        serde_json::Value::Null => String::new(),
                        other => other.to_string(),
                    };
                    (k, v)
                })
                .collect()),
            _ => Err(DataError::NotArray),
        })
        .collect()
}

/// RFC 4180 CSV with a header row; `,` or `;` (whichever the header uses).
pub fn parse_csv(text: &str) -> Result<Vec<Row>, DataError> {
    let first_line = text.lines().next().unwrap_or("");
    let sep = if first_line.matches(';').count() > first_line.matches(',').count() { ';' } else { ',' };
    let records = csv_records(text, sep)?;
    let mut iter = records.into_iter();
    let Some(header) = iter.next() else { return Ok(Vec::new()) };
    let header: Vec<String> = header.into_iter().map(|h| h.trim().to_string()).collect();
    Ok(iter
        .filter(|r| !(r.len() == 1 && r[0].is_empty()))
        .map(|r| {
            header
                .iter()
                .enumerate()
                .filter(|(_, h)| !h.is_empty())
                .map(|(i, h)| (h.clone(), r.get(i).cloned().unwrap_or_default()))
                .collect()
        })
        .collect())
}

fn csv_records(text: &str, sep: char) -> Result<Vec<Vec<String>>, DataError> {
    let mut records = Vec::new();
    let mut record = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut line = 1;
    let mut quote_line = 0;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => quoted = false,
                '\n' => {
                    line += 1;
                    field.push(c);
                }
                c => field.push(c),
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => {
                quoted = true;
                quote_line = line;
            }
            c if c == sep => record.push(std::mem::take(&mut field)),
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' => {
                line += 1;
                record.push(std::mem::take(&mut field));
                records.push(std::mem::take(&mut record));
            }
            c => field.push(c),
        }
    }
    if quoted {
        return Err(DataError::Unterminated(quote_line));
    }
    if !field.is_empty() || !record.is_empty() {
        record.push(field);
        records.push(record);
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_with_quotes_and_crlf() {
        let rows =
            parse("name,city,note\r\nBudi,Jakarta,\"says \"\"hi\"\", ok\"\r\n\"Siti\",Bandung,\"two\nlines\"\r\n")
                .unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][2], ("note".into(), "says \"hi\", ok".into()));
        assert_eq!(rows[1][0].1, "Siti");
        assert_eq!(rows[1][2].1, "two\nlines");
    }

    #[test]
    fn semicolon_csv_and_missing_cells() {
        let rows = parse("a;b\n1\n").unwrap();
        assert_eq!(rows, vec![vec![("a".to_string(), "1".to_string()), ("b".to_string(), String::new())]]);
        assert!(matches!(parse("a\n\"open"), Err(DataError::Unterminated(2))));
    }

    #[test]
    fn json_values_become_strings() {
        let rows = parse(r#"[{"id": 1, "ok": true, "tags": ["x"], "name": "Budi", "none": null}]"#).unwrap();
        assert_eq!(
            rows[0],
            vec![
                ("id".to_string(), "1".to_string()),
                ("ok".into(), "true".into()),
                ("tags".into(), "[\"x\"]".into()),
                ("name".into(), "Budi".into()),
                ("none".into(), String::new()),
            ]
        );
        assert!(matches!(parse("[1]"), Err(DataError::NotArray)));
    }
}
