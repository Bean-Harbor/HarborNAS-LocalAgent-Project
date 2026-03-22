use serde_json::{json, Value};

pub fn handle(
    operation: &str,
    input_path: &str,
    output_path: Option<&str>,
    options: Option<&Value>,
) -> Value {
    let options = options.cloned().unwrap_or_else(|| json!({}));
    match operation {
        "trim" | "concat" | "transcode" | "thumbnail" => json!({
            "status": "ok",
            "operation": operation,
            "input": input_path,
            "output": output_path,
            "options": options,
        }),
        _ => json!({"error": format!("Unknown operation: {operation:?}")}),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::handle;

    #[test]
    fn trim_returns_ok_payload() {
        let payload = handle("trim", "/media/in.mp4", Some("/media/out.mp4"), None);
        assert_eq!(payload["status"], "ok");
        assert_eq!(payload["operation"], "trim");
    }

    #[test]
    fn transcode_keeps_options() {
        let payload = handle(
            "transcode",
            "/media/in.mp4",
            Some("/media/out.mov"),
            Some(&json!({"codec": "h264"})),
        );
        assert_eq!(payload["status"], "ok");
        assert_eq!(payload["options"]["codec"], "h264");
    }

    #[test]
    fn unknown_operation_returns_error() {
        let payload = handle("watermark", "/media/in.mp4", None, None);
        assert!(payload.get("error").is_some());
    }
}
