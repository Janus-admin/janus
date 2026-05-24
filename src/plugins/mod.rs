use crate::models::api_key::ApiKey;
use crate::providers::{ChatCompletionRequest, ChatCompletionResponse};
use async_trait::async_trait;

pub mod content_length;
pub mod pii;

/// Error type returned by plugin hooks.
#[derive(Debug)]
pub enum PluginError {
    /// Abort the request with HTTP 400 and this message.
    BadRequest(String),
    /// Abort the request with HTTP 403 and this message.
    Forbidden(String),
    /// Log the error and continue (non-fatal).
    Warning(String),
}

/// A plugin that can inspect and modify requests and responses in the gateway pipeline.
///
/// Plugins are called in order for `before_request` and in reverse order for
/// `after_response`, mirroring a standard middleware stack. A fatal error from
/// any plugin aborts the rest of the chain and returns an HTTP error to the caller.
#[async_trait]
pub trait RequestPlugin: Send + Sync {
    fn name(&self) -> &'static str;

    /// Called after auth/rate-limit checks, before cache lookup.
    /// May mutate the request (e.g. redact PII) or abort with an error.
    async fn before_request(
        &self,
        request: &mut ChatCompletionRequest,
        api_key: &ApiKey,
    ) -> Result<(), PluginError>;

    /// Called after a successful provider response or cache hit.
    /// May mutate the response or record side effects.
    async fn after_response(
        &self,
        request: &ChatCompletionRequest,
        response: &mut ChatCompletionResponse,
        api_key: &ApiKey,
    ) -> Result<(), PluginError>;
}

/// Run `before_request` on every plugin in order.
///
/// Returns the first fatal error encountered. `Warning` errors are logged and
/// execution continues. The second plugin is not called if the first returns a
/// fatal error.
pub async fn run_before(
    plugins: &[Box<dyn RequestPlugin>],
    request: &mut ChatCompletionRequest,
    api_key: &ApiKey,
) -> Result<(), PluginError> {
    for plugin in plugins {
        match plugin.before_request(request, api_key).await {
            Ok(()) => {}
            Err(PluginError::Warning(msg)) => {
                tracing::warn!(plugin = plugin.name(), "Plugin warning: {msg}");
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Run `after_response` on every plugin in **reverse** order.
///
/// Returns the first fatal error. Warnings are logged and execution continues.
pub async fn run_after(
    plugins: &[Box<dyn RequestPlugin>],
    request: &ChatCompletionRequest,
    response: &mut ChatCompletionResponse,
    api_key: &ApiKey,
) -> Result<(), PluginError> {
    for plugin in plugins.iter().rev() {
        match plugin.after_response(request, response, api_key).await {
            Ok(()) => {}
            Err(PluginError::Warning(msg)) => {
                tracing::warn!(plugin = plugin.name(), "Plugin warning: {msg}");
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
