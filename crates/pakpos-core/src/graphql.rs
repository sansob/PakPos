//! GraphQL schema introspection, used by the schema explorer in the app.

use serde_json::Value;

/// A compact introspection query: root operation fields with their
/// arguments and types, plus object types for drilling down.
pub const INTROSPECTION_QUERY: &str = r#"query PakPosIntrospection {
  __schema {
    queryType { name }
    mutationType { name }
    subscriptionType { name }
    types {
      kind
      name
      description
      fields {
        name
        description
        args { name type { ...TypeRef } defaultValue }
        type { ...TypeRef }
      }
    }
  }
}
fragment TypeRef on __Type {
  kind name
  ofType { kind name ofType { kind name ofType { kind name ofType { kind name } } } }
}"#;

/// A field of an object type (or a root operation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub description: String,
    pub args: Vec<(String, String)>,
    /// Rendered type, e.g. `[User!]!`.
    pub ty: String,
    /// Name of the underlying named type, e.g. `User`.
    pub base_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectType {
    pub name: String,
    pub description: String,
    pub fields: Vec<Field>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Schema {
    pub queries: Vec<Field>,
    pub mutations: Vec<Field>,
    pub subscriptions: Vec<Field>,
    pub types: Vec<ObjectType>,
}

impl Schema {
    pub fn object(&self, name: &str) -> Option<&ObjectType> {
        self.types.iter().find(|t| t.name == name)
    }
}

fn render_type(t: &Value) -> (String, String) {
    match t.get("kind").and_then(Value::as_str) {
        Some("NON_NULL") => {
            let (inner, base) = render_type(t.get("ofType").unwrap_or(&Value::Null));
            (format!("{inner}!"), base)
        }
        Some("LIST") => {
            let (inner, base) = render_type(t.get("ofType").unwrap_or(&Value::Null));
            (format!("[{inner}]"), base)
        }
        _ => {
            let name = t.get("name").and_then(Value::as_str).unwrap_or("?").to_string();
            (name.clone(), name)
        }
    }
}

fn fields_of(t: &Value) -> Vec<Field> {
    t.get("fields")
        .and_then(Value::as_array)
        .map(|fields| {
            fields
                .iter()
                .map(|f| {
                    let (ty, base_type) = render_type(f.get("type").unwrap_or(&Value::Null));
                    Field {
                        name: f.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
                        description: f.get("description").and_then(Value::as_str).unwrap_or("").to_string(),
                        args: f
                            .get("args")
                            .and_then(Value::as_array)
                            .map(|args| {
                                args.iter()
                                    .map(|a| {
                                        (
                                            a.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
                                            render_type(a.get("type").unwrap_or(&Value::Null)).0,
                                        )
                                    })
                                    .collect()
                            })
                            .unwrap_or_default(),
                        ty,
                        base_type,
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Parses an introspection response (`{"data": {"__schema": …}}`).
pub fn parse_schema(response: &Value) -> Result<Schema, String> {
    if let Some(errors) = response.get("errors").and_then(Value::as_array).filter(|e| !e.is_empty()) {
        let msgs: Vec<String> =
            errors.iter().filter_map(|e| e.get("message").and_then(Value::as_str)).map(str::to_string).collect();
        return Err(format!("introspection failed: {}", msgs.join("; ")));
    }
    let schema = response
        .pointer("/data/__schema")
        .ok_or_else(|| "not a GraphQL introspection response (introspection may be disabled)".to_string())?;
    let root = |key: &str| schema.pointer(&format!("/{key}/name")).and_then(Value::as_str).map(str::to_string);
    let types = schema.get("types").and_then(Value::as_array).cloned().unwrap_or_default();
    let find = |name: &Option<String>| -> Vec<Field> {
        name.as_ref()
            .and_then(|n| types.iter().find(|t| t.get("name").and_then(Value::as_str) == Some(n)))
            .map(fields_of)
            .unwrap_or_default()
    };
    let objects = types
        .iter()
        .filter(|t| matches!(t.get("kind").and_then(Value::as_str), Some("OBJECT" | "INTERFACE")))
        .filter(|t| !t.get("name").and_then(Value::as_str).unwrap_or("__").starts_with("__"))
        .map(|t| ObjectType {
            name: t.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
            description: t.get("description").and_then(Value::as_str).unwrap_or("").to_string(),
            fields: fields_of(t),
        })
        .collect();
    Ok(Schema {
        queries: find(&root("queryType")),
        mutations: find(&root("mutationType")),
        subscriptions: find(&root("subscriptionType")),
        types: objects,
    })
}

/// A starter query for a root field, with scalar sub-fields selected.
pub fn skeleton(schema: &Schema, operation: &str, field: &Field) -> String {
    let args = if field.args.is_empty() {
        String::new()
    } else {
        format!("({})", field.args.iter().map(|(n, _)| format!("{n}: ${n}")).collect::<Vec<_>>().join(", "))
    };
    let var_decls = if field.args.is_empty() {
        String::new()
    } else {
        format!("({})", field.args.iter().map(|(n, t)| format!("${n}: {t}")).collect::<Vec<_>>().join(", "))
    };
    let selection = schema
        .object(&field.base_type)
        .map(|o| {
            let scalars: Vec<&str> = o
                .fields
                .iter()
                .filter(|f| schema.object(&f.base_type).is_none() && f.args.is_empty())
                .map(|f| f.name.as_str())
                .take(8)
                .collect();
            if scalars.is_empty() {
                " { __typename }".to_string()
            } else {
                format!(" {{\n    {}\n  }}", scalars.join("\n    "))
            }
        })
        .unwrap_or_default();
    format!("{operation} {}{var_decls} {{\n  {}{args}{selection}\n}}\n", capitalize(&field.name), field.name)
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> Value {
        let named = |n: &str| json!({"kind": "SCALAR", "name": n, "ofType": null});
        let obj = |n: &str| json!({"kind": "OBJECT", "name": n, "ofType": null});
        json!({"data": {"__schema": {
        "queryType": {"name": "Query"}, "mutationType": {"name": "Mutation"}, "subscriptionType": null,
        "types": [
            {"kind": "OBJECT", "name": "Query", "fields": [
                {"name": "users", "description": "All users", "args": [
                    {"name": "first", "type": named("Int"), "defaultValue": null}],
                 "type": {"kind": "NON_NULL", "name": null, "ofType": {"kind": "LIST", "name": null, "ofType": obj("User")}}},
                {"name": "user", "args": [{"name": "id", "type": {"kind": "NON_NULL", "name": null, "ofType": named("ID")}}],
                 "type": obj("User")}]},
            {"kind": "OBJECT", "name": "Mutation", "fields": [
                {"name": "createUser", "args": [], "type": obj("User")}]},
            {"kind": "OBJECT", "name": "User", "fields": [
                {"name": "id", "args": [], "type": named("ID")},
                {"name": "name", "args": [], "type": named("String")},
                {"name": "friends", "args": [], "type": {"kind": "LIST", "name": null, "ofType": obj("User")}}]},
            {"kind": "OBJECT", "name": "__Type", "fields": []},
            {"kind": "SCALAR", "name": "String"}
        ]}}})
    }

    #[test]
    fn parses_roots_and_types() {
        let s = parse_schema(&sample()).unwrap();
        assert_eq!(s.queries.len(), 2);
        assert_eq!(s.queries[0].ty, "[User]!");
        assert_eq!(s.queries[0].base_type, "User");
        assert_eq!(s.queries[1].args, vec![("id".to_string(), "ID!".to_string())]);
        assert_eq!(s.mutations[0].name, "createUser");
        assert!(s.object("__Type").is_none());
        assert!(s.object("User").is_some());
    }

    #[test]
    fn skeleton_selects_scalars_and_declares_vars() {
        let s = parse_schema(&sample()).unwrap();
        let q = skeleton(&s, "query", &s.queries[1]);
        assert!(q.starts_with("query User($id: ID!) {"));
        assert!(q.contains("user(id: $id) {"));
        assert!(q.contains("id\n    name"));
        assert!(!q.contains("friends"));
    }

    #[test]
    fn reports_errors() {
        assert!(parse_schema(&json!({"errors": [{"message": "introspection disabled"}]})).is_err());
        assert!(parse_schema(&json!({"data": {}})).is_err());
    }
}
