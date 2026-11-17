//! Import OpenAPI 3 documents as agent capabilities.
//!
//! Backends that already describe their API (FastAPI: `/openapi.json`, Spring Boot with
//! springdoc: `/v3/api-docs`, ASP.NET, NestJS, ...) become usable by agents without any
//! extra code: every operation turns into a tool with a JSON input schema.

use openshell_config::{OpenApiConfig, Permission};
use serde_json::{json, Map, Value};

#[derive(Debug, Clone)]
pub struct ApiOperation {
    /// Service the operation belongs to.
    pub service: String,
    pub name: String,
    pub description: String,
    pub permission: Permission,
    pub method: String,
    pub base_url: String,
    pub path: String,
    pub path_params: Vec<String>,
    pub query_params: Vec<String>,
    /// `Some(true)` = object body (input properties), `Some(false)` = whole body in `body`.
    pub body: Option<bool>,
    pub input_schema: Value,
}

/// `listTodoItems` / `list-todo items` -> `list_todo_items`
pub fn snake(s: &str) -> String {
    let mut out = String::new();
    let mut prev_lower = false;
    for c in s.chars() {
        if c.is_ascii_uppercase() {
            if prev_lower {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
            prev_lower = false;
        } else if c.is_ascii_alphanumeric() {
            out.push(c);
            prev_lower = c.is_ascii_lowercase() || c.is_ascii_digit();
        } else {
            if !out.ends_with('_') && !out.is_empty() {
                out.push('_');
            }
            prev_lower = false;
        }
    }
    let out = out.trim_matches('_').to_string();
    if out.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(true) {
        format!("op_{out}")
    } else {
        out
    }
}

/// Inline `$ref`s pointing into `#/components/...` (bounded depth, cycles are cut).
fn resolve(doc: &Value, v: &Value, depth: usize) -> Value {
    if depth > 8 {
        return json!({});
    }
    match v {
        Value::Object(o) => {
            if let Some(Value::String(r)) = o.get("$ref") {
                if let Some(ptr) = r.strip_prefix('#') {
                    if let Some(target) = doc.pointer(ptr) {
                        return resolve(doc, target, depth + 1);
                    }
                }
                return json!({});
            }
            Value::Object(o.iter().map(|(k, v)| (k.clone(), resolve(doc, v, depth + 1))).collect())
        }
        Value::Array(a) => Value::Array(a.iter().map(|v| resolve(doc, v, depth + 1)).collect()),
        other => other.clone(),
    }
}

fn matches_any(list: &[String], op_id: &str, path: &str) -> bool {
    list.iter().any(|p| op_id.contains(p.as_str()) || path.contains(p.as_str()))
}

/// Convert an OpenAPI 3 document into operations.
pub fn operations(doc: &Value, service: &str, base_url: &str, cfg: &OpenApiConfig) -> Vec<ApiOperation> {
    let mut out: Vec<ApiOperation> = Vec::new();
    let Some(paths) = doc.get("paths").and_then(|p| p.as_object()) else {
        return out;
    };
    let prefix = cfg.prefix.as_deref().map(snake).map(|p| format!("{p}_")).unwrap_or_default();
    for (path, item) in paths {
        let Some(item) = item.as_object() else { continue };
        let shared_params = item.get("parameters").cloned().unwrap_or(json!([]));
        for method in ["get", "post", "put", "patch", "delete"] {
            let Some(op) = item.get(method) else { continue };
            let op_id = op.get("operationId").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if !cfg.include.is_empty() && !matches_any(&cfg.include, &op_id, path) {
                continue;
            }
            if matches_any(&cfg.exclude, &op_id, path) {
                continue;
            }
            let base_name = if op_id.is_empty() { snake(&format!("{method} {path}")) } else { snake(&op_id) };
            let mut name = format!("{prefix}{base_name}");
            if out.iter().any(|o| o.name == name) {
                name = format!("{name}_{method}");
            }
            let description = [op.get("summary"), op.get("description")]
                .iter()
                .filter_map(|v| v.and_then(|v| v.as_str()))
                .collect::<Vec<_>>()
                .join(" – ");
            let description = if description.is_empty() {
                format!("{} {path}", method.to_uppercase())
            } else {
                format!("{description} ({} {path})", method.to_uppercase())
            };

            let mut properties = Map::new();
            let mut required: Vec<String> = Vec::new();
            let mut path_params = Vec::new();
            let mut query_params = Vec::new();
            let params = resolve(doc, &json!([shared_params, op.get("parameters").cloned().unwrap_or(json!([]))]), 0);
            for p in params.as_array().into_iter().flatten().flat_map(|a| a.as_array().cloned().unwrap_or_default()) {
                let (Some(pname), Some(location)) = (p.get("name").and_then(|v| v.as_str()), p.get("in").and_then(|v| v.as_str())) else {
                    continue;
                };
                let mut schema = p.get("schema").cloned().unwrap_or(json!({ "type": "string" }));
                if let (Some(d), Some(o)) = (p.get("description").and_then(|d| d.as_str()), schema.as_object_mut()) {
                    o.insert("description".into(), json!(d));
                }
                match location {
                    "path" => {
                        path_params.push(pname.to_string());
                        required.push(pname.to_string());
                    }
                    "query" => {
                        query_params.push(pname.to_string());
                        if p.get("required").and_then(|r| r.as_bool()).unwrap_or(false) {
                            required.push(pname.to_string());
                        }
                    }
                    _ => continue, // headers / cookies are not exposed
                }
                properties.insert(pname.to_string(), schema);
            }
            let mut body = None;
            if let Some(content) = op.pointer("/requestBody/content") {
                let schema = content
                    .get("application/json")
                    .or_else(|| content.as_object().and_then(|c| c.values().next()))
                    .and_then(|c| c.get("schema"))
                    .map(|s| resolve(doc, s, 0))
                    .unwrap_or(json!({}));
                let body_required = op.pointer("/requestBody/required").and_then(|r| r.as_bool()).unwrap_or(false);
                match schema.get("properties").and_then(|p| p.as_object()) {
                    Some(props) => {
                        for (k, v) in props {
                            properties.entry(k.clone()).or_insert(v.clone());
                        }
                        for r in schema.get("required").and_then(|r| r.as_array()).into_iter().flatten() {
                            if let Some(r) = r.as_str() {
                                required.push(r.to_string());
                            }
                        }
                        body = Some(true);
                    }
                    None => {
                        properties.insert("body".into(), schema);
                        if body_required {
                            required.push("body".into());
                        }
                        body = Some(false);
                    }
                }
            }
            required.sort();
            required.dedup();
            out.push(ApiOperation {
                service: service.to_string(),
                name,
                description,
                permission: if method == "get" { Permission::Read } else { Permission::Actions },
                method: method.to_uppercase(),
                base_url: base_url.trim_end_matches('/').to_string(),
                path: path.clone(),
                path_params,
                query_params,
                body,
                input_schema: json!({ "type": "object", "properties": properties, "required": required }),
            });
        }
    }
    out
}

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

impl ApiOperation {
    /// Build URL, query and body for an input object.
    pub fn request(&self, input: &Value) -> (String, Vec<(String, String)>, Option<Value>) {
        let text = |v: &Value| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string());
        let mut path = self.path.clone();
        for p in &self.path_params {
            let v = input.get(p).map(text).unwrap_or_default();
            path = path.replace(&format!("{{{p}}}"), &encode(&v));
        }
        let query: Vec<(String, String)> = self
            .query_params
            .iter()
            .filter_map(|q| input.get(q).filter(|v| !v.is_null()).map(|v| (q.clone(), text(v))))
            .collect();
        let body = match self.body {
            Some(true) => {
                let mut obj = input.as_object().cloned().unwrap_or_default();
                for p in self.path_params.iter().chain(&self.query_params) {
                    obj.remove(p);
                }
                Some(Value::Object(obj))
            }
            Some(false) => input.get("body").cloned(),
            None => None,
        };
        (format!("{}{}", self.base_url, path), query, body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_fastapi_like_document() {
        let doc = json!({
            "openapi": "3.1.0",
            "paths": {
                "/items/{item_id}": {
                    "get": {
                        "operationId": "readItem",
                        "summary": "Read an item",
                        "parameters": [
                            { "name": "item_id", "in": "path", "required": true, "schema": { "type": "integer" } },
                            { "name": "verbose", "in": "query", "schema": { "type": "boolean" } }
                        ]
                    },
                    "put": {
                        "operationId": "update_item_items__item_id__put",
                        "parameters": [{ "name": "item_id", "in": "path", "required": true, "schema": { "type": "integer" } }],
                        "requestBody": { "required": true, "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Item" } } } }
                    }
                },
                "/health": { "get": { "summary": "Health" } }
            },
            "components": { "schemas": { "Item": {
                "type": "object", "required": ["name"],
                "properties": { "name": { "type": "string" }, "price": { "type": "number" } }
            } } }
        });
        let cfg = OpenApiConfig { location: "/openapi.json".into(), include: vec![], exclude: vec!["health".into()], prefix: None };
        let ops = operations(&doc, "backend", "http://127.0.0.1:8000/", &cfg);
        assert_eq!(ops.len(), 2);
        let read = ops.iter().find(|o| o.name == "read_item").unwrap();
        assert_eq!(read.permission, Permission::Read);
        assert_eq!(read.input_schema["required"], json!(["item_id"]));
        let (url, query, body) = read.request(&json!({ "item_id": 7, "verbose": true }));
        assert_eq!(url, "http://127.0.0.1:8000/items/7");
        assert_eq!(query, vec![("verbose".to_string(), "true".to_string())]);
        assert!(body.is_none());

        let update = ops.iter().find(|o| o.method == "PUT").unwrap();
        assert_eq!(update.name, "update_item_items_item_id_put");
        assert_eq!(update.permission, Permission::Actions);
        assert_eq!(update.input_schema["required"], json!(["item_id", "name"]));
        let (_, _, body) = update.request(&json!({ "item_id": 1, "name": "x", "price": 2 }));
        assert_eq!(body.unwrap(), json!({ "name": "x", "price": 2 }));
    }

    #[test]
    fn snake_case_names() {
        assert_eq!(snake("listTodoItems"), "list_todo_items");
        assert_eq!(snake("get /api/todos/{id}"), "get_api_todos_id");
        assert_eq!(snake("2fa-setup"), "op_2fa_setup");
    }
}
