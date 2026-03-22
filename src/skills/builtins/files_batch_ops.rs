use serde_json::{json, Value};

pub fn handle(
    operation: &str,
    source: &str,
    destination: Option<&str>,
    pattern: Option<&str>,
) -> Value {
    match operation {
        "search" => json!({
            "status": "ok",
            "operation": "search",
            "source": source,
            "pattern": pattern,
            "matches": [],
        }),
        "copy" => json!({
            "status": "ok",
            "operation": "copy",
            "source": source,
            "destination": destination,
        }),
        "move" => json!({
            "status": "ok",
            "operation": "move",
            "source": source,
            "destination": destination,
        }),
        "archive" => json!({
            "status": "ok",
            "operation": "archive",
            "source": source,
            "destination": destination,
        }),
        _ => json!({"error": format!("Unknown operation: {operation:?}")}),
    }
}

#[cfg(test)]
mod tests {
    use super::handle;

    #[test]
    fn search_returns_ok_payload() {
        let payload = handle("search", "/pool/data", None, Some("*.mp4"));
        assert_eq!(payload["status"], "ok");
        assert_eq!(payload["operation"], "search");
    }

    #[test]
    fn copy_returns_ok_payload() {
        let payload = handle("copy", "/a", Some("/b"), None);
        assert_eq!(payload["status"], "ok");
        assert_eq!(payload["operation"], "copy");
    }

    #[test]
    fn unknown_operation_returns_error() {
        let payload = handle("delete", "/x", None, None);
        assert!(payload.get("error").is_some());
    }
}
