use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Datelike, FixedOffset, NaiveDate, NaiveDateTime, TimeZone};
use regex::Regex;
use scraper::{ElementRef, Html, Selector};
use sha2::{Digest, Sha256};
use url::Url;

use crate::model::{Article, now_shanghai, shanghai_offset};

pub fn parse_datetime(value: &str) -> Result<DateTime<FixedOffset>> {
    let value = value.trim();
    if let Ok(parsed) = DateTime::parse_from_rfc3339(value) {
        return Ok(parsed);
    }

    let offset = shanghai_offset();
    for format in ["%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M", "%Y-%m-%dT%H:%M"] {
        if let Ok(parsed) = NaiveDateTime::parse_from_str(value, format) {
            return offset
                .from_local_datetime(&parsed)
                .single()
                .ok_or_else(|| anyhow!("ambiguous datetime: {value}"));
        }
    }

    if let Ok(month_day) = NaiveDateTime::parse_from_str(
        &format!("{}-{value}", now_shanghai().year()),
        "%Y-%m-%d %H:%M",
    ) {
        return offset
            .from_local_datetime(&month_day)
            .single()
            .ok_or_else(|| anyhow!("ambiguous legacy datetime: {value}"));
    }

    bail!("unsupported datetime '{value}'; use RFC3339 or YYYY-MM-DD HH:MM")
}

pub fn resolve_published_at(
    raw: &str,
    from: DateTime<FixedOffset>,
    to: DateTime<FixedOffset>,
) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if let Ok(value) = parse_datetime(raw)
        && raw.len() > 11
    {
        return Some(value.to_rfc3339());
    }

    let start_year = from.year() - 1;
    let end_year = to.year() + 1;
    for year in start_year..=end_year {
        let with_year = format!("{year}-{raw}");
        let Ok(naive) = NaiveDateTime::parse_from_str(&with_year, "%Y-%m-%d %H:%M") else {
            continue;
        };
        let Some(candidate) = shanghai_offset().from_local_datetime(&naive).single() else {
            continue;
        };
        if candidate >= from && candidate <= to {
            return Some(candidate.to_rfc3339());
        }
    }
    None
}

pub fn parse_hot_list(
    html: &str,
    list_url: &str,
    from: DateTime<FixedOffset>,
    to: DateTime<FixedOffset>,
    page: u32,
) -> Result<Vec<Article>> {
    let document = Html::parse_document(html);
    let item_selector = selector(".Nbbs-tiezi-lists")?;
    let date_selector = selector(".left.middle-list-post")?;
    let author_selector = selector(".left.middle-list-user.cblue.cursor.overhide")?;
    let link_selector = selector("a[href]")?;
    let base = Url::parse(list_url).with_context(|| format!("invalid list URL: {list_url}"))?;

    let mut articles = Vec::new();
    for (index, item) in document.select(&item_selector).enumerate() {
        let Some(date_node) = item.select(&date_selector).next() else {
            continue;
        };
        let raw_date = clean_inline(&date_node.text().collect::<Vec<_>>().join(" "));
        let Some(published_at) = resolve_published_at(&raw_date, from, to) else {
            continue;
        };
        let Ok(parsed_date) = DateTime::parse_from_rfc3339(&published_at) else {
            continue;
        };
        if parsed_date < from || parsed_date > to {
            continue;
        }

        let Some(link) = item.select(&link_selector).next() else {
            continue;
        };
        let Some(href) = link.value().attr("href") else {
            continue;
        };
        let Ok(url) = base.join(href) else {
            continue;
        };
        let Some(article_id) = article_id_from_url(url.as_str()) else {
            continue;
        };
        let title = link
            .value()
            .attr("title")
            .map(clean_inline)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| clean_inline(&link.text().collect::<Vec<_>>().join(" ")));
        if title.is_empty() {
            continue;
        }
        let author_name = item
            .select(&author_selector)
            .next()
            .map(|node| clean_inline(&node.text().collect::<Vec<_>>().join(" ")))
            .filter(|value| !value.is_empty());

        let mut article = Article::metadata(article_id, title, url.to_string(), "hot");
        article.author_name = author_name;
        article.published_at = Some(published_at);
        article.published_raw = Some(raw_date);
        article.source_rank = Some(((page - 1) as i64 * 100) + index as i64 + 1);
        articles.push(article);
    }
    Ok(articles)
}

pub fn parse_blog_list(
    html: &str,
    base_url: &str,
    author_id: &str,
    author_name: Option<&str>,
    page: u32,
) -> Result<Vec<Article>> {
    let document = Html::parse_document(html);
    let item_selector = selector(".article_tittle")?;
    let link_selector = selector("a[href]")?;
    let metrics_selector = selector(".tittle_llhf")?;
    let date_selector = selector(".tittle_fbshijian")?;
    let elite_selector = selector(".tittle_jinghua")?;
    let pinned_selector = selector(".tittle_zhiding")?;
    let original_selector = selector(".tittle_yuanchuang")?;
    let base = Url::parse(base_url).with_context(|| format!("invalid base URL: {base_url}"))?;
    let number_re = Regex::new(r"\d+").expect("valid number regex");

    let mut articles = Vec::new();
    for (index, item) in document.select(&item_selector).enumerate() {
        let Some(link) = item.select(&link_selector).next() else {
            continue;
        };
        let Some(href) = link.value().attr("href") else {
            continue;
        };
        let Ok(url) = base.join(href) else {
            continue;
        };
        let Some(article_id) = article_id_from_url(url.as_str()) else {
            continue;
        };
        let title = link
            .value()
            .attr("title")
            .map(clean_inline)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| clean_inline(&link.text().collect::<Vec<_>>().join(" ")));
        if title.is_empty() {
            continue;
        }

        let metrics = item
            .select(&metrics_selector)
            .next()
            .map(|node| node.text().collect::<Vec<_>>().join(" "))
            .unwrap_or_default();
        let numbers = number_re
            .find_iter(&metrics)
            .filter_map(|value| value.as_str().parse::<i64>().ok())
            .collect::<Vec<_>>();
        let published_raw = item
            .select(&date_selector)
            .next()
            .map(|node| clean_inline(&node.text().collect::<Vec<_>>().join(" ")))
            .filter(|value| !value.is_empty());
        let published_at = published_raw.as_deref().and_then(parse_blog_date);

        let mut tags = Vec::new();
        if item.select(&elite_selector).next().is_some() {
            tags.push("精华".into());
        }
        if item.select(&pinned_selector).next().is_some() {
            tags.push("置顶".into());
        }
        if item.select(&original_selector).next().is_some() {
            tags.push("原创".into());
        }

        let mut article = Article::metadata(article_id, title, url.to_string(), "author");
        article.author_id = Some(author_id.to_string());
        article.author_name = author_name.map(ToOwned::to_owned);
        article.published_at = published_at;
        article.published_raw = published_raw;
        article.source_rank = Some(((page - 1) as i64 * 100) + index as i64 + 1);
        article.views = numbers.first().copied();
        article.replies = numbers.get(1).copied();
        article.tags = tags;
        articles.push(article);
    }
    Ok(articles)
}

pub fn extract_article_body(html: &str) -> Result<(String, &'static str)> {
    let cleaned_html = strip_non_content_tags(html);
    let document = Html::parse_document(&cleaned_html);
    let candidates = [
        (".article-text.p_coten", "article-text.p_coten"),
        (".stockDetailContent", "stockDetailContent"),
        (".article-content", "article-content"),
        (".stock_detail", "stock_detail"),
        (
            "article[itemprop='articleBody']",
            "article[itemprop=articleBody]",
        ),
    ];

    for (css, label) in candidates {
        let selector = selector(css)?;
        if let Some(element) = document.select(&selector).next() {
            let text = element_text(element);
            if !text.is_empty() {
                return Ok((text, label));
            }
        }
    }

    bail!("no supported article body container found")
}

pub fn extract_article_metadata(html: &str) -> (Option<String>, Option<String>) {
    let document = Html::parse_document(html);
    let title_selectors = [
        "h1",
        ".article-title",
        ".article_tittle h1",
        "meta[property='og:title']",
        "title",
    ];
    let author_selectors = [
        ".article-author",
        ".author-name",
        ".p_author",
        "meta[name='author']",
    ];

    let title = first_metadata_value(&document, &title_selectors);
    let author = first_metadata_value(&document, &author_selectors);
    (title, author)
}

pub fn apply_article_body(article: &mut Article, html: &str, http_status: u16) {
    article.http_status = Some(http_status);
    article.fetched_at = Some(chrono::Utc::now().to_rfc3339());
    match extract_article_body(html) {
        Ok((body, _selector)) => {
            let char_count = body.chars().count();
            article.content_hash = Some(content_hash(&body));
            article.body_text = Some(body);
            if char_count < 20 {
                article.parse_status = "too_short".into();
                article.error = Some(format!(
                    "body container found but only {char_count} characters were extracted"
                ));
            } else {
                article.parse_status = "parsed".into();
                article.error = None;
            }
        }
        Err(error) => {
            article.parse_status = "missing_body".into();
            article.error = Some(error.to_string());
        }
    }
}

pub fn article_id_from_url(value: &str) -> Option<String> {
    let url = Url::parse(value).ok()?;
    let segments = url.path_segments()?.collect::<Vec<_>>();
    segments
        .windows(2)
        .find(|window| window[0] == "a" && !window[1].is_empty())
        .map(|window| window[1].to_string())
}

pub fn normalize_article_target(base_url: &str, target: &str) -> Result<(String, String)> {
    if target.starts_with("http://") || target.starts_with("https://") {
        let id = article_id_from_url(target)
            .ok_or_else(|| anyhow!("article URL must contain /a/<article-id>"))?;
        return Ok((id, target.to_string()));
    }
    let id = target.trim().trim_matches('/');
    if id.is_empty() || id.contains('/') {
        bail!("invalid article ID: {target}");
    }
    let base = Url::parse(base_url).context("invalid base URL")?;
    let url = base.join(&format!("/a/{id}"))?;
    Ok((id.to_string(), url.to_string()))
}

pub fn normalize_author_id(value: &str) -> Result<String> {
    let trimmed = value.trim();
    if !trimmed.is_empty() && trimmed.chars().all(|ch| ch.is_ascii_digit()) {
        return Ok(trimmed.to_string());
    }
    let url = Url::parse(trimmed).context("author must be a numeric ID or blog URL")?;
    let segments = url
        .path_segments()
        .ok_or_else(|| anyhow!("invalid blog URL"))?;
    let values = segments.collect::<Vec<_>>();
    values
        .windows(2)
        .find(|window| window[0] == "blog" && window[1].chars().all(|ch| ch.is_ascii_digit()))
        .map(|window| window[1].to_string())
        .ok_or_else(|| anyhow!("blog URL must contain /blog/<numeric-id>"))
}

fn parse_blog_date(raw: &str) -> Option<String> {
    if let Ok(date_time) = parse_datetime(raw) {
        return Some(date_time.to_rfc3339());
    }
    if let Ok(date) = NaiveDate::parse_from_str(raw, "%Y-%m-%d") {
        let date_time = date.and_hms_opt(0, 0, 0)?;
        return shanghai_offset()
            .from_local_datetime(&date_time)
            .single()
            .map(|value| value.to_rfc3339());
    }
    None
}

fn selector(value: &str) -> Result<Selector> {
    Selector::parse(value).map_err(|error| anyhow!("invalid CSS selector {value}: {error:?}"))
}

fn first_metadata_value(document: &Html, selectors: &[&str]) -> Option<String> {
    for css in selectors {
        let Ok(selector) = Selector::parse(css) else {
            continue;
        };
        let Some(element) = document.select(&selector).next() else {
            continue;
        };
        let value = element
            .value()
            .attr("content")
            .map(clean_inline)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| clean_inline(&element.text().collect::<Vec<_>>().join(" ")));
        if !value.is_empty() {
            return Some(value);
        }
    }
    None
}

fn clean_inline(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn element_text(element: ElementRef<'_>) -> String {
    let mut lines = Vec::new();
    for fragment in element.text() {
        let cleaned = clean_inline(fragment);
        if cleaned.is_empty() {
            continue;
        }
        if lines.last() != Some(&cleaned) {
            lines.push(cleaned);
        }
    }
    lines.join("\n")
}

fn strip_non_content_tags(html: &str) -> String {
    let script = Regex::new(r"(?is)<script\b[^>]*>.*?</script\s*>").expect("valid regex");
    let style = Regex::new(r"(?is)<style\b[^>]*>.*?</style\s*>").expect("valid regex");
    let noscript = Regex::new(r"(?is)<noscript\b[^>]*>.*?</noscript\s*>").expect("valid regex");
    let value = script.replace_all(html, "");
    let value = style.replace_all(&value, "");
    noscript.replace_all(&value, "").into_owned()
}

fn content_hash(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hot_list_and_resolves_cross_year_dates() {
        let html = r#"
        <div class="Nbbs-tiezi-lists">
          <div class="left middle-list-post">12-31 23:50</div>
          <a href="/a/abc123" title="跨年文章">跨年文章</a>
          <div class="left middle-list-user cblue cursor overhide">作者甲</div>
        </div>"#;
        let from = parse_datetime("2025-12-31 00:00").unwrap();
        let to = parse_datetime("2026-01-01 12:00").unwrap();
        let articles = parse_hot_list(html, "https://www.tgb.cn/dianzan/1-1", from, to, 1).unwrap();
        assert_eq!(articles.len(), 1);
        assert_eq!(articles[0].article_id, "abc123");
        assert!(
            articles[0]
                .published_at
                .as_deref()
                .unwrap()
                .starts_with("2025-12-31")
        );
    }

    #[test]
    fn article_body_keeps_paragraphs_and_drops_script() {
        let html = r#"
        <div class="article-text p_coten">
          <p>第一段正文，有完整语义。</p>
          <p>第二段正文，继续说明观点。</p>
          <script>window.bad = "不应出现";</script>
        </div>"#;
        let (body, selector) = extract_article_body(html).unwrap();
        assert_eq!(selector, "article-text.p_coten");
        assert!(body.contains("第一段正文"));
        assert!(body.contains('\n'));
        assert!(!body.contains("window.bad"));
    }

    #[test]
    fn body_parser_does_not_accept_arbitrary_large_div() {
        let html = format!("<div>{}</div>", "页面导航和推荐".repeat(100));
        assert!(extract_article_body(&html).is_err());
    }

    #[test]
    fn parses_blog_metadata_and_tags() {
        let html = r#"
        <div class="article_tittle">
          <a href="a/post1" title="精选文章">精选文章</a>
          <div class="tittle_llhf">12345阅读/678回复</div>
          <div class="tittle_fbshijian">2026-07-26</div>
          <span class="tittle_jinghua">精华</span>
        </div>"#;
        let articles =
            parse_blog_list(html, "https://www.tgb.cn", "4223", Some("职业炒手"), 1).unwrap();
        assert_eq!(articles.len(), 1);
        assert_eq!(articles[0].views, Some(12345));
        assert_eq!(articles[0].replies, Some(678));
        assert!(articles[0].tags.contains(&"精华".to_string()));
    }
}
