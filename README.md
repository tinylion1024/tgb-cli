# tgb-cli

[![CI](https://github.com/tinylion1024/tgb-cli/actions/workflows/ci.yml/badge.svg)](https://github.com/tinylion1024/tgb-cli/actions/workflows/ci.yml)
[![GitHub Release](https://img.shields.io/github/v/release/tinylion1024/tgb-cli)](https://github.com/tinylion1024/tgb-cli/releases)
[![License](https://img.shields.io/github/license/tinylion1024/tgb-cli)](LICENSE)

`tgb-cli` 是一个基于 Rust 的淘股吧公开文章采集工具。它可以抓取热门榜、
单篇文章、指定大 V 博客和大 V 精选文章，并将结果保存到 SQLite，供后续分析、
检索或导出。

> 本工具只处理淘股吧公开页面。请合理设置抓取频率，并遵守目标网站的服务条款。

## 主要功能

- 抓取指定时间范围内的点赞榜热门文章
- 通过作者 ID 或博客地址抓取指定大 V 的最新文章
- 使用内置大 V 目录批量抓取精选、置顶文章
- 抓取文章正文，并记录解析状态和失败原因
- 支持断点续抓、请求重试、全局限速和原始 HTML 留档
- 使用 SQLite 保存文章、采集批次和错误记录
- 导出 JSONL、CSV、Markdown 或纯文本

## 安装

### Homebrew（推荐）

```bash
brew install tinylion1024/tap/tgb-cli
```

Homebrew 软件包名是 `tgb-cli`，安装后的命令名是 `tgb`：

```bash
tgb --version
tgb --help
```

升级或重新安装：

```bash
brew upgrade tgb-cli
brew reinstall tgb-cli
```

### Cargo

需要 Rust 1.85 或更高版本：

```bash
cargo install --git https://github.com/tinylion1024/tgb-cli --locked
```

### 从源码构建

```bash
git clone https://github.com/tinylion1024/tgb-cli.git
cd tgb-cli
cargo build --release
```

生成的可执行文件位于 `target/release/tgb`。

## 快速开始

抓取“炒股养家”最近 3 页文章及正文：

```bash
tgb author 134434 \
  --pages 3 \
  --fetch-body \
  --resume
```

查看最近的采集批次：

```bash
tgb run list
```

假设刚才的运行编号是 `1`，将成功解析的文章导出为 Markdown：

```bash
tgb export \
  --run 1 \
  --only-success \
  --format markdown \
  --output articles.md
```

默认数据库是当前工作目录下的 `data/tgb.db`。建议在固定目录运行，或者始终通过
`--database` 指定同一个数据库文件。

## 获取热门文章

使用 `hot` 抓取点赞榜中指定时间范围内的文章：

```bash
tgb hot \
  --from "2026-07-28 00:00" \
  --to "2026-07-29 23:59" \
  --pages 5 \
  --fetch-body
```

常用参数：

| 参数 | 说明 | 默认值 |
| --- | --- | --- |
| `--from` | 开始时间，必填 | - |
| `--to` | 结束时间，必填 | - |
| `--pages` | 扫描的点赞榜页数 | `5` |
| `--fetch-body` | 继续请求每篇文章并解析正文 | 关闭 |
| `--concurrency` | 正文请求并发数 | `2` |
| `--raw-dir` | 保存文章原始 HTML 的目录 | 不保存 |

当前版本支持以下时间格式：

- `2026-07-28 09:30`
- `2026-07-28T09:30:00+08:00`
- `07-28 09:30`

纯日期 `2026-07-28` 暂不支持；需要显式补充时间。若要覆盖完整自然日，
开始时间使用 `00:00`，结束时间使用 `23:59`。

## 获取单篇文章

可以传入文章 ID：

```bash
tgb article 2rjdXpk0pCK
```

也可以直接传入包含 `/a/<文章 ID>` 的完整地址：

```bash
tgb article https://www.tgb.cn/a/2rjdXpk0pCK
```

默认会将结果写入数据库，并在终端输出文章 JSON。只想查看、不写数据库时：

```bash
tgb article 2rjdXpk0pCK --no-save
```

保存原始页面，方便站点结构变化后重新分析：

```bash
tgb article 2rjdXpk0pCK --raw-dir data/raw
```

## 获取指定大 V 的文章

`author` 接受作者数字 ID 或博客 URL，并按博客最新页向后抓取。

通过作者 ID 抓取：

```bash
tgb author 134434 \
  --pages 3 \
  --fetch-body \
  --resume
```

通过博客 URL 抓取：

```bash
tgb author https://www.tgb.cn/blog/134434 \
  --pages 5 \
  --fetch-body
```

内置目录中的部分作者示例：

| 作者 | 作者 ID | 博客地址 |
| --- | ---: | --- |
| 职业炒手 | `4223` | `https://www.tgb.cn/blog/4223` |
| 炒股养家 | `134434` | `https://www.tgb.cn/blog/134434` |
| 赵老哥 | `154034` | `https://www.tgb.cn/blog/154034` |

常用参数：

| 参数 | 说明 | 默认值 |
| --- | --- | --- |
| `--pages` | 抓取的博客页数 | `3` |
| `--fetch-body` | 同时抓取文章正文 | 关闭 |
| `--resume` | 数据库已有成功正文时跳过重复请求 | 关闭 |
| `--concurrency` | 正文请求并发数 | `2` |
| `--raw-dir` | 保存原始 HTML 的目录 | 不保存 |

`author` 当前按最新页数抓取，暂不支持按日期过滤。

## 批量获取大 V 精选文章

`vip` 使用项目内置的 116 位大 V 目录，批量抓取博客中的精选或置顶文章：

```bash
tgb vip \
  --pages 2 \
  --fetch-body \
  --resume
```

第一次运行时可以先限制作者数量：

```bash
tgb vip \
  --max-authors 5 \
  --pages 2 \
  --fetch-body
```

默认情况下，页面没有精选或置顶标记时不会猜测候选文章。若希望这种情况下取
每位作者首页前 3 篇：

```bash
tgb vip \
  --pages 2 \
  --fetch-body \
  --fallback-top 3 \
  --resume
```

常用参数：

| 参数 | 说明 | 默认值 |
| --- | --- | --- |
| `--pages` | 每位作者抓取的博客页数 | `1` |
| `--max-authors` | 最多处理多少位作者 | 不限制 |
| `--fetch-body` | 同时抓取文章正文 | 关闭 |
| `--resume` | 跳过已有成功正文的文章 | 关闭 |
| `--fallback-top` | 无精选标记时，取首页前 N 篇 | `0` |
| `--concurrency` | 正文请求并发数 | `2` |
| `--raw-dir` | 保存原始 HTML 的目录 | 不保存 |

### 使用自定义大 V 名单

创建一个 JSON 文件，例如 `authors.json`：

```json
{
  "authors": {
    "4223": {
      "name": "职业炒手",
      "blog_url": "https://www.tgb.cn/blog/4223"
    },
    "134434": {
      "name": "炒股养家",
      "blog_url": "https://www.tgb.cn/blog/134434"
    }
  }
}
```

然后执行：

```bash
tgb vip \
  --authors ./authors.json \
  --pages 2 \
  --fetch-body \
  --fallback-top 3
```

内置完整目录位于
[`references/tgb_blog_authors.json`](references/tgb_blog_authors.json)，并已编译进
二进制，因此通过 Homebrew 安装后也能直接使用 `tgb vip`。

## 查看采集记录和错误

每次执行 `hot`、`article`、`author` 或 `vip` 都会创建一个运行记录。

查看最近 20 次运行：

```bash
tgb run list
```

只查看最近 5 次：

```bash
tgb run list --limit 5
```

查看指定运行的统计和错误明细：

```bash
tgb run show 1
```

运行状态中会记录计划页数、成功抓取页数、发现文章数、正文请求数、解析成功数和
失败数。抓取异常会保留阶段、目标地址、错误信息和 HTTP 状态码。

## 导出文章

支持四种格式：

```bash
# JSONL
tgb export --run 1 --only-success \
  --format jsonl --output articles.jsonl

# CSV
tgb export --run 1 --only-success \
  --format csv --output articles.csv

# Markdown
tgb export --run 1 --only-success \
  --format markdown --output articles.md

# 纯文本
tgb export --run 1 --only-success \
  --format text --output articles.txt
```

参数说明：

- `--run <ID>`：只导出某次运行发现的文章；省略时导出数据库中的全部文章。
- `--only-success`：只导出正文解析成功的文章。
- `--output <路径>`：写入文件；省略时直接输出到终端。

## 全局参数

以下参数可以放在任意子命令前后：

| 参数 | 说明 | 默认值 |
| --- | --- | --- |
| `--database` | SQLite 数据库路径 | `data/tgb.db` |
| `--base-url` | 淘股吧站点根地址，主要供测试或镜像环境使用 | `https://www.tgb.cn` |
| `--delay-ms` | 两次请求启动之间的最小间隔 | `1000` |
| `--max-attempts` | 瞬时错误的最大请求次数 | `3` |
| `--timeout-secs` | 单次请求超时秒数 | `20` |

例如，将数据统一写到固定位置，并将请求间隔提高到 2 秒：

```bash
tgb author 134434 \
  --database "$HOME/tgb-data/tgb.db" \
  --delay-ms 2000 \
  --pages 3 \
  --fetch-body \
  --resume
```

## 推荐工作流

```bash
# 1. 首次抓取正文
tgb author 134434 --pages 5 --fetch-body

# 2. 后续增量运行，跳过已成功解析的正文
tgb author 134434 --pages 3 --fetch-body --resume

# 3. 查看最新运行编号和统计
tgb run list

# 4. 查看失败原因
tgb run show 2

# 5. 导出本次成功文章
tgb export --run 2 --only-success \
  --format markdown --output run-2.md
```

如果你需要保留页面快照用于排查解析问题，可以在采集命令中增加：

```bash
--raw-dir data/raw
```

## 常见问题

### 安装成功后为什么找不到 `tgb-cli` 命令？

Homebrew Formula 名称是 `tgb-cli`，实际可执行文件名是 `tgb`：

```bash
tgb --help
```

可以通过下面的命令确认安装位置：

```bash
which tgb
brew list tgb-cli
```

### 为什么纯日期提示 `unsupported datetime`？

当前版本要求日期包含时间。请使用：

```bash
tgb hot \
  --from "2026-07-28 00:00" \
  --to "2026-07-29 23:59"
```

### 为什么 `vip` 没有抓到文章？

`vip` 默认只保留页面上明确标记为精选或置顶的文章。可以增加：

```bash
--fallback-top 3
```

这样在没有精选标记时，会取每位作者首页前 3 篇作为候选。

### 为什么只看到了标题，没有正文？

`hot`、`author` 和 `vip` 默认只抓文章列表。增加 `--fetch-body` 才会继续请求和
解析每篇文章正文。

### 如何降低访问频率？

增加全局请求间隔并降低并发数：

```bash
tgb vip \
  --delay-ms 2000 \
  --concurrency 1 \
  --pages 1 \
  --fetch-body
```

## 开发

```bash
cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets
cargo package --locked
```

版本发布及 Homebrew Formula 更新步骤见 [RELEASING.md](RELEASING.md)。

## 许可

[MIT](LICENSE)
