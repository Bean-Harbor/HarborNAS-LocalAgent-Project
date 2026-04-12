//! Feishu (飞书) connectivity test binary.
//!
//! Tests the Feishu Open API by:
//! 1. Acquiring a tenant_access_token
//! 2. Querying bot info to verify the token works
//!
//! Usage:
//!   feishu-connectivity --app-id <ID> --app-secret <SECRET> [--domain https://open.feishu.cn]

use clap::Parser;
use serde_json::{json, Value};

#[derive(Debug, Parser)]
#[command(name = "feishu-connectivity")]
#[command(about = "Test Feishu Open API connectivity")]
struct Cli {
    #[arg(long, help = "Feishu app_id")]
    app_id: String,

    #[arg(long, help = "Feishu app_secret")]
    app_secret: String,

    #[arg(
        long,
        default_value = "https://open.feishu.cn",
        help = "Feishu Open API domain"
    )]
    domain: String,
}

fn main() {
    let cli = Cli::parse();

    println!("=== Feishu Connectivity Test ===");
    println!("Domain : {}", cli.domain);
    println!("App ID : {}", cli.app_id);
    println!();

    // Step 1: Get tenant_access_token
    println!("[1/3] Requesting tenant_access_token ...");
    let token_url = format!(
        "{}/open-apis/auth/v3/tenant_access_token/internal",
        cli.domain.trim_end_matches('/')
    );

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap_or_else(|e| {
            eprintln!("FAIL: could not build HTTP client: {e}");
            std::process::exit(1);
        });

    let body = json!({
        "app_id": cli.app_id,
        "app_secret": cli.app_secret,
    });

    let resp = match client.post(&token_url).json(&body).send() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("FAIL: HTTP request failed: {e}");
            std::process::exit(1);
        }
    };

    let status = resp.status();
    let resp_body: Value = match resp.json() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("FAIL: could not parse JSON response (HTTP {status}): {e}");
            std::process::exit(1);
        }
    };

    let code = resp_body.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
    if code != 0 {
        let msg = resp_body
            .get("msg")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        eprintln!("FAIL: Feishu returned error code={code}, msg=\"{msg}\"");
        eprintln!("  Full response: {resp_body}");
        std::process::exit(1);
    }

    let token = resp_body
        .get("tenant_access_token")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let expire = resp_body
        .get("expire")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    println!(
        "  OK — token acquired (expires in {expire}s, length={})",
        token.len()
    );

    // Step 2: Verify token by querying bot info
    println!("[2/3] Querying bot info to verify token ...");
    let bot_url = format!("{}/open-apis/bot/v3/info", cli.domain.trim_end_matches('/'));

    let bot_resp = match client
        .get(&bot_url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("FAIL: bot info request failed: {e}");
            std::process::exit(1);
        }
    };

    let bot_status = bot_resp.status();
    let bot_body: Value = match bot_resp.json() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("FAIL: could not parse bot info response (HTTP {bot_status}): {e}");
            std::process::exit(1);
        }
    };

    let bot_code = bot_body.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
    if bot_code == 0 {
        let bot = bot_body.get("bot").unwrap_or(&Value::Null);
        let bot_name = bot
            .get("bot_name")
            .and_then(|v| v.as_str())
            .unwrap_or("(unknown)");
        let open_id = bot
            .get("open_id")
            .and_then(|v| v.as_str())
            .unwrap_or("(none)");
        let app_name = bot
            .get("app_name")
            .and_then(|v| v.as_str())
            .unwrap_or("(unknown)");
        println!("  OK — Bot Name: {bot_name}");
        println!("       App Name: {app_name}");
        println!("       Open ID : {open_id}");
    } else {
        let msg = bot_body
            .get("msg")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        println!("  WARN: bot info returned code={bot_code}, msg=\"{msg}\"");
        println!("  (Token is valid but bot may not be enabled or this app has no bot capability)");
    }

    // Step 3: Summary
    println!("[3/3] Connectivity summary");
    println!("  tenant_access_token : OK");
    println!(
        "  bot_info            : {}",
        if bot_code == 0 {
            "OK"
        } else {
            "DEGRADED (no bot)"
        }
    );
    println!();
    println!("=== Feishu connectivity test PASSED ===");
}
