use std::{fs, path::Path};

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};

use crate::model::{Article, CrawlError, CrawlRun, RunStats};

const SCHEMA: &str = r#"
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS tgb_articles (
    article_id      TEXT PRIMARY KEY,
    title           TEXT NOT NULL,
    author_id       TEXT,
    author_name     TEXT,
    url             TEXT NOT NULL,
    published_at    TEXT,
    published_raw   TEXT,
    body_text       TEXT,
    content_hash    TEXT,
    parse_status    TEXT NOT NULL DEFAULT 'metadata',
    http_status     INTEGER,
    error           TEXT,
    fetched_at      TEXT,
    raw_html_path   TEXT,
    views           INTEGER,
    replies         INTEGER,
    tags_json       TEXT NOT NULL DEFAULT '[]',
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS tgb_crawl_runs (
    id                      INTEGER PRIMARY KEY AUTOINCREMENT,
    command                 TEXT NOT NULL,
    status                  TEXT NOT NULL DEFAULT 'running',
    started_at              TEXT NOT NULL DEFAULT (datetime('now')),
    finished_at             TEXT,
    options_json            TEXT NOT NULL,
    requested_pages         INTEGER NOT NULL DEFAULT 0,
    list_pages_fetched      INTEGER NOT NULL DEFAULT 0,
    discovered_count        INTEGER NOT NULL DEFAULT 0,
    body_attempted_count    INTEGER NOT NULL DEFAULT 0,
    body_fetched_count      INTEGER NOT NULL DEFAULT 0,
    parsed_count            INTEGER NOT NULL DEFAULT 0,
    failed_count            INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS tgb_article_sources (
    article_id      TEXT NOT NULL,
    run_id          INTEGER NOT NULL,
    source          TEXT NOT NULL,
    source_rank     INTEGER,
    discovered_at  TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (article_id, run_id, source),
    FOREIGN KEY (article_id) REFERENCES tgb_articles(article_id),
    FOREIGN KEY (run_id) REFERENCES tgb_crawl_runs(id)
);

CREATE TABLE IF NOT EXISTS tgb_crawl_errors (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id          INTEGER NOT NULL,
    stage           TEXT NOT NULL,
    target          TEXT NOT NULL,
    message         TEXT NOT NULL,
    http_status     INTEGER,
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (run_id) REFERENCES tgb_crawl_runs(id)
);

CREATE INDEX IF NOT EXISTS idx_tgb_articles_published_at
    ON tgb_articles(published_at);
CREATE INDEX IF NOT EXISTS idx_tgb_articles_author_id
    ON tgb_articles(author_id);
CREATE INDEX IF NOT EXISTS idx_tgb_articles_parse_status
    ON tgb_articles(parse_status);
CREATE INDEX IF NOT EXISTS idx_tgb_sources_run
    ON tgb_article_sources(run_id);
CREATE INDEX IF NOT EXISTS idx_tgb_errors_run
    ON tgb_crawl_errors(run_id);
"#;

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if path != Path::new(":memory:")
            && let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed creating database directory {}", parent.display())
            })?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("failed opening SQLite database {}", path.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(SCHEMA)
            .context("failed initializing tgb SQLite schema")?;
        Ok(Self { conn })
    }

    pub fn create_run(
        &self,
        command: &str,
        options_json: &str,
        requested_pages: i64,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO tgb_crawl_runs (command, options_json, requested_pages)
             VALUES (?, ?, ?)",
            params![command, options_json, requested_pages],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn finish_run(&self, run_id: i64, status: &str, stats: &RunStats) -> Result<()> {
        self.conn.execute(
            r#"UPDATE tgb_crawl_runs SET
                status = ?,
                finished_at = datetime('now'),
                requested_pages = ?,
                list_pages_fetched = ?,
                discovered_count = ?,
                body_attempted_count = ?,
                body_fetched_count = ?,
                parsed_count = ?,
                failed_count = ?
               WHERE id = ?"#,
            params![
                status,
                stats.requested_pages,
                stats.list_pages_fetched,
                stats.discovered_count,
                stats.body_attempted_count,
                stats.body_fetched_count,
                stats.parsed_count,
                stats.failed_count,
                run_id
            ],
        )?;
        Ok(())
    }

    pub fn upsert_article(&self, article: &Article) -> Result<()> {
        let tags_json = serde_json::to_string(&article.tags)?;
        self.conn.execute(
            r#"INSERT INTO tgb_articles (
                article_id, title, author_id, author_name, url,
                published_at, published_raw, body_text, content_hash,
                parse_status, http_status, error, fetched_at, raw_html_path,
                views, replies, tags_json
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(article_id) DO UPDATE SET
                title = excluded.title,
                author_id = COALESCE(excluded.author_id, tgb_articles.author_id),
                author_name = COALESCE(excluded.author_name, tgb_articles.author_name),
                url = excluded.url,
                published_at = COALESCE(excluded.published_at, tgb_articles.published_at),
                published_raw = COALESCE(excluded.published_raw, tgb_articles.published_raw),
                body_text = COALESCE(excluded.body_text, tgb_articles.body_text),
                content_hash = COALESCE(excluded.content_hash, tgb_articles.content_hash),
                parse_status = CASE
                    WHEN tgb_articles.parse_status = 'parsed'
                         AND excluded.parse_status != 'parsed'
                    THEN tgb_articles.parse_status
                    WHEN excluded.parse_status = 'metadata'
                         AND tgb_articles.parse_status != 'metadata'
                    THEN tgb_articles.parse_status
                    ELSE excluded.parse_status
                END,
                http_status = CASE
                    WHEN tgb_articles.parse_status = 'parsed'
                         AND excluded.parse_status != 'parsed'
                    THEN tgb_articles.http_status
                    ELSE COALESCE(excluded.http_status, tgb_articles.http_status)
                END,
                error = CASE
                    WHEN tgb_articles.parse_status = 'parsed'
                         AND excluded.parse_status != 'parsed'
                    THEN tgb_articles.error
                    WHEN excluded.parse_status = 'metadata'
                    THEN tgb_articles.error
                    ELSE excluded.error
                END,
                fetched_at = CASE
                    WHEN tgb_articles.parse_status = 'parsed'
                         AND excluded.parse_status != 'parsed'
                    THEN tgb_articles.fetched_at
                    ELSE COALESCE(excluded.fetched_at, tgb_articles.fetched_at)
                END,
                raw_html_path = COALESCE(excluded.raw_html_path, tgb_articles.raw_html_path),
                views = COALESCE(excluded.views, tgb_articles.views),
                replies = COALESCE(excluded.replies, tgb_articles.replies),
                tags_json = CASE
                    WHEN excluded.tags_json = '[]' THEN tgb_articles.tags_json
                    ELSE excluded.tags_json
                END,
                updated_at = datetime('now')"#,
            params![
                article.article_id,
                article.title,
                article.author_id,
                article.author_name,
                article.url,
                article.published_at,
                article.published_raw,
                article.body_text,
                article.content_hash,
                article.parse_status,
                article.http_status,
                article.error,
                article.fetched_at,
                article.raw_html_path,
                article.views,
                article.replies,
                tags_json,
            ],
        )?;
        Ok(())
    }

    pub fn link_article_to_run(&self, run_id: i64, article: &Article) -> Result<()> {
        self.conn.execute(
            r#"INSERT INTO tgb_article_sources (article_id, run_id, source, source_rank)
               VALUES (?, ?, ?, ?)
               ON CONFLICT(article_id, run_id, source) DO UPDATE SET
                   source_rank = excluded.source_rank"#,
            params![
                article.article_id,
                run_id,
                article.source,
                article.source_rank
            ],
        )?;
        Ok(())
    }

    pub fn record_article(&self, run_id: i64, article: &Article) -> Result<()> {
        self.upsert_article(article)?;
        self.link_article_to_run(run_id, article)
    }

    pub fn record_error(
        &self,
        run_id: i64,
        stage: &str,
        target: &str,
        message: &str,
        http_status: Option<i64>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO tgb_crawl_errors (run_id, stage, target, message, http_status)
             VALUES (?, ?, ?, ?, ?)",
            params![run_id, stage, target, message, http_status],
        )?;
        Ok(())
    }

    pub fn has_parsed_body(&self, article_id: &str) -> Result<bool> {
        let value = self
            .conn
            .query_row(
                "SELECT 1 FROM tgb_articles
                 WHERE article_id = ? AND parse_status = 'parsed'
                       AND body_text IS NOT NULL AND length(body_text) > 0",
                [article_id],
                |_| Ok(true),
            )
            .optional()?;
        Ok(value.unwrap_or(false))
    }

    pub fn get_run(&self, run_id: i64) -> Result<Option<CrawlRun>> {
        self.conn
            .query_row(
                r#"SELECT id, command, status, started_at, finished_at, options_json,
                          requested_pages, list_pages_fetched, discovered_count,
                          body_attempted_count, body_fetched_count, parsed_count, failed_count
                   FROM tgb_crawl_runs WHERE id = ?"#,
                [run_id],
                row_to_run,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn list_runs(&self, limit: usize) -> Result<Vec<CrawlRun>> {
        let mut statement = self.conn.prepare(
            r#"SELECT id, command, status, started_at, finished_at, options_json,
                      requested_pages, list_pages_fetched, discovered_count,
                      body_attempted_count, body_fetched_count, parsed_count, failed_count
               FROM tgb_crawl_runs ORDER BY id DESC LIMIT ?"#,
        )?;
        let rows = statement.query_map([limit as i64], row_to_run)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn get_run_errors(&self, run_id: i64) -> Result<Vec<CrawlError>> {
        let mut statement = self.conn.prepare(
            r#"SELECT stage, target, message, http_status, created_at
               FROM tgb_crawl_errors WHERE run_id = ? ORDER BY id"#,
        )?;
        let rows = statement.query_map([run_id], |row| {
            Ok(CrawlError {
                stage: row.get(0)?,
                target: row.get(1)?,
                message: row.get(2)?,
                http_status: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn articles_for_export(
        &self,
        run_id: Option<i64>,
        only_success: bool,
    ) -> Result<Vec<Article>> {
        let mut sql = String::from(
            r#"SELECT
                a.article_id, a.title, a.author_id, a.author_name, a.url,
                a.published_at, a.published_raw,
                COALESCE(s.source, 'stored') AS source, s.source_rank,
                a.body_text, a.content_hash, a.parse_status, a.http_status,
                a.error, a.fetched_at, a.raw_html_path, a.views, a.replies,
                a.tags_json
               FROM tgb_articles a
               LEFT JOIN (
                   SELECT article_id, MIN(source) AS source, MIN(source_rank) AS source_rank
                   FROM tgb_article_sources"#,
        );
        if run_id.is_some() {
            sql.push_str(" WHERE run_id = ?");
        }
        sql.push_str(
            r#" GROUP BY article_id
               ) s ON s.article_id = a.article_id"#,
        );
        let mut filters = Vec::new();
        if run_id.is_some() {
            filters.push("s.article_id IS NOT NULL");
        }
        if only_success {
            filters.push("a.parse_status = 'parsed'");
        }
        if !filters.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&filters.join(" AND "));
        }
        sql.push_str(" ORDER BY a.published_at DESC, s.source_rank ASC, a.article_id");

        let mut statement = self.conn.prepare(&sql)?;
        let map_row = |row: &rusqlite::Row<'_>| -> rusqlite::Result<Article> {
            let tags_json: String = row.get(18)?;
            Ok(Article {
                article_id: row.get(0)?,
                title: row.get(1)?,
                author_id: row.get(2)?,
                author_name: row.get(3)?,
                url: row.get(4)?,
                published_at: row.get(5)?,
                published_raw: row.get(6)?,
                source: row.get(7)?,
                source_rank: row.get(8)?,
                body_text: row.get(9)?,
                content_hash: row.get(10)?,
                parse_status: row.get(11)?,
                http_status: row.get::<_, Option<i64>>(12)?.map(|value| value as u16),
                error: row.get(13)?,
                fetched_at: row.get(14)?,
                raw_html_path: row.get(15)?,
                views: row.get(16)?,
                replies: row.get(17)?,
                tags: serde_json::from_str(&tags_json).unwrap_or_default(),
            })
        };

        let values = match run_id {
            Some(run_id) => statement
                .query_map([run_id], map_row)?
                .collect::<Result<Vec<_>, _>>()?,
            None => statement
                .query_map([], map_row)?
                .collect::<Result<Vec<_>, _>>()?,
        };
        Ok(values)
    }
}

fn row_to_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<CrawlRun> {
    Ok(CrawlRun {
        id: row.get(0)?,
        command: row.get(1)?,
        status: row.get(2)?,
        started_at: row.get(3)?,
        finished_at: row.get(4)?,
        options_json: row.get(5)?,
        stats: RunStats {
            requested_pages: row.get(6)?,
            list_pages_fetched: row.get(7)?,
            discovered_count: row.get(8)?,
            body_attempted_count: row.get(9)?,
            body_fetched_count: row.get(10)?,
            parsed_count: row.get(11)?,
            failed_count: row.get(12)?,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Article;

    #[test]
    fn stores_articles_runs_and_errors() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let run_id = store.create_run("hot", "{}", 2).unwrap();
        let mut article = Article::metadata(
            "abc".into(),
            "测试".into(),
            "https://www.tgb.cn/a/abc".into(),
            "hot",
        );
        article.parse_status = "parsed".into();
        article.body_text = Some("这是一段足够长的测试正文，用于验证存储。".into());
        article.http_status = Some(200);
        store.record_article(run_id, &article).unwrap();
        store
            .record_error(run_id, "body", article.url.as_str(), "example", None)
            .unwrap();
        let stats = RunStats {
            requested_pages: 2,
            discovered_count: 1,
            parsed_count: 1,
            ..Default::default()
        };
        store.finish_run(run_id, "partial", &stats).unwrap();

        assert!(store.has_parsed_body("abc").unwrap());
        assert_eq!(
            store.get_run(run_id).unwrap().unwrap().stats.parsed_count,
            1
        );
        assert_eq!(store.get_run_errors(run_id).unwrap().len(), 1);
        assert_eq!(
            store.articles_for_export(Some(run_id), true).unwrap().len(),
            1
        );

        let second_run = store.create_run("article", "{}", 1).unwrap();
        let mut same_article = article.clone();
        same_article.source = "article".into();
        same_article.mark_fetch_error("temporary network error");
        store.record_article(second_run, &same_article).unwrap();
        assert!(store.has_parsed_body("abc").unwrap());
        assert_eq!(store.articles_for_export(None, true).unwrap().len(), 1);
        let exported = store.articles_for_export(None, true).unwrap();
        assert_eq!(exported[0].parse_status, "parsed");
        assert_eq!(exported[0].http_status, Some(200));
        assert_eq!(exported[0].error, None);
    }
}
