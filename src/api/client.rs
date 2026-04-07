//! GLM API client for fetching quota usage data

use super::types::{ApiError, Platform, QuotaLimitResponse, QuotaUsage, UsageStats};
use anyhow::Result;
use std::time::Duration;
use ureq::{Agent, Request};

/// GLM API client
pub struct GlmApiClient {
    agent: Agent,
    base_url: String,
    token: String,
    #[allow(dead_code)] // 保留供未来调试/日志使用
    timeout_ms: u64,
    retry_attempts: u32,
}

impl GlmApiClient {
    /// Create client from environment variables
    ///
    /// Reads `ANTHROPIC_AUTH_TOKEN` and `ANTHROPIC_BASE_URL` from environment.
    /// Defaults base URL to `https://open.bigmodel.cn/api/anthropic`.
    ///
    /// # Errors
    /// Returns `ApiError::MissingEnvVar` if `ANTHROPIC_AUTH_TOKEN` is not set,
    /// or `ApiError::PlatformDetectionFailed` if the base URL doesn't match known platforms.
    pub fn from_env() -> Result<Self, ApiError> {
        let token = std::env::var("ANTHROPIC_AUTH_TOKEN")
            .map_err(|_| ApiError::MissingEnvVar("ANTHROPIC_AUTH_TOKEN".to_string()))?;

        let base_url = std::env::var("ANTHROPIC_BASE_URL")
            .unwrap_or_else(|_| "https://open.bigmodel.cn/api/anthropic".to_string());

        Self::with_config(&base_url, &token, 5000, 2)
    }

    /// Create client with explicit configuration
    ///
    /// # Errors
    /// Returns `ApiError::PlatformDetectionFailed` if the base URL doesn't match
    /// any known GLM platform (ZHIPU or ZAI).
    pub fn with_config(
        base_url: &str,
        token: &str,
        timeout_ms: u64,
        retry_attempts: u32,
    ) -> Result<Self, ApiError> {
        let platform =
            Platform::detect_from_url(base_url).ok_or(ApiError::PlatformDetectionFailed)?;

        // Fix base URL for ZHIPU platform (remove /anthropic suffix for monitor API)
        let base_url = if platform == Platform::Zhipu {
            base_url
                .replace("/api/anthropic", "/api")
                .replace("/anthropic", "")
        } else {
            base_url.to_string()
        };

        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_millis(timeout_ms))
            .build();

        Ok(Self {
            agent,
            base_url,
            token: token.to_string(),
            timeout_ms,
            retry_attempts,
        })
    }

    /// Fetch complete usage statistics with retry
    ///
    /// # Errors
    /// Returns an error if all retry attempts fail, the API returns a non-success
    /// response, or the response cannot be parsed.
    pub fn fetch_usage_stats(&self) -> Result<UsageStats> {
        let mut last_error: Option<anyhow::Error> = None;

        for attempt in 0..=self.retry_attempts {
            match self.try_fetch_usage_stats() {
                Ok(stats) => return Ok(stats),
                Err(e) => {
                    last_error = Some(e);
                    if attempt < self.retry_attempts {
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Unknown API error")))
    }

    fn try_fetch_usage_stats(&self) -> Result<UsageStats> {
        let url = format!("{}/monitor/usage/quota/limit", self.base_url);

        let response = self
            .authenticated_request(&url)
            .call()
            .map_err(|e| ApiError::HttpError(e.to_string()))?;

        if response.status() != 200 {
            return Err(ApiError::ApiResponse(format!(
                "Status {}: {}",
                response.status(),
                response.status_text()
            ))
            .into());
        }

        let quota_response: QuotaLimitResponse = response
            .into_json()
            .map_err(|e| ApiError::ParseError(e.to_string()))?;

        if !quota_response.success {
            return Err(ApiError::ApiResponse(quota_response.msg).into());
        }

        // Extract token usage (TOKENS_LIMIT)，按 unit 区分 5h(unit=3) 和 7d(unit=6)
        let token_usage = quota_response
            .data
            .limits
            .iter()
            .filter(|item| item.quota_type == "TOKENS_LIMIT")
            .find(|item| item.unit == 3 || item.unit == 0) // unit=3 或缺失(unit=0)均视为 5h
            .or_else(|| {
                // 未找到 unit=3 的条目时，fallback 取任意一个 TOKENS_LIMIT
                quota_response
                    .data
                    .limits
                    .iter()
                    .find(|item| item.quota_type == "TOKENS_LIMIT")
            })
            .map(|item| QuotaUsage {
                used: item.current_value,
                limit: item.usage,
                #[allow(clippy::cast_sign_loss)] // clamp(0, 100) 确保非负
                percentage: item.percentage.clamp(0, 100) as u8,
                time_window: "5h".to_string(),
                reset_at: item.next_reset_time.map(|ms| ms / 1000),
            });

        // Extract weekly token usage (TOKENS_LIMIT with unit=6 → 7d)
        let weekly_token_usage = quota_response
            .data
            .limits
            .iter()
            .find(|item| item.quota_type == "TOKENS_LIMIT" && item.unit == 6)
            .map(|item| QuotaUsage {
                used: item.current_value,
                limit: item.usage,
                #[allow(clippy::cast_sign_loss)] // clamp(0, 100) 确保非负
                percentage: item.percentage.clamp(0, 100) as u8,
                time_window: "7d".to_string(),
                reset_at: item.next_reset_time.map(|ms| ms / 1000),
            });

        // Extract tool usage (TIME_LIMIT)
        let mcp_usage = quota_response
            .data
            .limits
            .iter()
            .find(|item| item.quota_type == "TIME_LIMIT")
            .map(|item| QuotaUsage {
                used: item.current_value,
                limit: item.usage,
                #[allow(clippy::cast_sign_loss)] // clamp(0, 100) 确保非负
                percentage: item.percentage.clamp(0, 100) as u8,
                time_window: "30d".to_string(),
                reset_at: item.next_reset_time.map(|ms| ms / 1000),
            });

        Ok(UsageStats {
            token_usage,
            mcp_usage,
            weekly_token_usage,
        })
    }

    fn authenticated_request(&self, url: &str) -> Request {
        self.agent
            .get(url)
            .set("Authorization", &format!("Bearer {}", self.token))
            .set("Content-Type", "application/json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_client_base_url_contains(client: &GlmApiClient, expected: &str) {
        assert!(
            client.base_url.contains(expected),
            "base_url '{}' should contain '{}'",
            client.base_url,
            expected
        );
    }

    fn assert_client_base_url_not_contains(client: &GlmApiClient, unexpected: &str) {
        assert!(
            !client.base_url.contains(unexpected),
            "base_url '{}' should not contain '{}'",
            client.base_url,
            unexpected
        );
    }

    #[test]
    fn test_with_config_zhipu_url_transform() {
        let client = GlmApiClient::with_config(
            "https://open.bigmodel.cn/api/anthropic",
            "test-token",
            5000,
            2,
        );
        assert!(client.is_ok());
        if let Ok(cl) = client {
            assert_client_base_url_contains(&cl, "/api");
            assert_client_base_url_not_contains(&cl, "/anthropic");
        }
    }

    #[test]
    fn test_with_config_zai_no_transform() {
        let client = GlmApiClient::with_config("https://api.z.ai/v1", "test-token", 5000, 2);
        assert!(client.is_ok());
    }

    #[test]
    fn test_with_config_unknown_platform_fails() {
        let result = GlmApiClient::with_config("https://api.anthropic.com/v1", "token", 5000, 2);
        assert!(result.is_err());
    }

    #[test]
    fn test_from_env_missing_token() {
        // Ensure ANTHROPIC_AUTH_TOKEN is not set
        std::env::remove_var("ANTHROPIC_AUTH_TOKEN");
        let result = GlmApiClient::from_env();
        assert!(result.is_err());
    }

    #[test]
    fn test_with_config_zhipu_simple_url() {
        let client = GlmApiClient::with_config("https://zhipu.ai/api", "token", 3000, 1);
        assert!(client.is_ok());
    }

    #[test]
    fn test_with_config_zhipu_double_anthropic() {
        let client = GlmApiClient::with_config(
            "https://open.bigmodel.cn/api/anthropic/anthropic",
            "token",
            5000,
            2,
        );
        assert!(client.is_ok());
        if let Ok(cl) = client {
            assert_client_base_url_not_contains(&cl, "anthropic");
        }
    }
}
