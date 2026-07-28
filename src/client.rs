use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow, bail};
use reqwest::{
    Client, StatusCode,
    header::{ACCEPT, ACCEPT_LANGUAGE, HeaderMap, HeaderValue, REFERER, USER_AGENT},
};
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub delay: Duration,
    pub max_attempts: u32,
    pub timeout: Duration,
    pub referer: String,
}

#[derive(Debug)]
struct RateState {
    next_request_at: Instant,
}

#[derive(Clone)]
pub struct TgbClient {
    client: Client,
    config: ClientConfig,
    rate: Arc<Mutex<RateState>>,
}

#[derive(Debug)]
pub struct FetchResponse {
    pub status: u16,
    pub final_url: String,
    pub text: String,
}

impl TgbClient {
    pub fn new(config: ClientConfig) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static(
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
                 AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0 Safari/537.36 \
                 tgb-cli/0.1",
            ),
        );
        headers.insert(
            ACCEPT,
            HeaderValue::from_static(
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            ),
        );
        headers.insert(
            ACCEPT_LANGUAGE,
            HeaderValue::from_static("zh-CN,zh;q=0.9,en;q=0.7"),
        );
        headers.insert(
            REFERER,
            HeaderValue::from_str(&config.referer).context("invalid referer header")?,
        );

        let client = Client::builder()
            .default_headers(headers)
            .timeout(config.timeout)
            .cookie_store(true)
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
            .context("failed to build HTTP client")?;

        Ok(Self {
            client,
            config,
            rate: Arc::new(Mutex::new(RateState {
                next_request_at: Instant::now(),
            })),
        })
    }

    async fn wait_for_rate_slot(&self) {
        let mut state = self.rate.lock().await;
        let now = Instant::now();
        if state.next_request_at > now {
            tokio::time::sleep(state.next_request_at - now).await;
        }
        state.next_request_at = Instant::now() + self.config.delay;
    }

    pub async fn get_text(&self, url: &str) -> Result<FetchResponse> {
        if self.config.max_attempts == 0 {
            bail!("max_attempts must be at least 1");
        }

        let mut last_error = None;
        for attempt in 1..=self.config.max_attempts {
            self.wait_for_rate_slot().await;
            let result = self.client.get(url).send().await;

            match result {
                Ok(response) => {
                    let status = response.status();
                    let final_url = response.url().to_string();
                    if status.is_success() {
                        let text = response
                            .text()
                            .await
                            .with_context(|| format!("failed reading response body from {url}"))?;
                        return Ok(FetchResponse {
                            status: status.as_u16(),
                            final_url,
                            text,
                        });
                    }

                    let message = format!("HTTP {} from {}", status.as_u16(), final_url);
                    if !is_transient_status(status) || attempt == self.config.max_attempts {
                        bail!(message);
                    }
                    last_error = Some(message);
                }
                Err(error) => {
                    let retryable = error.is_connect() || error.is_timeout() || error.is_request();
                    let message = format!("request failed for {url}: {error}");
                    if !retryable || attempt == self.config.max_attempts {
                        return Err(anyhow!(message));
                    }
                    last_error = Some(message);
                }
            }

            let backoff_ms = 500_u64.saturating_mul(2_u64.pow(attempt.saturating_sub(1)));
            tracing::warn!(
                attempt,
                max_attempts = self.config.max_attempts,
                backoff_ms,
                error = last_error.as_deref().unwrap_or("unknown"),
                "transient request failure"
            );
            tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
        }

        Err(anyhow!(
            "{}",
            last_error.unwrap_or_else(|| "request failed without detail".into())
        ))
    }
}

fn is_transient_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS
        || status == StatusCode::REQUEST_TIMEOUT
        || status == StatusCode::BAD_GATEWAY
        || status == StatusCode::SERVICE_UNAVAILABLE
        || status == StatusCode::GATEWAY_TIMEOUT
        || status.is_server_error()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_statuses_are_classified_conservatively() {
        assert!(is_transient_status(StatusCode::TOO_MANY_REQUESTS));
        assert!(is_transient_status(StatusCode::INTERNAL_SERVER_ERROR));
        assert!(!is_transient_status(StatusCode::NOT_FOUND));
        assert!(!is_transient_status(StatusCode::FORBIDDEN));
    }
}
