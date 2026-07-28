use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

pub const DEFAULT_AUTHORS_PATH: &str = "references/tgb_blog_authors.json";

#[derive(Debug, Parser)]
#[command(
    name = "tgb",
    version,
    about = "淘股吧文章采集、持久化与导出工具",
    long_about = "从淘股吧公开页面抓取热门文章和大V博客，保留运行统计、解析状态与失败记录。"
)]
pub struct Cli {
    /// SQLite 数据库路径
    #[arg(long, global = true, default_value = "data/tgb.db")]
    pub database: PathBuf,

    /// 淘股吧站点根地址；测试和镜像环境可覆盖
    #[arg(long, global = true, default_value = "https://www.tgb.cn")]
    pub base_url: String,

    /// 两次请求启动之间的最小间隔
    #[arg(long, global = true, default_value_t = 1000)]
    pub delay_ms: u64,

    /// 瞬时错误的最大请求次数
    #[arg(long, global = true, default_value_t = 3)]
    pub max_attempts: u32,

    /// 单次请求超时
    #[arg(long, global = true, default_value_t = 20)]
    pub timeout_secs: u64,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// 抓取点赞榜热门文章
    Hot(HotArgs),
    /// 抓取单篇文章
    Article(ArticleArgs),
    /// 抓取指定大V博客文章
    Author(AuthorArgs),
    /// 批量抓取大V精选/置顶文章
    Vip(VipArgs),
    /// 查询采集运行记录
    Run(RunArgs),
    /// 导出结构化文章语料
    Export(ExportArgs),
}

#[derive(Debug, Args)]
pub struct HotArgs {
    /// 开始时间：RFC3339、YYYY-MM-DD HH:MM，或兼容格式 MM-DD HH:MM
    #[arg(long)]
    pub from: String,

    /// 结束时间：RFC3339、YYYY-MM-DD HH:MM，或兼容格式 MM-DD HH:MM
    #[arg(long)]
    pub to: String,

    /// 点赞榜页数
    #[arg(long, default_value_t = 5)]
    pub pages: u32,

    /// 同时抓取正文
    #[arg(long)]
    pub fetch_body: bool,

    /// 正文请求并发数；全局限速仍然生效
    #[arg(long, default_value_t = 2)]
    pub concurrency: usize,

    /// 保存原始 HTML，方便站点结构变化后离线重解析
    #[arg(long)]
    pub raw_dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct ArticleArgs {
    /// 文章 URL 或 /a/ 后的文章 ID
    pub target: String,

    /// 保存原始 HTML
    #[arg(long)]
    pub raw_dir: Option<PathBuf>,

    /// 不写入数据库，只向标准输出打印 JSON
    #[arg(long)]
    pub no_save: bool,
}

#[derive(Debug, Args)]
pub struct AuthorArgs {
    /// 作者数字 ID 或博客 URL
    pub author: String,

    /// 博客页数
    #[arg(long, default_value_t = 3)]
    pub pages: u32,

    /// 同时抓取正文
    #[arg(long)]
    pub fetch_body: bool,

    /// 已有成功正文时跳过重复请求
    #[arg(long)]
    pub resume: bool,

    /// 正文请求并发数
    #[arg(long, default_value_t = 2)]
    pub concurrency: usize,

    /// 保存原始 HTML
    #[arg(long)]
    pub raw_dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct VipArgs {
    /// 大V配置 JSON
    #[arg(long, default_value = DEFAULT_AUTHORS_PATH)]
    pub authors: PathBuf,

    /// 每位作者抓取页数
    #[arg(long, default_value_t = 1)]
    pub pages: u32,

    /// 最多处理多少位作者
    #[arg(long)]
    pub max_authors: Option<usize>,

    /// 同时抓取正文
    #[arg(long)]
    pub fetch_body: bool,

    /// 跳过已经有成功正文的文章
    #[arg(long)]
    pub resume: bool,

    /// 页面无精选标记时，取首页前 N 篇作为候选；默认不猜测
    #[arg(long, default_value_t = 0)]
    pub fallback_top: usize,

    /// 正文请求并发数
    #[arg(long, default_value_t = 2)]
    pub concurrency: usize,

    /// 保存原始 HTML
    #[arg(long)]
    pub raw_dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct RunArgs {
    #[command(subcommand)]
    pub command: RunCommand,
}

#[derive(Debug, Subcommand)]
pub enum RunCommand {
    /// 展示最近的运行记录
    List {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// 展示某次运行及错误明细
    Show { run_id: i64 },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ExportFormat {
    Jsonl,
    Csv,
    Markdown,
    Text,
}

#[derive(Debug, Args)]
pub struct ExportArgs {
    /// 只导出某次运行发现的文章；省略则导出数据库全部文章
    #[arg(long)]
    pub run: Option<i64>,

    #[arg(long, value_enum, default_value_t = ExportFormat::Jsonl)]
    pub format: ExportFormat,

    /// 输出文件；省略时写入标准输出
    #[arg(long)]
    pub output: Option<PathBuf>,

    /// 仅导出正文解析成功的文章
    #[arg(long)]
    pub only_success: bool,
}
