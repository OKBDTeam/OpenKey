//! openkey-lexicon：CC0 种子词库 → rime 词典构建器（docs/02 5.6 / 5.1）。
//!
//! 用法：
//! ```text
//! openkey-lexicon build --seed shared/schemas/lexicon/openkey-base.seed.tsv \
//!     --out shared/schemas/lexicon --version 0.1.0
//! ```
//! 产物：
//! - `openkey-base.dict.yaml`：rime 词典源（word \t pinyin \t weight），
//!   由 librime 部署期编译为 table.bin / prism.bin（各方案经 translator.prism
//!   构建独立拼写索引，见 shared/schemas/*.schema.yaml）。
//! - `openkey-base.manifest.json`：词库包清单（版本 / 条目数 / SHA-256 / 生成时间），
//!   供词库中心组件分发与「标注版本与哈希」（5.6）使用。

mod syllables;

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::syllables::is_valid_syllable;

/// 词典名（与 shared/schemas 各方案的 `translator.dictionary` 一致）。
const DICT_NAME: &str = "openkey_base";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("种子文件第 {line} 行格式错误（应为 词条\\t拼音\\t词频）: {content}")]
    BadLine { line: usize, content: String },
    #[error("种子文件第 {line} 行词频非 1..=100000 整数: {freq}")]
    BadFreq { line: usize, freq: String },
    #[error("种子文件第 {line} 行拼音含无效音节 {syllable:?}: {pinyin}")]
    BadSyllable {
        line: usize,
        syllable: String,
        pinyin: String,
    },
    #[error("种子文件第 {line} 行与第 {first} 行重复（词条+拼音相同）: {word} {pinyin}")]
    Duplicate {
        line: usize,
        first: usize,
        word: String,
        pinyin: String,
    },
}

pub type Result<T> = std::result::Result<T, Error>;

/// 一条词料。
#[derive(Debug, Clone)]
struct Entry {
    word: String,
    pinyin: String,
    freq: u32,
}

/// 读取并校验种子词库。
fn load_seed(path: &Path) -> Result<Vec<Entry>> {
    let text = fs::read_to_string(path)?;
    let mut entries = Vec::new();
    let mut seen: std::collections::HashMap<(String, String), usize> =
        std::collections::HashMap::new();
    for (idx, raw) in text.lines().enumerate() {
        let line = idx + 1;
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = trimmed.split('\t').collect();
        if cols.len() != 3 {
            return Err(Error::BadLine {
                line,
                content: trimmed.to_string(),
            });
        }
        let word = cols[0].trim();
        let pinyin = cols[1].trim();
        let freq: u32 = cols[2].trim().parse().map_err(|_| Error::BadFreq {
            line,
            freq: cols[2].trim().to_string(),
        })?;
        if word.is_empty() || pinyin.is_empty() || !(1..=100_000).contains(&freq) {
            return Err(Error::BadFreq {
                line,
                freq: format!("{freq}（word={word:?} pinyin={pinyin:?}）"),
            });
        }
        for syllable in pinyin.split(' ') {
            if !is_valid_syllable(syllable) {
                return Err(Error::BadSyllable {
                    line,
                    syllable: syllable.to_string(),
                    pinyin: pinyin.to_string(),
                });
            }
        }
        let key = (word.to_string(), pinyin.to_string());
        if let Some(&first) = seen.get(&key) {
            return Err(Error::Duplicate {
                line,
                first,
                word: word.to_string(),
                pinyin: pinyin.to_string(),
            });
        }
        seen.insert(key, line);
        entries.push(Entry {
            word: word.to_string(),
            pinyin: pinyin.to_string(),
            freq,
        });
    }
    if entries.is_empty() {
        return Err(Error::BadLine {
            line: 0,
            content: "<空文件>".to_string(),
        });
    }
    Ok(entries)
}

/// 生成 rime 词典 YAML 文本。
fn render_dict(entries: &[Entry], version: &str) -> String {
    // 按词频降序、同频按词条排序，保证产物确定性与 sort: by_weight 语义。
    let mut sorted = entries.to_vec();
    sorted.sort_by(|a, b| b.freq.cmp(&a.freq).then_with(|| a.word.cmp(&b.word)));

    let mut out = String::new();
    out.push_str("# Rime dictionary\n");
    out.push_str("# encoding: utf-8\n");
    out.push_str("# 本文件由 tools/lexicon 从 CC0 自制种子词库生成，请勿手改；\n");
    out.push_str("# 重新生成：cargo run --manifest-path tools/lexicon/Cargo.toml -- build ...\n");
    out.push_str("---\n");
    out.push_str(&format!("name: {DICT_NAME}\n"));
    out.push_str(&format!("version: \"{version}\"\n"));
    out.push_str("sort: by_weight\n");
    out.push_str("use_preset_vocabulary: false\n");
    out.push_str("...\n");
    for e in &sorted {
        out.push_str(&format!("{}\t{}\t{}\n", e.word, e.pinyin, e.freq));
    }
    out
}

/// 构建：种子 → dict.yaml + manifest.json。
fn build(seed: &Path, out_dir: &Path, version: &str) -> Result<()> {
    let entries = load_seed(seed)?;
    let dict_text = render_dict(&entries, version);

    let mut hasher = Sha256::new();
    hasher.update(dict_text.as_bytes());
    let sha256 = hex::encode(hasher.finalize());

    fs::create_dir_all(out_dir)?;
    let dict_path = out_dir.join(format!("{DICT_NAME}.dict.yaml"));
    fs::write(&dict_path, dict_text.as_bytes())?;

    let manifest = serde_json::json!({
        "name": DICT_NAME,
        "version": version,
        "entries": entries.len(),
        "sha256": sha256,
        "license": "CC0-1.0",
        "source": "OpenKey 自制（DR-001）",
        "generated_by": "openkey-lexicon",
    });
    let manifest_path = out_dir.join(format!("{DICT_NAME}.manifest.json"));
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap_or_default(),
    )?;

    log::info!(
        "词库构建完成：{} 条 → {}（sha256={}）",
        entries.len(),
        dict_path.display(),
        &sha256[..16]
    );
    println!(
        "{} 条词条 → {}\n清单 → {}",
        entries.len(),
        dict_path.display(),
        manifest_path.display()
    );
    Ok(())
}

/// 校验：仅解析与校验种子，不写产物（CI 的词料门禁）。
fn check(seed: &Path) -> Result<()> {
    let entries = load_seed(seed)?;
    let words: BTreeSet<&str> = entries.iter().map(|e| e.word.as_str()).collect();
    println!(
        "种子词库校验通过：{} 条（{} 个不同词条）",
        entries.len(),
        words.len()
    );
    Ok(())
}

fn main() {
    env_logger_lite::init();

    let args: Vec<String> = std::env::args().collect();
    let mut command = String::new();
    let mut seed: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut version = "0.1.0".to_string();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "build" | "check" => command = args[i].clone(),
            "--seed" => {
                i += 1;
                seed = args.get(i).map(PathBuf::from);
            }
            "--out" => {
                i += 1;
                out = args.get(i).map(PathBuf::from);
            }
            "--version" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    version = v.clone();
                }
            }
            other => {
                eprintln!("未知参数: {other}");
                print_usage();
                std::process::exit(2);
            }
        }
        i += 1;
    }

    let result = match command.as_str() {
        "build" => match (seed, out) {
            (Some(seed), Some(out)) => build(&seed, &out, &version),
            _ => {
                print_usage();
                std::process::exit(2);
            }
        },
        "check" => match seed {
            Some(seed) => check(&seed),
            None => {
                print_usage();
                std::process::exit(2);
            }
        },
        _ => {
            print_usage();
            std::process::exit(2);
        }
    };

    if let Err(e) = result {
        eprintln!("错误: {e}");
        std::process::exit(1);
    }
}

fn print_usage() {
    eprintln!(
        "用法:\n  openkey-lexicon build --seed <seed.tsv> --out <目录> [--version <版本>]\n  \
         openkey-lexicon check --seed <seed.tsv>"
    );
}

/// 极简日志初始化（无 env_logger 依赖）：OPENKEY_LOG 非空时输出日志。
mod env_logger_lite {
    pub fn init() {
        let level = std::env::var("OPENKEY_LOG").unwrap_or_default();
        if level.is_empty() {
            return;
        }
        log::set_max_level(if level == "debug" {
            log::LevelFilter::Debug
        } else {
            log::LevelFilter::Info
        });
        let _ = log::set_boxed_logger(Box::new(Logger));
    }

    struct Logger;

    impl log::Log for Logger {
        fn enabled(&self, _: &log::Metadata) -> bool {
            true
        }
        fn log(&self, record: &log::Record) {
            eprintln!("[{}] {}", record.level(), record.args());
        }
        fn flush(&self) {}
    }
}
