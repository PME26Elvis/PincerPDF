#![forbid(unsafe_code)]
//! Small Rust/WASM bridge to the trusted Tauri command boundary.

use pincerpdf_desktop_api::CommandError;
use serde::Serialize;
use serde::de::DeserializeOwned;
use wasm_bindgen::JsValue;
use wasm_bindgen::prelude::wasm_bindgen;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(catch, js_namespace = ["window", "__TAURI__", "core"], js_name = invoke)]
    async fn invoke_without_args(command: &str) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch, js_namespace = ["window", "__TAURI__", "core"], js_name = invoke)]
    async fn invoke_with_args(command: &str, args: JsValue) -> Result<JsValue, JsValue>;
}

pub(crate) fn is_tauri() -> bool {
    let Some(window) = web_sys::window() else {
        return false;
    };
    js_sys::Reflect::get(window.as_ref(), &JsValue::from_str("__TAURI__"))
        .is_ok_and(|value| !value.is_null() && !value.is_undefined())
}

pub(crate) async fn call_without_args<T: DeserializeOwned>(
    command: &str,
) -> Result<T, CommandError> {
    let value = invoke_without_args(command)
        .await
        .map_err(command_rejection)?;
    decode(value)
}

pub(crate) async fn call_with_args<T: DeserializeOwned, A: Serialize>(
    command: &str,
    args: &A,
) -> Result<T, CommandError> {
    let json = serde_json::to_string(args)
        .map_err(|error| bridge_error(format!("Cannot encode command arguments: {error}")))?;
    let value = js_sys::JSON::parse(&json)
        .map_err(|error| bridge_error(format!("Cannot prepare command arguments: {error:?}")))?;
    let value = invoke_with_args(command, value)
        .await
        .map_err(command_rejection)?;
    decode(value)
}

fn decode<T: DeserializeOwned>(value: JsValue) -> Result<T, CommandError> {
    let json = js_sys::JSON::stringify(&value)
        .map_err(|error| bridge_error(format!("Cannot read command response: {error:?}")))?
        .as_string()
        .ok_or_else(|| bridge_error("The command returned non-text JSON."))?;
    serde_json::from_str(&json)
        .map_err(|error| bridge_error(format!("Cannot decode command response: {error}")))
}

fn command_rejection(value: JsValue) -> CommandError {
    if let Ok(json) = js_sys::JSON::stringify(&value)
        && let Some(json) = json.as_string()
        && let Ok(error) = serde_json::from_str(&json)
    {
        return error;
    }
    if let Some(message) = value.as_string() {
        return CommandError::new("native_command_failed", message);
    }
    bridge_error("The native command failed without a readable error.")
}

fn bridge_error(message: impl Into<String>) -> CommandError {
    CommandError::new("bridge_error", message)
}
