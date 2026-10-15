//! openkey-bench：黄金语料首屏命中率评测（docs/02 7.2）。
//!
//! 用法：
//! ```text
//! openkey-bench run --corpus tools/bench-corpus/corpus.tsv \
//!     --shared shared/schemas [--report bench-report.md]
//! ```
//!
//! 指标定义（与 tools/bench-corpus/README.md 一致）：
//! - 逐句输入该句的无声调拼音（字母流，由 librime 自动分词）；
//! - 输入完毕后取候选第一页，按候选顺序做「前缀贪心匹配」：
//!   句命中率 = 句子全部字符均可在第一页候选按序拼接覆盖的句数占比；
//!   字符首屏命中率 = 被覆盖字符数 / 总字符数。
//! - 跨版本回归不得劣化 >1%（7.2）。

use std::fs;
use std::path::{Path, PathBuf};

use openkey_rime_ffi::{Candidate, EngineConfig, RimeEngine};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("语料第 {line} 行格式错误（应为 句子\\t拼音）: {content}")]
    BadLine { line: usize, content: String },
    #[error("librime 初始化失败: {0}")]
    Rime(#[from] openkey_rime_ffi::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// 语料分类（按文件行序：新闻 250 / 聊天 150 / 技术 50 / 古文 50）。
const CATEGORIES: &[(&str, usize)] = &[("新闻", 250), ("聊天", 150), ("技术", 50), ("古文", 50)];

struct Sentence {
    text: String,
    pinyin: String,
}

fn load_corpus(path: &Path) -> Result<Vec<Sentence>> {
    let text = fs::read_to_string(path)?;
    let mut out = Vec::new();
    for (idx, raw) in text.lines().enumerate() {
        let line = idx + 1;
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = trimmed.split('\t').collect();
        if cols.len() != 2 || cols[0].is_empty() || cols[1].is_empty() {
            return Err(Error::BadLine {
                line,
                content: trimmed.to_string(),
            });
        }
        out.push(Sentence {
            text: cols[0].trim().to_string(),
            pinyin: cols[1].trim().to_string(),
        });
    }
    Ok(out)
}

/// 前缀贪心匹配：返回 (覆盖字符数, 总字符数)。
fn coverage(sentence: &str, candidates: &[Candidate]) -> (usize, usize) {
    let chars: Vec<char> = sentence.chars().collect();
    let total = chars.len();
    let mut covered = 0usize;
    for c in candidates {
        let text: Vec<char> = c.text.chars().collect();
        if text.is_empty() {
            continue;
        }
        if text.len() <= total - covered && chars[covered..covered + text.len()] == text[..] {
            covered += text.len();
            if covered == total {
                break;
            }
        }
    }
    (covered, total)
}

struct Stats {
    sentences: usize,
    fully_covered: usize,
    chars_covered: usize,
    chars_total: usize,
}

impl Stats {
    fn new() -> Self {
        Self {
            sentences: 0,
            fully_covered: 0,
            chars_covered: 0,
            chars_total: 0,
        }
    }

    fn add(&mut self, covered: usize, total: usize) {
        self.sentences += 1;
        if covered == total {
            self.fully_covered += 1;
        }
        self.chars_covered += covered;
        self.chars_total += total;
    }

    fn sentence_rate(&self) -> f64 {
        if self.sentences == 0 {
            0.0
        } else {
            self.fully_covered as f64 / self.sentences as f64 * 100.0
        }
    }

    fn char_rate(&self) -> f64 {
        if self.chars_total == 0 {
            0.0
        } else {
            self.chars_covered as f64 / self.chars_total as f64 * 100.0
        }
    }
}

fn category_of(index: usize) -> &'static str {
    let mut acc = 0usize;
    for (name, count) in CATEGORIES {
        acc += count;
        if index < acc {
            return name;
        }
    }
    "其他"
}

fn run(corpus: &Path, shared: &Path, report: Option<&Path>) -> Result<()> {
    let sentences = load_corpus(corpus)?;
    if sentences.is_empty() {
        return Err(Error::BadLine {
            line: 0,
            content: "<空语料>".to_string(),
        });
    }

    // 临时用户数据目录（避免污染开发者本机 rime 配置）。
    let user_dir = std::env::temp_dir().join(format!("openkey-bench-user-{}", std::process::id()));
    let _ = fs::remove_dir_all(&user_dir);
    fs::create_dir_all(&user_dir)?;

    let config = EngineConfig {
        shared_data_dir: Some(shared.to_path_buf()),
        user_data_dir: Some(user_dir.clone()),
        min_log_level: 3,
        ..Default::default()
    };
    let engine = RimeEngine::init(&config)?;
    let session = engine.create_session()?;

    let mut total = Stats::new();
    let mut per_category: std::collections::BTreeMap<&str, Stats> =
        std::collections::BTreeMap::new();

    for (index, sentence) in sentences.iter().enumerate() {
        session.clear_composition();
        // 逐字母输入拼音（跳过音节间空格；librime 自动分词）。
        for ch in sentence.pinyin.chars() {
            if ch == ' ' {
                continue;
            }
            let keycode = ch as i32;
            if !(b'a' as i32..=b'z' as i32).contains(&keycode) {
                continue;
            }
            session.process_key(keycode, 0)?;
        }
        let ctx = session.context()?;
        let (covered, chars) = coverage(&sentence.text, &ctx.candidates);
        total.add(covered, chars);
        per_category
            .entry(category_of(index))
            .or_insert_with(Stats::new)
            .add(covered, chars);
    }

    drop(session);
    let _ = fs::remove_dir_all(&user_dir);

    // 汇总输出。
    println!("语料 {} 句（{} 字符）", total.sentences, total.chars_total);
    println!(
        "{:<6} {:>6} {:>10} {:>14}",
        "分类", "句数", "句命中率", "字符首屏命中率"
    );
    for (name, stats) in &per_category {
        println!(
            "{:<6} {:>6} {:>9.2}% {:>13.2}%",
            name,
            stats.sentences,
            stats.sentence_rate(),
            stats.char_rate()
        );
    }
    println!(
        "{:<6} {:>6} {:>9.2}% {:>13.2}%",
        "合计",
        total.sentences,
        total.sentence_rate(),
        total.char_rate()
    );

    if let Some(report_path) = report {
        let mut md = String::new();
        md.push_str("# OpenKey 首屏命中率基准报告\n\n");
        md.push_str(&format!(
            "- 语料：`{}`（{} 句 / {} 字符）\n",
            corpus.display(),
            total.sentences,
            total.chars_total
        ));
        md.push_str("- 指标：句命中率（整句可在第一页候选按序覆盖）、字符首屏命中率\n");
        md.push_str("- 回归门禁：跨版本劣化 >1% 需说明（docs/02 7.2）\n\n");
        md.push_str("| 分类 | 句数 | 句命中率 | 字符首屏命中率 |\n");
        md.push_str("|---|---|---|---|\n");
        for (name, stats) in &per_category {
            md.push_str(&format!(
                "| {} | {} | {:.2}% | {:.2}% |\n",
                name,
                stats.sentences,
                stats.sentence_rate(),
                stats.char_rate()
            ));
        }
        md.push_str(&format!(
            "| **合计** | **{}** | **{:.2}%** | **{:.2}%** |\n",
            total.sentences,
            total.sentence_rate(),
            total.char_rate()
        ));
        if let Some(parent) = report_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(report_path, md)?;
        println!("报告已写入 {}", report_path.display());
    }

    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut command = String::new();
    let mut corpus: Option<PathBuf> = None;
    let mut shared: Option<PathBuf> = None;
    let mut report: Option<PathBuf> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "run" => command = args[i].clone(),
            "--corpus" => {
                i += 1;
                corpus = args.get(i).map(PathBuf::from);
            }
            "--shared" => {
                i += 1;
                shared = args.get(i).map(PathBuf::from);
            }
            "--report" => {
                i += 1;
                report = args.get(i).map(PathBuf::from);
            }
            other => {
                eprintln!("未知参数: {other}");
                eprintln!(
                    "用法: openkey-bench run --corpus <corpus.tsv> --shared <方案目录> [--report <out.md>]"
                );
                std::process::exit(2);
            }
        }
        i += 1;
    }

    let result = match command.as_str() {
        "run" => match (corpus, shared) {
            (Some(corpus), Some(shared)) => run(&corpus, &shared, report.as_deref()),
            _ => {
                eprintln!(
                    "用法: openkey-bench run --corpus <corpus.tsv> --shared <方案目录> [--report <out.md>]"
                );
                std::process::exit(2);
            }
        },
        _ => {
            eprintln!(
                "用法: openkey-bench run --corpus <corpus.tsv> --shared <方案目录> [--report <out.md>]"
            );
            std::process::exit(2);
        }
    };

    if let Err(e) = result {
        eprintln!("错误: {e}");
        std::process::exit(1);
    }
}
