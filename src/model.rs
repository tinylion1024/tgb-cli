use chrono::{DateTime, FixedOffset, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Article {
    pub article_id: String,
    pub title: String,
    pub author_id: Option<String>,
    pub author_name: Option<String>,
    pub url: String,
    pub published_at: Option<String>,
    pub published_raw: Option<String>,
    pub source: String,
    pub source_rank: Option<i64>,
    pub body_text: Option<String>,
    pub content_hash: Option<String>,
    pub parse_status: String,
    pub http_status: Option<u16>,
    pub error: Option<String>,
    pub fetched_at: Option<String>,
    pub raw_html_path: Option<String>,
    pub views: Option<i64>,
    pub replies: Option<i64>,
    pub tags: Vec<String>,
}

impl Article {
    pub fn metadata(
        article_id: String,
        title: String,
        url: String,
        source: impl Into<String>,
    ) -> Self {
        Self {
            article_id,
            title,
            author_id: None,
            author_name: None,
            url,
            published_at: None,
            published_raw: None,
            source: source.into(),
            source_rank: None,
            body_text: None,
            content_hash: None,
            parse_status: "metadata".into(),
            http_status: None,
            error: None,
            fetched_at: None,
            raw_html_path: None,
            views: None,
            replies: None,
            tags: Vec::new(),
        }
    }

    pub fn mark_fetch_error(&mut self, message: impl Into<String>) {
        self.parse_status = "fetch_error".into();
        self.error = Some(message.into());
        self.fetched_at = Some(Utc::now().to_rfc3339());
    }

    pub fn body_succeeded(&self) -> bool {
        self.parse_status == "parsed"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RunStats {
    pub requested_pages: i64,
    pub list_pages_fetched: i64,
    pub discovered_count: i64,
    pub body_attempted_count: i64,
    pub body_fetched_count: i64,
    pub parsed_count: i64,
    pub failed_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlRun {
    pub id: i64,
    pub command: String,
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub options_json: String,
    #[serde(flatten)]
    pub stats: RunStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlError {
    pub stage: String,
    pub target: String,
    pub message: String,
    pub http_status: Option<i64>,
    pub created_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuthorInfo {
    pub name: Option<String>,
    pub blog_url: Option<String>,
    pub fans: Option<serde_json::Value>,
    pub tier: Option<String>,
    pub source: Option<String>,
}

pub const SHANGHAI_OFFSET_SECS: i32 = 8 * 60 * 60;

pub fn shanghai_offset() -> FixedOffset {
    FixedOffset::east_opt(SHANGHAI_OFFSET_SECS).expect("valid Shanghai fixed offset")
}

pub fn now_shanghai() -> DateTime<FixedOffset> {
    Utc::now().with_timezone(&shanghai_offset())
}
