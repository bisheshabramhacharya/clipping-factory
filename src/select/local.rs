//! Local OpenAI-compatible provider (Ollama, llama.cpp, LM Studio). The
//! endpoint runs on the user's machine, so nothing leaves it and no
//! credential is ever sent — a stored cloud key must not leak into a
//! user-entered base URL.

use super::openai::map_error;
use anyhow::{anyhow, Result};
use serde_json::json;
use std::time::Duration;

/// Small local models generate slowly, especially on CPU.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(600);
const TEST_TIMEOUT: Duration = Duration::from_secs(15);

fn client(timeout: Duration) -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder().timeout(timeout).build()?)
}

fn endpoint(base_url: &str, path: &str) -> String {
    format!("{}/{}", base_url.trim_end_matches('/'), path)
}

fn unreachable(base_url: &str, e: reqwest::Error) -> anyhow::Error {
    anyhow!("Could not reach the local endpoint at {}: {}", base_url, e)
}

pub async fn complete(base_url: &str, model: &str, system: &str, user: &str) -> Result<String> {
    let client = client(REQUEST_TIMEOUT)?;
    let body = json!({
        "model": model,
        "temperature": 0.3,
        "response_format": { "type": "json_object" },
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user }
        ]
    });
    let resp = client
        .post(endpoint(base_url, "chat/completions"))
        .json(&body)
        .send()
        .await
        .map_err(|e| unreachable(base_url, e))?;

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(map_error(status.as_u16(), &text, "The local endpoint"));
    }
    let v: serde_json::Value = serde_json::from_str(&text)
        .map_err(|_| anyhow!("The local endpoint returned an unreadable response."))?;
    let content = v["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| anyhow!("The local endpoint response had no content."))?;
    Ok(content.to_string())
}

/// GET `{base}/models`. When the listing is parseable, verify the configured
/// model is actually served so a typo fails here rather than mid-pipeline.
pub async fn test(base_url: &str, model: &str) -> Result<()> {
    let client = client(TEST_TIMEOUT)?;
    let resp = client
        .get(endpoint(base_url, "models"))
        .send()
        .await
        .map_err(|e| unreachable(base_url, e))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(map_error(status.as_u16(), &text, "The local endpoint"));
    }
    let ids: Vec<String> = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| v["data"].as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|m| m["id"].as_str().map(str::to_string))
        .collect();
    if !ids.is_empty() && !ids.iter().any(|id| id == model) {
        let known = ids.iter().take(6).cloned().collect::<Vec<_>>().join(", ");
        return Err(anyhow!(
            "The endpoint is reachable but does not serve `{}`. Available: {}. Pull the model first (e.g. `ollama pull {}`) or fix the name.",
            model,
            known,
            model
        ));
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod test_server {
    /// Minimal OpenAI-compatible stub on a loopback port: GET `…/models`
    /// lists `models`, POST `…/chat/completions` answers with `content` as the
    /// assistant message. Returns the base URL (with `/v1` suffix).
    pub(crate) async fn spawn(models: Vec<String>, content: String) -> String {
        spawn_with_requests(models, content).await.0
    }

    /// Like `spawn`, but also returns a shared log of the raw request bodies
    /// the stub received, so tests can assert on what the provider was sent.
    pub(crate) async fn spawn_with_requests(
        models: Vec<String>,
        content: String,
    ) -> (String, std::sync::Arc<tokio::sync::Mutex<Vec<String>>>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let requests = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::<String>::new()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let log = requests.clone();
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                let models = models.clone();
                let content = content.clone();
                let log = log.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 65536];
                    let mut filled = 0;
                    let request = loop {
                        let n = match sock.read(&mut buf[filled..]).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => n,
                        };
                        filled += n;
                        let text = String::from_utf8_lossy(&buf[..filled]).into_owned();
                        let complete = text.find("\r\n\r\n").is_some_and(|head| {
                            let len = text[..head]
                                .lines()
                                .find_map(|l| {
                                    l.to_ascii_lowercase()
                                        .strip_prefix("content-length:")
                                        .and_then(|v| v.trim().parse::<usize>().ok())
                                })
                                .unwrap_or(0);
                            filled >= head + 4 + len
                        });
                        if complete {
                            break text;
                        }
                    };
                    log.lock().await.push(request.clone());
                    let payload = if request.starts_with("POST") {
                        serde_json::json!({
                            "choices": [{
                                "message": { "role": "assistant", "content": content }
                            }]
                        })
                    } else {
                        serde_json::json!({
                            "data": models
                                .iter()
                                .map(|m| serde_json::json!({ "id": m }))
                                .collect::<Vec<_>>()
                        })
                    }
                    .to_string();
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                        payload.len(),
                        payload
                    );
                    sock.write_all(response.as_bytes()).await.ok();
                });
            }
        });
        (format!("http://127.0.0.1:{port}/v1"), requests)
    }
}
