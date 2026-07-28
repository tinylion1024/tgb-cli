use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail};
use futures::{StreamExt, stream};
use serde_json::json;
use url::Url;

use crate::{
    cli::{
        ArticleArgs, AuthorArgs, Cli, Command, DEFAULT_AUTHORS_PATH, ExportArgs, HotArgs,
        RunCommand, VipArgs,
    },
    client::{ClientConfig, TgbClient},
    export,
    model::{Article, AuthorInfo, RunStats},
    parser::{
        apply_article_body, extract_article_metadata, normalize_article_target,
        normalize_author_id, parse_blog_list, parse_datetime, parse_hot_list,
    },
    store::Store,
};

const BUILTIN_AUTHORS_JSON: &str = include_str!("../references/tgb_blog_authors.json");

pub async fn run(cli: Cli) -> Result<()> {
    match &cli.command {
        Command::Hot(args) => run_hot(&cli, args).await,
        Command::Article(args) => run_article(&cli, args).await,
        Command::Author(args) => run_author(&cli, args).await,
        Command::Vip(args) => run_vip(&cli, args).await,
        Command::Run(args) => {
            let store = Store::open(&cli.database)?;
            match &args.command {
                RunCommand::List { limit } => show_run_list(&store, *limit),
                RunCommand::Show { run_id } => show_run(&store, *run_id),
            }
        }
        Command::Export(args) => run_export(&cli, args),
    }
}

fn build_client(cli: &Cli) -> Result<TgbClient> {
    TgbClient::new(ClientConfig {
        delay: Duration::from_millis(cli.delay_ms),
        max_attempts: cli.max_attempts,
        timeout: Duration::from_secs(cli.timeout_secs),
        referer: cli.base_url.clone(),
    })
}

async fn run_hot(cli: &Cli, args: &HotArgs) -> Result<()> {
    validate_common(args.pages, args.concurrency)?;
    let from = parse_datetime(&args.from)?;
    let to = parse_datetime(&args.to)?;
    if from > to {
        bail!("--from must not be later than --to");
    }

    let store = Store::open(&cli.database)?;
    let options = json!({
        "from": from.to_rfc3339(),
        "to": to.to_rfc3339(),
        "pages": args.pages,
        "fetch_body": args.fetch_body,
        "concurrency": args.concurrency,
        "raw_dir": args.raw_dir.as_ref(),
    });
    let run_id = store.create_run("hot", &options.to_string(), args.pages as i64)?;
    let client = build_client(cli)?;
    let mut stats = RunStats {
        requested_pages: args.pages as i64,
        ..Default::default()
    };
    let mut articles = Vec::new();
    let mut seen = HashSet::new();

    for page in 1..=args.pages {
        let list_url = join_base(&cli.base_url, &format!("/dianzan/{page}-1"))?;
        tracing::info!(page, pages = args.pages, url = %list_url, "fetching hot list");
        match client.get_text(&list_url).await {
            Ok(response) => {
                stats.list_pages_fetched += 1;
                if let Some(raw_dir) = &args.raw_dir
                    && let Err(error) =
                        save_raw(raw_dir, &format!("hot-list-{page}"), &response.text)
                {
                    tracing::warn!(%error, "failed saving raw hot list");
                }
                match parse_hot_list(&response.text, &list_url, from, to, page) {
                    Ok(found) => {
                        for article in found {
                            if seen.insert(article.article_id.clone()) {
                                store.record_article(run_id, &article)?;
                                articles.push(article);
                            }
                        }
                    }
                    Err(error) => {
                        stats.failed_count += 1;
                        store.record_error(
                            run_id,
                            "parse_list",
                            &list_url,
                            &error.to_string(),
                            Some(response.status as i64),
                        )?;
                    }
                }
            }
            Err(error) => {
                stats.failed_count += 1;
                store.record_error(run_id, "fetch_list", &list_url, &error.to_string(), None)?;
            }
        }
    }

    stats.discovered_count = articles.len() as i64;
    if args.fetch_body {
        stats.body_attempted_count = articles.len() as i64;
        articles =
            fetch_body_batch(client, articles, args.concurrency, args.raw_dir.as_deref()).await;
        persist_body_results(&store, run_id, &articles, &mut stats)?;
    }

    finish_run(&store, run_id, &stats)?;
    print_run_summary(run_id, "hot", &stats);
    Ok(())
}

async fn run_article(cli: &Cli, args: &ArticleArgs) -> Result<()> {
    let (article_id, url) = normalize_article_target(&cli.base_url, &args.target)?;
    let client = build_client(cli)?;
    let mut article = Article::metadata(article_id, args.target.clone(), url.clone(), "article");

    let response = client.get_text(&url).await;
    match response {
        Ok(response) => {
            let (title, author) = extract_article_metadata(&response.text);
            if let Some(title) = title {
                article.title = title;
            }
            article.author_name = author;
            article.url = response.final_url;
            if let Some(raw_dir) = &args.raw_dir {
                article.raw_html_path = Some(
                    save_raw(raw_dir, &article.article_id, &response.text)?
                        .display()
                        .to_string(),
                );
            }
            apply_article_body(&mut article, &response.text, response.status);
        }
        Err(error) => article.mark_fetch_error(error.to_string()),
    }

    if args.no_save {
        println!("{}", serde_json::to_string_pretty(&article)?);
        return if article.body_succeeded() {
            Ok(())
        } else {
            Err(anyhow!(
                "{}",
                article.error.as_deref().unwrap_or("article parse failed")
            ))
        };
    }

    let store = Store::open(&cli.database)?;
    let options = json!({"target": args.target, "raw_dir": args.raw_dir.as_ref()});
    let run_id = store.create_run("article", &options.to_string(), 1)?;
    store.record_article(run_id, &article)?;
    let mut stats = RunStats {
        requested_pages: 1,
        discovered_count: 1,
        body_attempted_count: 1,
        ..Default::default()
    };
    if article.http_status.is_some() {
        stats.body_fetched_count = 1;
    }
    if article.body_succeeded() {
        stats.parsed_count = 1;
    } else {
        stats.failed_count = 1;
        store.record_error(
            run_id,
            "article",
            &article.url,
            article.error.as_deref().unwrap_or("article parse failed"),
            article.http_status.map(i64::from),
        )?;
    }
    finish_run(&store, run_id, &stats)?;
    println!("{}", serde_json::to_string_pretty(&article)?);
    print_run_summary(run_id, "article", &stats);
    Ok(())
}

async fn run_author(cli: &Cli, args: &AuthorArgs) -> Result<()> {
    validate_common(args.pages, args.concurrency)?;
    let author_id = normalize_author_id(&args.author)?;
    let known_authors = load_authors(Path::new(DEFAULT_AUTHORS_PATH)).unwrap_or_default();
    let author_name = known_authors
        .get(&author_id)
        .and_then(|info| info.name.as_deref());
    let store = Store::open(&cli.database)?;
    let options = json!({
        "author_id": author_id,
        "pages": args.pages,
        "fetch_body": args.fetch_body,
        "resume": args.resume,
        "concurrency": args.concurrency,
        "raw_dir": args.raw_dir.as_ref(),
    });
    let run_id = store.create_run("author", &options.to_string(), args.pages as i64)?;
    let client = build_client(cli)?;
    let mut stats = RunStats {
        requested_pages: args.pages as i64,
        ..Default::default()
    };
    let mut articles = crawl_author_lists(
        cli,
        &client,
        &store,
        run_id,
        &author_id,
        author_name,
        args.pages,
        args.raw_dir.as_deref(),
        true,
        &mut stats,
    )
    .await?;
    stats.discovered_count = articles.len() as i64;

    if args.fetch_body {
        if args.resume {
            retain_unparsed(&store, &mut articles)?;
        }
        stats.body_attempted_count = articles.len() as i64;
        let articles =
            fetch_body_batch(client, articles, args.concurrency, args.raw_dir.as_deref()).await;
        persist_body_results(&store, run_id, &articles, &mut stats)?;
    }

    finish_run(&store, run_id, &stats)?;
    print_run_summary(run_id, "author", &stats);
    Ok(())
}

async fn run_vip(cli: &Cli, args: &VipArgs) -> Result<()> {
    validate_common(args.pages, args.concurrency)?;
    let authors = load_authors(&args.authors)?;
    let selected_authors = authors
        .into_iter()
        .take(args.max_authors.unwrap_or(usize::MAX))
        .collect::<Vec<_>>();
    if selected_authors.is_empty() {
        bail!("authors file contains no authors");
    }

    let requested_pages = selected_authors.len() as i64 * args.pages as i64;
    let store = Store::open(&cli.database)?;
    let options = json!({
        "authors": &args.authors,
        "author_count": selected_authors.len(),
        "pages": args.pages,
        "fetch_body": args.fetch_body,
        "resume": args.resume,
        "fallback_top": args.fallback_top,
        "concurrency": args.concurrency,
        "raw_dir": args.raw_dir.as_ref(),
    });
    let run_id = store.create_run("vip", &options.to_string(), requested_pages)?;
    let client = build_client(cli)?;
    let mut stats = RunStats {
        requested_pages,
        ..Default::default()
    };
    let mut selected = Vec::new();
    let mut seen = HashSet::new();

    for (author_id, info) in selected_authors {
        let before_failures = stats.failed_count;
        let discovered = crawl_author_lists(
            cli,
            &client,
            &store,
            run_id,
            &author_id,
            info.name.as_deref(),
            args.pages,
            args.raw_dir.as_deref(),
            false,
            &mut stats,
        )
        .await?;
        let mut marked = discovered
            .iter()
            .filter(|article| {
                article
                    .tags
                    .iter()
                    .any(|tag| tag == "精华" || tag == "置顶")
            })
            .cloned()
            .collect::<Vec<_>>();
        if marked.is_empty() && args.fallback_top > 0 {
            marked = discovered
                .into_iter()
                .filter(|article| {
                    article.source_rank.unwrap_or(i64::MAX) <= args.fallback_top as i64
                })
                .take(args.fallback_top)
                .collect();
        }
        for mut article in marked {
            article.source = "vip".into();
            if seen.insert(article.article_id.clone()) {
                store.record_article(run_id, &article)?;
                selected.push(article);
            }
        }
        if before_failures != stats.failed_count {
            tracing::warn!(author_id, "author crawl completed with errors");
        }
    }
    stats.discovered_count = selected.len() as i64;

    if args.fetch_body {
        if args.resume {
            retain_unparsed(&store, &mut selected)?;
        }
        stats.body_attempted_count = selected.len() as i64;
        let articles =
            fetch_body_batch(client, selected, args.concurrency, args.raw_dir.as_deref()).await;
        persist_body_results(&store, run_id, &articles, &mut stats)?;
    }

    finish_run(&store, run_id, &stats)?;
    print_run_summary(run_id, "vip", &stats);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn crawl_author_lists(
    cli: &Cli,
    client: &TgbClient,
    store: &Store,
    run_id: i64,
    author_id: &str,
    author_name: Option<&str>,
    pages: u32,
    raw_dir: Option<&Path>,
    persist_discovered: bool,
    stats: &mut RunStats,
) -> Result<Vec<Article>> {
    let mut articles = Vec::new();
    let mut seen = HashSet::new();
    for page in 1..=pages {
        let path = if page == 1 {
            format!("/blog/{author_id}")
        } else {
            format!("/blog/{author_id}?page={page}")
        };
        let url = join_base(&cli.base_url, &path)?;
        tracing::info!(author_id, page, pages, url = %url, "fetching author list");
        match client.get_text(&url).await {
            Ok(response) => {
                stats.list_pages_fetched += 1;
                if let Some(raw_dir) = raw_dir
                    && let Err(error) = save_raw(
                        raw_dir,
                        &format!("author-{author_id}-page-{page}"),
                        &response.text,
                    )
                {
                    tracing::warn!(%error, "failed saving raw author list");
                }
                match parse_blog_list(&response.text, &cli.base_url, author_id, author_name, page) {
                    Ok(found) if found.is_empty() => break,
                    Ok(found) => {
                        for article in found {
                            if seen.insert(article.article_id.clone()) {
                                if persist_discovered {
                                    store.record_article(run_id, &article)?;
                                }
                                articles.push(article);
                            }
                        }
                    }
                    Err(error) => {
                        stats.failed_count += 1;
                        store.record_error(
                            run_id,
                            "parse_author_list",
                            &url,
                            &error.to_string(),
                            Some(response.status as i64),
                        )?;
                    }
                }
            }
            Err(error) => {
                stats.failed_count += 1;
                store.record_error(run_id, "fetch_author_list", &url, &error.to_string(), None)?;
                break;
            }
        }
    }
    Ok(articles)
}

async fn fetch_body_batch(
    client: TgbClient,
    articles: Vec<Article>,
    concurrency: usize,
    raw_dir: Option<&Path>,
) -> Vec<Article> {
    let raw_dir = raw_dir.map(Path::to_path_buf);
    let mut results = stream::iter(articles.into_iter().map(|article| {
        let client = client.clone();
        let raw_dir = raw_dir.clone();
        async move { fetch_one_body(client, article, raw_dir.as_deref()).await }
    }))
    .buffer_unordered(concurrency)
    .collect::<Vec<_>>()
    .await;
    results.sort_by_key(|article| article.source_rank.unwrap_or(i64::MAX));
    results
}

async fn fetch_one_body(
    client: TgbClient,
    mut article: Article,
    raw_dir: Option<&Path>,
) -> Article {
    match client.get_text(&article.url).await {
        Ok(response) => {
            article.url = response.final_url;
            if let Some(raw_dir) = raw_dir {
                match save_raw(raw_dir, &article.article_id, &response.text) {
                    Ok(path) => article.raw_html_path = Some(path.display().to_string()),
                    Err(error) => tracing::warn!(
                        article_id = article.article_id,
                        %error,
                        "failed saving raw article"
                    ),
                }
            }
            apply_article_body(&mut article, &response.text, response.status);
        }
        Err(error) => article.mark_fetch_error(error.to_string()),
    }
    article
}

fn persist_body_results(
    store: &Store,
    run_id: i64,
    articles: &[Article],
    stats: &mut RunStats,
) -> Result<()> {
    for article in articles {
        store.record_article(run_id, article)?;
        if article.http_status.is_some() {
            stats.body_fetched_count += 1;
        }
        if article.body_succeeded() {
            stats.parsed_count += 1;
        } else {
            stats.failed_count += 1;
            store.record_error(
                run_id,
                "parse_body",
                &article.url,
                article.error.as_deref().unwrap_or("article body failed"),
                article.http_status.map(i64::from),
            )?;
        }
    }
    Ok(())
}

fn run_export(cli: &Cli, args: &ExportArgs) -> Result<()> {
    let store = Store::open(&cli.database)?;
    if let Some(run_id) = args.run
        && store.get_run(run_id)?.is_none()
    {
        bail!("run {run_id} does not exist");
    }
    let articles = store.articles_for_export(args.run, args.only_success)?;
    let output = export::render(&articles, args.format)?;
    export::write_output(&output, args.output.as_deref())?;
    if args.output.is_some() {
        eprintln!("exported {} articles", articles.len());
    }
    Ok(())
}

fn show_run_list(store: &Store, limit: usize) -> Result<()> {
    let runs = store.list_runs(limit)?;
    println!("{}", serde_json::to_string_pretty(&runs)?);
    Ok(())
}

fn show_run(store: &Store, run_id: i64) -> Result<()> {
    let run = store
        .get_run(run_id)?
        .ok_or_else(|| anyhow!("run {run_id} does not exist"))?;
    let errors = store.get_run_errors(run_id)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({"run": run, "errors": errors}))?
    );
    Ok(())
}

fn load_authors(path: &Path) -> Result<BTreeMap<String, AuthorInfo>> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(_) if path == Path::new(DEFAULT_AUTHORS_PATH) => BUILTIN_AUTHORS_JSON.to_owned(),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed reading authors file {}", path.display()));
        }
    };
    parse_authors(&content, &path.display().to_string())
}

fn parse_authors(content: &str, source: &str) -> Result<BTreeMap<String, AuthorInfo>> {
    let value: serde_json::Value =
        serde_json::from_str(content).with_context(|| format!("invalid authors JSON {source}"))?;
    let authors = value.get("authors").unwrap_or(&value);
    serde_json::from_value(authors.clone())
        .with_context(|| format!("authors must be an object in {source}"))
}

fn save_raw(directory: &Path, name: &str, html: &str) -> Result<PathBuf> {
    fs::create_dir_all(directory)
        .with_context(|| format!("failed creating raw directory {}", directory.display()))?;
    let safe_name = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    let path = directory.join(format!("{safe_name}.html"));
    fs::write(&path, html).with_context(|| format!("failed writing {}", path.display()))?;
    Ok(path)
}

fn join_base(base_url: &str, path: &str) -> Result<String> {
    let base = Url::parse(base_url).context("invalid --base-url")?;
    Ok(base.join(path)?.to_string())
}

fn validate_common(pages: u32, concurrency: usize) -> Result<()> {
    if pages == 0 {
        bail!("--pages must be at least 1");
    }
    if concurrency == 0 {
        bail!("--concurrency must be at least 1");
    }
    Ok(())
}

fn retain_unparsed(store: &Store, articles: &mut Vec<Article>) -> Result<()> {
    let mut pending = Vec::with_capacity(articles.len());
    for article in articles.drain(..) {
        if !store.has_parsed_body(&article.article_id)? {
            pending.push(article);
        }
    }
    *articles = pending;
    Ok(())
}

fn finish_run(store: &Store, run_id: i64, stats: &RunStats) -> Result<()> {
    let status = if stats.failed_count == 0 {
        "success"
    } else if stats.discovered_count == 0
        || stats.parsed_count == 0 && stats.body_attempted_count > 0
    {
        "failed"
    } else {
        "partial"
    };
    store.finish_run(run_id, status, stats)
}

fn print_run_summary(run_id: i64, command: &str, stats: &RunStats) {
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "run_id": run_id,
            "command": command,
            "requested_pages": stats.requested_pages,
            "list_pages_fetched": stats.list_pages_fetched,
            "discovered": stats.discovered_count,
            "body_attempted": stats.body_attempted_count,
            "body_fetched": stats.body_fetched_count,
            "parsed": stats.parsed_count,
            "failed": stats.failed_count,
        }))
        .expect("run summary is serializable")
    );
}

#[cfg(test)]
mod tests {
    use super::{BUILTIN_AUTHORS_JSON, parse_authors};

    #[test]
    fn bundled_author_catalog_is_valid_and_complete() {
        let authors = parse_authors(BUILTIN_AUTHORS_JSON, "bundled catalog").unwrap();
        assert_eq!(authors.len(), 116);
        assert_eq!(
            authors
                .get("134434")
                .and_then(|author| author.name.as_deref()),
            Some("炒股养家")
        );
    }
}
