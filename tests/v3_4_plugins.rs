// tests/v3_4_plugins.rs
// Phase V3-4 acceptance tests — Plugin Middleware.
//
// Run with: cargo test v3_4
//
// All tests are pure unit tests — no DB, no HTTP server, no real providers.
// They verify the behavioral guarantees added in V3-4:
//
//   7.1  RequestPlugin trait: before_request / after_response hooks
//   7.2  Plugin chain ordering and error short-circuiting
//   7.3  Built-in plugins: PiiRedactionPlugin, ContentLengthPlugin

use velox::{
    models::api_key::ApiKey,
    plugins::{
        content_length::ContentLengthPlugin, pii::PiiRedactionPlugin, run_after, run_before,
        PluginError, RequestPlugin,
    },
    providers::{ChatCompletionRequest, ChatCompletionResponse, ChatMessage, UsageData},
};

use async_trait::async_trait;
use rust_decimal::Decimal;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn make_key() -> ApiKey {
    ApiKey {
        id: Uuid::new_v4(),
        workspace_id: None,
        name: "test-key".to_string(),
        key_hash: String::new(),
        key_sha256: None,
        previous_key_sha256: None,
        rotation_expires_at: None,
        key_prefix: "vx-sk-test".to_string(),
        is_active: true,
        budget_limit: None,
        budget_used: Decimal::ZERO,
        rate_limit_rpm: None,
        rate_limit_tpm: None,
        allowed_models: None,
        routing_strategy: "priority".to_string(),
        downgrade_at_percent: None,
        downgrade_strategy: None,
        downgrade_to_model: None,
        expires_at: None,
        last_used_at: None,
        created_at: chrono::Utc::now(),
    }
}

fn make_request(content: &str) -> ChatCompletionRequest {
    ChatCompletionRequest {
        model: "gpt-4o-mini".to_string(),
        messages: vec![ChatMessage {
            role: "user".to_string(),
            content: serde_json::Value::String(content.to_string()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }],
        max_tokens: None,
        temperature: None,
        stream: None,
        top_p: None,
        n: None,
        stop: None,
        presence_penalty: None,
        frequency_penalty: None,
        seed: None,
        user: None,
        logit_bias: None,
        tools: None,
        tool_choice: None,
        parallel_tool_calls: None,
        response_format: None,
    }
}

fn make_response() -> ChatCompletionResponse {
    ChatCompletionResponse {
        id: "chatcmpl-test".to_string(),
        object: "chat.completion".to_string(),
        created: 1_716_000_000,
        model: "gpt-4o-mini".to_string(),
        choices: vec![velox::providers::ChatChoice {
            index: 0,
            message: ChatMessage {
                role: "assistant".to_string(),
                content: serde_json::Value::String("Hello!".to_string()),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
            finish_reason: Some("stop".to_string()),
            logprobs: None,
        }],
        usage: UsageData {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
        },
    }
}

// ─── 7.1: Trait behavior ──────────────────────────────────────────────────────

/// A plugin that always returns BadRequest from before_request.
struct BadRequestPlugin;

#[async_trait]
impl RequestPlugin for BadRequestPlugin {
    fn name(&self) -> &'static str {
        "bad_request"
    }
    async fn before_request(
        &self,
        _req: &mut ChatCompletionRequest,
        _key: &ApiKey,
    ) -> Result<(), PluginError> {
        Err(PluginError::BadRequest("blocked by policy".to_string()))
    }
    async fn after_response(
        &self,
        _req: &ChatCompletionRequest,
        _resp: &mut ChatCompletionResponse,
        _key: &ApiKey,
    ) -> Result<(), PluginError> {
        Ok(())
    }
}

/// A plugin that always returns Forbidden.
struct ForbiddenPlugin;

#[async_trait]
impl RequestPlugin for ForbiddenPlugin {
    fn name(&self) -> &'static str {
        "forbidden"
    }
    async fn before_request(
        &self,
        _req: &mut ChatCompletionRequest,
        _key: &ApiKey,
    ) -> Result<(), PluginError> {
        Err(PluginError::Forbidden("key not allowed".to_string()))
    }
    async fn after_response(
        &self,
        _req: &ChatCompletionRequest,
        _resp: &mut ChatCompletionResponse,
        _key: &ApiKey,
    ) -> Result<(), PluginError> {
        Ok(())
    }
}

/// A plugin that emits a Warning and continues.
struct WarningPlugin;

#[async_trait]
impl RequestPlugin for WarningPlugin {
    fn name(&self) -> &'static str {
        "warning"
    }
    async fn before_request(
        &self,
        _req: &mut ChatCompletionRequest,
        _key: &ApiKey,
    ) -> Result<(), PluginError> {
        Err(PluginError::Warning("non-fatal hint".to_string()))
    }
    async fn after_response(
        &self,
        _req: &ChatCompletionRequest,
        _resp: &mut ChatCompletionResponse,
        _key: &ApiKey,
    ) -> Result<(), PluginError> {
        Ok(())
    }
}

/// A plugin that appends its name to a shared call log.
struct TracePlugin {
    name: &'static str,
    log: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl RequestPlugin for TracePlugin {
    fn name(&self) -> &'static str {
        self.name
    }
    async fn before_request(
        &self,
        _req: &mut ChatCompletionRequest,
        _key: &ApiKey,
    ) -> Result<(), PluginError> {
        self.log
            .lock()
            .unwrap()
            .push(format!("before:{}", self.name));
        Ok(())
    }
    async fn after_response(
        &self,
        _req: &ChatCompletionRequest,
        _resp: &mut ChatCompletionResponse,
        _key: &ApiKey,
    ) -> Result<(), PluginError> {
        self.log
            .lock()
            .unwrap()
            .push(format!("after:{}", self.name));
        Ok(())
    }
}

/// A plugin that appends a suffix to the response content in after_response.
struct ResponseModPlugin {
    suffix: &'static str,
}

#[async_trait]
impl RequestPlugin for ResponseModPlugin {
    fn name(&self) -> &'static str {
        "response_mod"
    }
    async fn before_request(
        &self,
        _req: &mut ChatCompletionRequest,
        _key: &ApiKey,
    ) -> Result<(), PluginError> {
        Ok(())
    }
    async fn after_response(
        &self,
        _req: &ChatCompletionRequest,
        resp: &mut ChatCompletionResponse,
        _key: &ApiKey,
    ) -> Result<(), PluginError> {
        if let Some(choice) = resp.choices.first_mut() {
            if let Some(s) = choice.message.content.as_str() {
                choice.message.content = serde_json::Value::String(format!("{}{}", s, self.suffix));
            }
        }
        Ok(())
    }
}

#[tokio::test]
async fn v3_4_before_request_bad_request_aborts_pipeline() {
    let chain: Vec<Box<dyn RequestPlugin>> = vec![Box::new(BadRequestPlugin)];
    let mut req = make_request("hello");
    let key = make_key();
    let result = run_before(&chain, &mut req, &key).await;
    assert!(
        matches!(result, Err(PluginError::BadRequest(_))),
        "BadRequest error must propagate"
    );
}

#[tokio::test]
async fn v3_4_before_request_forbidden_returns_403() {
    let chain: Vec<Box<dyn RequestPlugin>> = vec![Box::new(ForbiddenPlugin)];
    let mut req = make_request("hello");
    let key = make_key();
    let result = run_before(&chain, &mut req, &key).await;
    assert!(
        matches!(result, Err(PluginError::Forbidden(_))),
        "Forbidden error must propagate"
    );
}

#[tokio::test]
async fn v3_4_before_request_warning_continues_pipeline() {
    let chain: Vec<Box<dyn RequestPlugin>> = vec![Box::new(WarningPlugin)];
    let mut req = make_request("hello");
    let key = make_key();
    // Warning is non-fatal: run_before logs it and returns Ok.
    let result = run_before(&chain, &mut req, &key).await;
    assert!(
        result.is_ok(),
        "Warning must not abort the pipeline — got {result:?}"
    );
}

#[tokio::test]
async fn v3_4_after_response_can_modify_response() {
    let chain: Vec<Box<dyn RequestPlugin>> = vec![Box::new(ResponseModPlugin { suffix: "!!!" })];
    let req = make_request("hi");
    let mut resp = make_response();
    let key = make_key();
    run_after(&chain, &req, &mut resp, &key).await.unwrap();
    let text = resp.choices[0]
        .message
        .content
        .as_str()
        .expect("content should be string");
    assert!(
        text.ends_with("!!!"),
        "after_response should have appended suffix, got: {text}"
    );
}

// ─── 7.2: Plugin chain ordering ───────────────────────────────────────────────

#[tokio::test]
async fn v3_4_plugins_called_in_order_before() {
    let log = Arc::new(Mutex::new(Vec::<String>::new()));
    let chain: Vec<Box<dyn RequestPlugin>> = vec![
        Box::new(TracePlugin {
            name: "a",
            log: log.clone(),
        }),
        Box::new(TracePlugin {
            name: "b",
            log: log.clone(),
        }),
        Box::new(TracePlugin {
            name: "c",
            log: log.clone(),
        }),
    ];
    let mut req = make_request("hello");
    let key = make_key();
    run_before(&chain, &mut req, &key).await.unwrap();
    let calls = log.lock().unwrap().clone();
    assert_eq!(
        calls,
        vec!["before:a", "before:b", "before:c"],
        "before_request must be called in chain order"
    );
}

#[tokio::test]
async fn v3_4_plugins_called_in_reverse_order_after() {
    let log = Arc::new(Mutex::new(Vec::<String>::new()));
    let chain: Vec<Box<dyn RequestPlugin>> = vec![
        Box::new(TracePlugin {
            name: "a",
            log: log.clone(),
        }),
        Box::new(TracePlugin {
            name: "b",
            log: log.clone(),
        }),
        Box::new(TracePlugin {
            name: "c",
            log: log.clone(),
        }),
    ];
    let req = make_request("hello");
    let mut resp = make_response();
    let key = make_key();
    run_after(&chain, &req, &mut resp, &key).await.unwrap();
    let calls = log.lock().unwrap().clone();
    assert_eq!(
        calls,
        vec!["after:c", "after:b", "after:a"],
        "after_response must be called in reverse order"
    );
}

#[tokio::test]
async fn v3_4_second_plugin_not_called_when_first_returns_error() {
    let log = Arc::new(Mutex::new(Vec::<String>::new()));
    let chain: Vec<Box<dyn RequestPlugin>> = vec![
        Box::new(BadRequestPlugin), // aborts chain
        Box::new(TracePlugin {
            name: "should_not_run",
            log: log.clone(),
        }),
    ];
    let mut req = make_request("hello");
    let key = make_key();
    let result = run_before(&chain, &mut req, &key).await;
    assert!(result.is_err(), "chain should have aborted");
    let calls = log.lock().unwrap().clone();
    assert!(
        calls.is_empty(),
        "second plugin must not be called after first returns error"
    );
}

// ─── 7.3: Built-in plugins ───────────────────────────────────────────────────

#[tokio::test]
async fn v3_4_pii_plugin_redacts_credit_card_in_request() {
    let plugin = PiiRedactionPlugin;
    let mut req = make_request("My card is 4111 1111 1111 1111 please charge it");
    let key = make_key();
    plugin.before_request(&mut req, &key).await.unwrap();
    let content = req.messages[0]
        .content
        .as_str()
        .expect("content should be string");
    assert!(
        !content.contains("4111"),
        "credit card number must be redacted, got: {content}"
    );
    assert!(
        content.contains("CC-REDACTED"),
        "redaction placeholder must be present"
    );
}

#[tokio::test]
async fn v3_4_pii_plugin_disabled_by_config_skips_redaction() {
    // When pii_redaction = false in config, the plugin is simply not added to the chain.
    // Simulate this by running an empty chain.
    let chain: Vec<Box<dyn RequestPlugin>> = vec![];
    let mut req = make_request("email me at alice@example.com");
    let key = make_key();
    run_before(&chain, &mut req, &key).await.unwrap();
    let content = req.messages[0]
        .content
        .as_str()
        .expect("content should be string");
    assert!(
        content.contains("alice@example.com"),
        "with no plugins, content should be unchanged"
    );
}

#[tokio::test]
async fn v3_4_content_length_plugin_rejects_oversized_prompt() {
    let plugin = ContentLengthPlugin { max_chars: 10 };
    let mut req = make_request("this message is definitely longer than ten characters");
    let key = make_key();
    let result = plugin.before_request(&mut req, &key).await;
    assert!(
        matches!(result, Err(PluginError::BadRequest(_))),
        "oversized prompt must return BadRequest"
    );
}

#[tokio::test]
async fn v3_4_content_length_zero_means_no_limit() {
    let plugin = ContentLengthPlugin { max_chars: 0 };
    let very_long = "x".repeat(1_000_000);
    let mut req = make_request(&very_long);
    let key = make_key();
    let result = plugin.before_request(&mut req, &key).await;
    assert!(
        result.is_ok(),
        "max_chars = 0 must impose no limit; got {result:?}"
    );
}

// ─── Regression ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn v3_4_no_plugins_configured_pipeline_works_unchanged() {
    let chain: Vec<Box<dyn RequestPlugin>> = vec![];
    let mut req = make_request("hello");
    let mut resp = make_response();
    let key = make_key();
    // Empty chain must not error.
    run_before(&chain, &mut req, &key).await.unwrap();
    run_after(&chain, &req, &mut resp, &key).await.unwrap();
    // Content unchanged.
    assert_eq!(resp.choices[0].message.content.as_str().unwrap(), "Hello!");
}

#[tokio::test]
async fn v3_4_regression_gateway_proxy_unaffected() {
    // Verify that the two built-in plugin types implement the trait correctly
    // and do not interfere with a normal, clean request.
    let pii = PiiRedactionPlugin;
    let len = ContentLengthPlugin { max_chars: 10_000 };

    let chain: Vec<Box<dyn RequestPlugin>> = vec![Box::new(pii), Box::new(len)];
    let mut req = make_request("What is 2 + 2?");
    let key = make_key();
    run_before(&chain, &mut req, &key).await.unwrap();

    // Content should be unchanged (no PII, under length limit).
    assert_eq!(req.messages[0].content.as_str().unwrap(), "What is 2 + 2?");
}
