use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
}

impl HttpRequest {
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            method: "GET".into(),
            url: url.into(),
            headers: vec![("accept".into(), "application/json".into())],
        }
    }

    pub fn bearer(mut self, token: &str) -> Self {
        self.headers
            .push(("authorization".into(), format!("Bearer {token}")));
        self
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
}

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("通信に失敗しました: {0}")]
    Transport(String),
}

#[async_trait]
pub trait HttpClient: Send + Sync {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, HttpError>;
}

pub struct ReqwestHttp {
    client: reqwest::Client,
}

impl ReqwestHttp {
    pub fn new() -> Result<Self, HttpError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("ai-quota/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|err| HttpError::Transport(err.to_string()))?;
        Ok(Self { client })
    }
}

#[async_trait]
impl HttpClient for ReqwestHttp {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, HttpError> {
        let mut builder = self.client.get(&request.url);
        for (name, value) in &request.headers {
            builder = builder.header(name, value);
        }
        let response = builder
            .send()
            .await
            .map_err(|err| HttpError::Transport(err.to_string()))?;
        let status = response.status().as_u16();
        let body = response
            .text()
            .await
            .map_err(|err| HttpError::Transport(err.to_string()))?;
        Ok(HttpResponse { status, body })
    }
}

pub fn explain_http(status: u16, body: &str) -> String {
    let detail = json_message(body).unwrap_or_else(|| flatten(body));
    let detail = truncate(&detail, 240);
    if status == 401 || status == 403 {
        format!("HTTP {status}（認証が拒否されました）: {detail}")
    } else if detail.is_empty() {
        format!("HTTP {status}")
    } else {
        format!("HTTP {status}: {detail}")
    }
}

fn json_message(body: &str) -> Option<String> {
    let value: Value = serde_json::from_str(body).ok()?;
    let message = value
        .pointer("/error/message")
        .and_then(Value::as_str)
        .or_else(|| value.get("message").and_then(Value::as_str))
        .or_else(|| value.get("error_msg").and_then(Value::as_str))?;
    Some(message.trim().to_string())
}

fn flatten(body: &str) -> String {
    body.chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn truncate(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_string();
    }
    let trimmed: String = text.chars().take(max_chars).collect();
    format!("{trimmed}…")
}

type MockHandler = dyn Fn(&HttpRequest) -> Result<HttpResponse, HttpError> + Send + Sync;

/// Test double that records requests and returns a scripted body.
pub struct MockHttp {
    handler: Arc<MockHandler>,
}

impl MockHttp {
    pub fn new<F>(handler: F) -> Self
    where
        F: Fn(&HttpRequest) -> Result<HttpResponse, HttpError> + Send + Sync + 'static,
    {
        Self {
            handler: Arc::new(handler),
        }
    }

    pub fn json(status: u16, url_part: impl Into<String>, body: impl Into<String>) -> Self {
        let url_part = url_part.into();
        let body = body.into();
        Self::new(move |request| {
            if request.url.contains(&url_part) {
                Ok(HttpResponse {
                    status,
                    body: body.clone(),
                })
            } else {
                Err(HttpError::Transport(format!(
                    "unexpected url {}",
                    request.url
                )))
            }
        })
    }
}

#[async_trait]
impl HttpClient for MockHttp {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, HttpError> {
        (self.handler)(&request)
    }
}
