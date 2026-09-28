//! OpenAI provider (PRD §14: "OpenAI API for candidate selection, using the
//! user's key"). The key is used for the request and never logged.

use anyhow::{anyhow, Result};
use serde_json::json;
use std::time::Duration;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const COMPLETIONS_URL: &str = "https://api.openai.com/v1/chat/completions";
const MODELS_URL: &str = "https://api.openai.com/v1/models";

fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()?)
}

/// JSON body for an OpenAI-compatible chat completion.
pub(crate) fn chat_body(model: &str, system: &str, user: &str) -> serde_json::Value {
    json!({
        "model": model,
        "temperature": 0.3,
        "response_format": { "type": "json_object" },
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user }
        ]
    })
}

/// POST an OpenAI-compatible chat completion and unwrap the assistant text.
/// `provider` names the endpoint in error messages ("OpenAI", "the local
/// endpoint at …").
pub(crate) async fn chat_completion(
    client: reqwest::Client,
    url: &str,
    bearer: Option<&str>,
    body: &serde_json::Value,
    provider: &str,
) -> Result<String> {
    let mut req = client.post(url);
    if let Some(key) = bearer {
        req = req.bearer_auth(key);
    }
    let resp = req
        .json(body)
        .send()
        .await
        .map_err(|e| anyhow!("Could not reach {provider}: {e}"))?;

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(map_error(status.as_u16(), &text, provider));
    }
    let v: serde_json::Value = serde_json::from_str(&text)
        .map_err(|_| anyhow!("{provider} returned an unreadable response. Retry the stage."))?;
    v["choices"][0]["message"]["content"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| anyhow!("{provider} response had no content. Retry the stage."))
}

pub async fn complete(key: &str, model: &str, system: &str, user: &str) -> Result<String> {
    chat_completion(
        client()?,
        COMPLETIONS_URL,
        Some(key),
        &chat_body(model, system, user),
        "OpenAI",
    )
    .await
}

/// GET a provider's model listing: a successful status means the key works.
pub(crate) async fn check_models(req: reqwest::RequestBuilder, provider: &str) -> Result<()> {
    let resp = req
        .send()
        .await
        .map_err(|e| anyhow!("Could not reach {provider}: {e}"))?;
    let status = resp.status();
    if status.is_success() {
        Ok(())
    } else {
        let text = resp.text().await.unwrap_or_default();
        Err(map_error(status.as_u16(), &text, provider))
    }
}

pub async fn test(key: &str) -> Result<()> {
    check_models(client()?.get(MODELS_URL).bearer_auth(key), "OpenAI").await
}

pub fn map_error(status: u16, body: &str, provider: &str) -> anyhow::Error {
    let snippet: String = body.chars().take(240).collect();
    match status {
        401 | 403 => anyhow!(
            "{} rejected the API key ({}). Open AI connection and check the key.",
            provider,
            status
        ),
        429 => anyhow!(
            "{} rate limit reached (429). Wait a moment, then retry the stage.",
            provider
        ),
        400 => anyhow!(
            "{} rejected the request (400). The model name may be wrong. Details: {}",
            provider,
            snippet
        ),
        404 => anyhow!(
            "{} says the model was not found (404). Check the model name in AI connection.",
            provider
        ),
        s if s >= 500 => anyhow!(
            "{} had a server error ({}). Retry the stage shortly.",
            provider,
            s
        ),
        s => anyhow!(
            "{} returned an unexpected error ({}): {}",
            provider,
            s,
            snippet
        ),
    }
}
