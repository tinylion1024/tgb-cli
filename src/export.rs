use std::{
    fs,
    io::{self, Write},
    path::Path,
};

use anyhow::{Context, Result};

use crate::{cli::ExportFormat, model::Article};

pub fn render(articles: &[Article], format: ExportFormat) -> Result<Vec<u8>> {
    match format {
        ExportFormat::Jsonl => render_jsonl(articles),
        ExportFormat::Csv => render_csv(articles),
        ExportFormat::Markdown => Ok(render_markdown(articles).into_bytes()),
        ExportFormat::Text => Ok(render_text(articles).into_bytes()),
    }
}

pub fn write_output(data: &[u8], output: Option<&Path>) -> Result<()> {
    match output {
        Some(path) => {
            if let Some(parent) = path.parent()
                && !parent.as_os_str().is_empty()
            {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed creating {}", parent.display()))?;
            }
            fs::write(path, data).with_context(|| format!("failed writing {}", path.display()))
        }
        None => {
            let mut stdout = io::stdout().lock();
            stdout.write_all(data)?;
            stdout.flush()?;
            Ok(())
        }
    }
}

fn render_jsonl(articles: &[Article]) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    for article in articles {
        serde_json::to_writer(&mut output, article)?;
        output.push(b'\n');
    }
    Ok(output)
}

fn render_csv(articles: &[Article]) -> Result<Vec<u8>> {
    let mut writer = csv::Writer::from_writer(Vec::new());
    for article in articles {
        writer.serialize(article)?;
    }
    writer.flush()?;
    Ok(writer.into_inner()?)
}

fn render_markdown(articles: &[Article]) -> String {
    let mut output = String::new();
    for (index, article) in articles.iter().enumerate() {
        output.push_str(&format!(
            "## {}. {}\n\n- 作者：{}\n- 时间：{}\n- 来源：{}\n- 链接：{}\n- 解析状态：{}\n\n",
            index + 1,
            article.title,
            article.author_name.as_deref().unwrap_or("未知"),
            article
                .published_at
                .as_deref()
                .or(article.published_raw.as_deref())
                .unwrap_or("未知"),
            article.source,
            article.url,
            article.parse_status,
        ));
        if let Some(body) = &article.body_text {
            output.push_str(body);
            output.push_str("\n\n");
        }
    }
    output
}

fn render_text(articles: &[Article]) -> String {
    let mut output = String::new();
    for article in articles {
        output.push_str(&format!(
            "《{}》 - {}\n{}\n{}\n\n",
            article.title,
            article.author_name.as_deref().unwrap_or("未知"),
            article.url,
            article.body_text.as_deref().unwrap_or("")
        ));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonl_keeps_one_complete_article_per_line() {
        let article = Article::metadata(
            "x".into(),
            "标题".into(),
            "https://www.tgb.cn/a/x".into(),
            "hot",
        );
        let output = String::from_utf8(render(&[article], ExportFormat::Jsonl).unwrap()).unwrap();
        assert_eq!(output.lines().count(), 1);
        assert!(output.contains("\"article_id\":\"x\""));
    }
}
