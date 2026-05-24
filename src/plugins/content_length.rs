use crate::models::api_key::ApiKey;
use crate::providers::{ChatCompletionRequest, ChatCompletionResponse};
use async_trait::async_trait;

use super::{PluginError, RequestPlugin};

/// Rejects requests whose total message character count exceeds a configured limit.
///
/// Controlled by `plugins.max_prompt_chars` in velox.toml.
/// A value of 0 (default) disables the limit entirely.
pub struct ContentLengthPlugin {
    pub max_chars: usize,
}

#[async_trait]
impl RequestPlugin for ContentLengthPlugin {
    fn name(&self) -> &'static str {
        "content_length"
    }

    async fn before_request(
        &self,
        request: &mut ChatCompletionRequest,
        _api_key: &ApiKey,
    ) -> Result<(), PluginError> {
        if self.max_chars == 0 {
            return Ok(());
        }
        let total_chars: usize = request
            .messages
            .iter()
            .map(|m| m.content.as_str().map(str::len).unwrap_or(0))
            .sum();
        if total_chars > self.max_chars {
            return Err(PluginError::BadRequest(format!(
                "Prompt too long: {total_chars} chars exceeds limit of {}",
                self.max_chars
            )));
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
