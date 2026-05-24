// tests/v2/common.rs
// V2-specific test helpers. Re-uses the V1 `tests/common` infrastructure.
#![allow(dead_code)]

pub use crate::common::{
    auth_header, fake_openai_response_json, minimal_chat_request, spawn_app,
    spawn_app_with_openai_base, test_api_key,
};
