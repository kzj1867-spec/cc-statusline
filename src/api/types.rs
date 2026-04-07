//! GLM API types and error definitions

use serde::Deserialize;
use std::fmt;

/// Platform detection from base URL
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Zai,
    Zhipu,
}

impl Platform {
    /// Detect platform from `ANTHROPIC_BASE_URL`
    #[must_use]
    pub fn detect_from_url(base_url: &str) -> Option<Self> {
        if base_url.contains("api.z.ai") {
            Some(Self::Zai)
        } else if base_url.contains("bigmodel.cn") || base_url.contains("zhipu") {
            Some(Self::Zhipu)
        } else {
            None
        }
    }
}

/// API error types
#[derive(Debug)]
pub enum ApiError {
    /// Missing environment variable
    MissingEnvVar(String),
    /// HTTP request failed
    HttpError(String),
    /// API returned non-success response
    ApiResponse(String),
    /// Failed to parse response body
    ParseError(String),
    /// Could not detect platform from URL
    PlatformDetectionFailed,
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingEnvVar(var) => write!(f, "Missing environment variable: {var}"),
            Self::HttpError(msg) => write!(f, "HTTP request failed: {msg}"),
            Self::ApiResponse(msg) => write!(f, "API returned error: {msg}"),
            Self::ParseError(msg) => write!(f, "Failed to parse response: {msg}"),
            Self::PlatformDetectionFailed => write!(f, "Platform detection failed"),
        }
    }
}

impl std::error::Error for ApiError {}

/// Quota limit API response
#[derive(Debug, Deserialize)]
pub struct QuotaLimitResponse {
    #[allow(dead_code)]
    pub code: i32,
    pub msg: String,
    pub data: QuotaLimitData,
    pub success: bool,
}

/// Quota limit data containing individual limit items
#[derive(Debug, Deserialize)]
pub struct QuotaLimitData {
    pub limits: Vec<QuotaLimitItem>,
}

/// Individual quota limit item
#[derive(Debug, Deserialize, Clone)]
pub struct QuotaLimitItem {
    #[serde(rename = "type")]
    pub quota_type: String,
    #[serde(default)]
    pub usage: i64,
    #[serde(rename = "currentValue", default)]
    pub current_value: i64,
    pub percentage: i32,
    #[serde(rename = "nextResetTime", default)]
    pub next_reset_time: Option<i64>,
    /// 时间窗口单位标识：3=5h, 6=7d
    #[serde(default)]
    pub unit: i32,
    /// 窗口数量（与 unit 配合使用）
    #[serde(default)]
    pub number: i32,
}

/// Combined usage statistics from GLM API
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UsageStats {
    /// 5-hour token usage
    pub token_usage: Option<QuotaUsage>,
    /// MCP/tool usage (30d window)
    pub mcp_usage: Option<QuotaUsage>,
    /// Weekly (7d) token usage
    #[serde(default)]
    pub weekly_token_usage: Option<QuotaUsage>,
}

/// Individual quota usage
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct QuotaUsage {
    /// Current usage amount
    pub used: i64,
    /// Usage limit
    pub limit: i64,
    /// Usage percentage (0-100)
    pub percentage: u8,
    /// Time window label (e.g. "5h", "30d")
    pub time_window: String,
    /// Reset timestamp in seconds (converted from API's milliseconds)
    pub reset_at: Option<i64>,
}

impl fmt::Display for QuotaUsage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}%", self.percentage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_zai_platform() {
        assert_eq!(
            Platform::detect_from_url("https://api.z.ai/v1"),
            Some(Platform::Zai)
        );
    }

    #[test]
    fn test_detect_zhipu_platform_bigmodel() {
        assert_eq!(
            Platform::detect_from_url("https://open.bigmodel.cn/api/anthropic"),
            Some(Platform::Zhipu)
        );
    }

    #[test]
    fn test_detect_zhipu_platform_zhipu() {
        assert_eq!(
            Platform::detect_from_url("https://zhipu.ai/api"),
            Some(Platform::Zhipu)
        );
    }

    #[test]
    fn test_detect_unknown_platform() {
        assert_eq!(
            Platform::detect_from_url("https://api.anthropic.com/v1"),
            None
        );
    }

    #[test]
    fn test_quota_usage_display() {
        let usage = QuotaUsage {
            used: 500,
            limit: 1000,
            percentage: 50,
            time_window: "5h".to_string(),
            reset_at: None,
        };
        assert_eq!(format!("{usage}"), "50%");
    }

    #[test]
    fn test_usage_stats_serialization() {
        let stats = UsageStats {
            token_usage: Some(QuotaUsage {
                used: 100,
                limit: 1000,
                percentage: 10,
                time_window: "5h".to_string(),
                reset_at: Some(1_700_000_000),
            }),
            mcp_usage: None,
            weekly_token_usage: None,
        };
        let json = serde_json::to_string(&stats).ok();
        assert!(json.is_some());
        let json = json.as_deref().unwrap_or_default();
        assert!(json.contains("token_usage"));
    }

    // ===== 新增：ApiError Display 测试 =====

    #[test]
    fn test_api_error_display_missing_env() {
        let err = ApiError::MissingEnvVar("TEST_VAR".to_string());
        assert_eq!(format!("{err}"), "Missing environment variable: TEST_VAR");
    }

    #[test]
    fn test_api_error_display_http() {
        let err = ApiError::HttpError("connection refused".to_string());
        assert_eq!(format!("{err}"), "HTTP request failed: connection refused");
    }

    #[test]
    fn test_api_error_display_api_response() {
        let err = ApiError::ApiResponse("rate limited".to_string());
        assert_eq!(format!("{err}"), "API returned error: rate limited");
    }

    #[test]
    fn test_api_error_display_parse() {
        let err = ApiError::ParseError("invalid JSON".to_string());
        assert_eq!(format!("{err}"), "Failed to parse response: invalid JSON");
    }

    #[test]
    fn test_api_error_display_platform_detection() {
        let err = ApiError::PlatformDetectionFailed;
        assert_eq!(format!("{err}"), "Platform detection failed");
    }

    // ===== 新增：JSON 反序列化测试 =====

    #[test]
    fn test_quota_limit_response_deserialize() {
        let json = r#"{
            "code": 200,
            "msg": "success",
            "data": {
                "limits": [
                    {
                        "type": "TOKENS_LIMIT",
                        "usage": 100000,
                        "currentValue": 32000,
                        "percentage": 32,
                        "nextResetTime": 1700000000000
                    },
                    {
                        "type": "TIME_LIMIT",
                        "usage": 100,
                        "currentValue": 15,
                        "percentage": 15
                    }
                ]
            },
            "success": true
        }"#;

        let resp: QuotaLimitResponse =
            serde_json::from_str(json)
                .ok()
                .unwrap_or(QuotaLimitResponse {
                    code: -1,
                    msg: String::new(),
                    data: QuotaLimitData { limits: vec![] },
                    success: false,
                });
        assert!(resp.success);
        assert_eq!(resp.code, 200);
        assert_eq!(resp.data.limits.len(), 2);

        let token = &resp.data.limits[0];
        assert_eq!(token.quota_type, "TOKENS_LIMIT");
        assert_eq!(token.usage, 100_000);
        assert_eq!(token.current_value, 32_000);
        assert_eq!(token.percentage, 32);
        assert_eq!(token.next_reset_time, Some(1_700_000_000_000_i64));

        let mcp = &resp.data.limits[1];
        assert_eq!(mcp.quota_type, "TIME_LIMIT");
        assert_eq!(mcp.current_value, 15);
        assert_eq!(mcp.percentage, 15);
        assert!(mcp.next_reset_time.is_none());
    }

    #[test]
    fn test_quota_limit_response_failure() {
        let json = r#"{
            "code": 429,
            "msg": "rate limited",
            "data": { "limits": [] },
            "success": false
        }"#;
        let resp: QuotaLimitResponse =
            serde_json::from_str(json)
                .ok()
                .unwrap_or(QuotaLimitResponse {
                    code: -1,
                    msg: String::new(),
                    data: QuotaLimitData { limits: vec![] },
                    success: true, // 默认值，确保断言失败
                });
        assert!(!resp.success);
        assert_eq!(resp.msg, "rate limited");
    }

    #[test]
    fn test_quota_limit_item_default_fields() {
        let json = r#"{
            "type": "TOKENS_LIMIT",
            "percentage": 50
        }"#;
        let item: QuotaLimitItem = serde_json::from_str(json).ok().unwrap_or(QuotaLimitItem {
            quota_type: String::new(),
            usage: -1, // 默认值，确保断言失败
            current_value: -1,
            percentage: 0,
            next_reset_time: Some(1), // 默认值，确保断言失败
            unit: -1,                 // 默认值，确保断言失败
            number: -1,
        });
        assert_eq!(item.quota_type, "TOKENS_LIMIT");
        assert_eq!(item.usage, 0); // default
        assert_eq!(item.current_value, 0); // default
        assert_eq!(item.percentage, 50);
        assert!(item.next_reset_time.is_none()); // default
        assert_eq!(item.unit, 0); // default
        assert_eq!(item.number, 0); // default
    }

    #[test]
    fn test_usage_stats_roundtrip() {
        let stats = UsageStats {
            token_usage: Some(QuotaUsage {
                used: 320,
                limit: 1000,
                percentage: 32,
                time_window: "5h".to_string(),
                reset_at: Some(1_700_000_000),
            }),
            mcp_usage: Some(QuotaUsage {
                used: 15,
                limit: 100,
                percentage: 15,
                time_window: "30d".to_string(),
                reset_at: None,
            }),
            weekly_token_usage: None,
        };
        let json = serde_json::to_string(&stats).ok();
        assert!(json.is_some());
        let deserialized: UsageStats = serde_json::from_str(json.as_deref().unwrap_or_default())
            .ok()
            .unwrap_or(UsageStats {
                token_usage: None,
                mcp_usage: None,
                weekly_token_usage: None,
            });
        assert_eq!(
            deserialized
                .token_usage
                .as_ref()
                .map_or(0, |u| u.percentage),
            32
        );
        assert_eq!(deserialized.mcp_usage.as_ref().map_or(0, |u| u.used), 15);
    }

    #[test]
    fn test_usage_stats_weekly_compat() {
        // 旧缓存不包含 weekly_token_usage 字段，反序列化后应为 None
        let json = r#"{"token_usage":{"used":320,"limit":1000,"percentage":32,"time_window":"5h","reset_at":1700000000},"mcp_usage":null}"#;
        let stats: UsageStats = serde_json::from_str(json).ok().unwrap_or(UsageStats {
            token_usage: None,
            mcp_usage: None,
            weekly_token_usage: Some(QuotaUsage {
                used: -1,
                limit: -1,
                percentage: 255,
                time_window: String::new(),
                reset_at: Some(1),
            }),
        });
        assert!(stats.token_usage.is_some());
        assert!(stats.weekly_token_usage.is_none());
    }

    #[test]
    fn test_usage_stats_with_weekly() {
        let stats = UsageStats {
            token_usage: Some(QuotaUsage {
                used: 320,
                limit: 1000,
                percentage: 32,
                time_window: "5h".to_string(),
                reset_at: None,
            }),
            mcp_usage: None,
            weekly_token_usage: Some(QuotaUsage {
                used: 500,
                limit: 10000,
                percentage: 5,
                time_window: "7d".to_string(),
                reset_at: Some(1_700_100_000),
            }),
        };
        let json = serde_json::to_string(&stats).ok();
        assert!(json.is_some());
        let deserialized: UsageStats = serde_json::from_str(json.as_deref().unwrap_or_default())
            .ok()
            .unwrap_or(UsageStats {
                token_usage: None,
                mcp_usage: None,
                weekly_token_usage: None,
            });
        assert!(deserialized.weekly_token_usage.is_some());
        assert_eq!(
            deserialized
                .weekly_token_usage
                .as_ref()
                .map_or(0, |u| u.percentage),
            5
        );
    }

    #[test]
    fn test_quota_limit_item_unit_number_deserialize() {
        let json = r#"{
            "type": "TOKENS_LIMIT",
            "usage": 100000,
            "currentValue": 50000,
            "percentage": 50,
            "unit": 6,
            "number": 7
        }"#;
        let item: QuotaLimitItem = serde_json::from_str(json).ok().unwrap_or(QuotaLimitItem {
            quota_type: String::new(),
            usage: -1,
            current_value: -1,
            percentage: 0,
            next_reset_time: Some(1),
            unit: -1,
            number: -1,
        });
        assert_eq!(item.unit, 6);
        assert_eq!(item.number, 7);
    }

    // ===== 新增：平台检测边界测试 =====

    #[test]
    fn test_detect_empty_url() {
        assert_eq!(Platform::detect_from_url(""), None);
    }

    #[test]
    fn test_detect_zai_with_path() {
        assert_eq!(
            Platform::detect_from_url("https://api.z.ai/anthropic/v1"),
            Some(Platform::Zai)
        );
    }

    #[test]
    fn test_detect_zhipu_subdomain() {
        assert_eq!(
            Platform::detect_from_url("https://open.zhipu.cn/api"),
            Some(Platform::Zhipu)
        );
    }
}
