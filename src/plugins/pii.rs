use crate::models::api_key::ApiKey;
use crate::providers::{ChatCompletionRequest, ChatCompletionResponse};
use async_trait::async_trait;

use super::{PluginError, RequestPlugin};

/// Scrubs PII from all message content fields before the request leaves the gateway.
///
/// Redacts credit cards, SSNs, email addresses, bearer tokens, and API key patterns.
/// Controlled by `plugins.pii_redaction` in velox.toml (default: true).
pub struct PiiRedactionPlugin;

#[async_trait]
impl RequestPlugin for PiiRedactionPlugin {
    fn name(&self) -> &'static str {
        "pii_redaction"
    }

    async fn before_request(
        &self,
        request: &mut ChatCompletionRequest,
        _api_key: &ApiKey,
    ) -> Result<(), PluginError> {
        for message in &mut request.messages {
            if let Some(s) = message.content.as_str() {
                let scrubbed = crate::pii::scrub(s);
                if scrubbed.as_ref() != s {
                    message.content = serde_json::Value::String(scrubbed.into_owned());
                }
            }
        }
        Ok(())
    }

    async fn after_response(
        &self,
        _request: &ChatCompletionRequest,
        _response: &mut ChatCompletionResponse,
        _api_key: &ApiKey,
    ) -> Result<(), PluginError> {
        Ok(())
    }
}
