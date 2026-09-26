//! Anthropic provider — optional alternative to OpenAI for editorial selection.

use super::openai::map_error;
use anyhow::{anyhow, Result};
use serde_json::json;
use std::time::Duration;

const VERSION: &str = "2023-06-01";
/// Current models think before answering; a 12-minute transcript window can
/// take a while.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(180);
/// Server-side fallback: a request the model's safety classifiers decline is
/// re-run on Anthropic's recommended substitute instead of failing.
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

/// Model families that accept `fallbacks: "default"`.
fn supports_fallbacks(model: &str) -> bool {
    model.starts_with("claude-opus-5") || model.starts_with("claude-fable-5")
}

fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()?)
}

pub async fn complete(key: &str, model: &str, system: &str, user: &str) -> Result<String> {
    let client = client()?;
    // No `temperature`: current models reject sampling parameters.
    let mut body = json!({
        "model": model,
        "max_tokens": 16000,
        "system": system,
        "messages": [ { "role": "user", "content": user } ]
    });
    let mut req = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", key)
        .header("anthropic-version", VERSION);
    if supports_fallbacks(model) {
        body["fallbacks"] = json!("default");
        req = req.header("anthropic-beta", FALLBACK_BETA);
    }
    let resp = req
        .json(&body)
        .send()
        .await
        .map_err(|e| anyhow!("Could not reach Anthropic: {}", e))?;

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(map_error(status.as_u16(), &text, "Anthropic"));
    }
    let v: serde_json::Value = serde_json::from_str(&text)
        .map_err(|_| anyhow!("Anthropic returned an unreadable response. Retry the stage."))?;
    response_text(&v)
}

/// The answer text of a Messages API response. Thinking blocks can come
/// first, so the text block is found by type, and a refusal is an error
/// rather than an empty answer.
fn response_text(v: &serde_json::Value) -> Result<String> {
    if v["stop_reason"] == "refusal" {
        return Err(anyhow!(
            "Claude declined to rank this transcript. Retry, or use local ranking."
        ));
    }
    v["content"]
        .as_array()
        .and_then(|blocks| blocks.iter().find(|b| b["type"] == "text"))
        .and_then(|b| b["text"].as_str())
        .map(str::to_string)
        .ok_or_else(|| anyhow!("Anthropic response had no content. Retry the stage."))
}

pub async fn test(key: &str) -> Result<()> {
    let client = client()?;
    let resp = client
        .get("https://api.anthropic.com/v1/models")
        .header("x-api-key", key)
        .header("anthropic-version", VERSION)
        .send()
        .await
        .map_err(|e| anyhow!("Could not reach Anthropic: {}", e))?;
    let status = resp.status();
    if status.is_success() {
        Ok(())
    } else {
        let text = resp.text().await.unwrap_or_default();
        Err(map_error(status.as_u16(), &text, "Anthropic"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answer_text_skips_thinking_blocks() {
        let v = json!({
            "stop_reason": "end_turn",
            "content": [
                {"type": "thinking", "thinking": ""},
                {"type": "text", "text": "[{\"start\": 1}]"}
            ]
        });
        assert_eq!(response_text(&v).unwrap(), "[{\"start\": 1}]");
    }

    #[test]
    fn a_refusal_is_an_error_not_an_empty_answer() {
        let v = json!({"stop_reason": "refusal", "content": []});
        assert!(response_text(&v)
            .unwrap_err()
            .to_string()
            .contains("declined"));
    }

    #[test]
    fn fallbacks_only_go_to_models_that_accept_them() {
        assert!(supports_fallbacks("claude-opus-5"));
        assert!(!supports_fallbacks("claude-sonnet-4-5"));
    }
}
