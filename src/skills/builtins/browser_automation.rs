use serde_json::{json, Value};

pub fn handle(
    operation: &str,
    url: Option<&str>,
    selector: Option<&str>,
    output_path: Option<&str>,
) -> Value {
    match operation {
        "navigate" => json!({
            "status": "ok",
            "operation": "navigate",
            "url": url,
        }),
        "click" => json!({
            "status": "ok",
            "operation": "click",
            "selector": selector,
        }),
        "scrape" => json!({
            "status": "ok",
            "operation": "scrape",
            "url": url,
            "selector": selector,
            "content": "",
        }),
        "screenshot" => json!({
            "status": "ok",
            "operation": "screenshot",
            "url": url,
            "output": output_path,
        }),
        _ => json!({"error": format!("Unknown operation: {operation:?}")}),
    }
}

#[cfg(test)]
mod tests {
    use super::handle;

    #[test]
    fn navigate_returns_ok_payload() {
        let payload = handle("navigate", Some("https://example.com"), None, None);
        assert_eq!(payload["status"], "ok");
        assert_eq!(payload["operation"], "navigate");
    }

    #[test]
    fn scrape_returns_content_field() {
        let payload = handle("scrape", Some("https://example.com"), Some("h1"), None);
        assert_eq!(payload["status"], "ok");
        assert!(payload.get("content").is_some());
    }

    #[test]
    fn unknown_operation_returns_error() {
        let payload = handle("hover", None, None, None);
        assert!(payload.get("error").is_some());
    }
}
