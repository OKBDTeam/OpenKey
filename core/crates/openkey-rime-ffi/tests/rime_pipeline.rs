//! 输入管线集成测试（docs/02 5.1 / 7.1）。
//!
//! librime 为进程级单例，全部用例在同一测试函数内串行执行；
//! 仅当启用 `librime` 特性（默认）时编译运行。

#![cfg(feature = "librime")]

use openkey_rime_ffi::{EngineConfig, RimeEngine};

const DEFAULT_YAML: &str = r#"
config_version: "0.16.minimal"

schema_list:
  - schema: openkey_test
"#;

const SCHEMA_YAML: &str = r#"
schema:
  schema_id: openkey_test
  name: OpenKey 测试方案
  version: "0.1"

engine:
  processors:
    - speller
    - selector
    - navigator
  segmentors:
    - ascii_segmentor
    - matcher
    - abc_segmentor
    - fallback_segmentor
  translators:
    - script_translator
  filters:
    - uniquifier

speller:
  alphabet: zyxwvutsrqponmlkjihgfedcba
  delimiter: " '"

translator:
  dictionary: openkey_test
"#;

const DICT_YAML: &str = r#"
---
name: openkey_test
version: "0.1"
sort: by_weight
use_preset_vocabulary: false
...
你	ni
好	hao
尼	ni
娜	na
拿	na
男	nan
南	nan
脑	nao
内	nei
能	neng
年	nian
鸟	niao
宁	ning
牛	niu
农	nong
怒	nu
暖	nuan
女	nv
世	shi
界	jie
中	zhong
国	guo
你好	ni hao
你好吗	ni hao ma
世界	shi jie
中国	zhong guo
中国人	zhong guo ren
"#;

const XK_N: i32 = b'n' as i32;
const XK_I: i32 = b'i' as i32;
const XK_H: i32 = b'h' as i32;
const XK_A: i32 = b'a' as i32;
const XK_O: i32 = b'o' as i32;

fn write_shared_data(dir: &std::path::Path) {
    std::fs::write(dir.join("default.yaml"), DEFAULT_YAML).unwrap();
    std::fs::write(dir.join("openkey_test.schema.yaml"), SCHEMA_YAML).unwrap();
    std::fs::write(dir.join("openkey_test.dict.yaml"), DICT_YAML).unwrap();
}

fn type_seq(session: &openkey_rime_ffi::Session, keys: &[i32]) {
    for &k in keys {
        assert!(session.process_key(k, 0).unwrap());
    }
}

#[test]
fn input_pipeline_end_to_end() {
    let base = std::env::temp_dir().join(format!("openkey-rime-test-{}", std::process::id()));
    let shared = base.join("shared");
    let user = base.join("user");
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&shared).unwrap();
    std::fs::create_dir_all(&user).unwrap();
    write_shared_data(&shared);

    let config = EngineConfig {
        shared_data_dir: Some(shared.clone()),
        user_data_dir: Some(user.clone()),
        min_log_level: 3,
        ..Default::default()
    };
    let engine = RimeEngine::init(&config).expect("librime 初始化失败");
    assert!(!RimeEngine::version().is_empty());

    // 方案列表包含测试方案。
    let schemas = engine.schema_list();
    assert!(
        schemas.iter().any(|(id, _)| id == "openkey_test"),
        "方案列表异常: {schemas:?}"
    );

    let session = engine.create_session().expect("创建会话失败");

    // 输入 nihao → 组词 → 候选包含“你好”。
    type_seq(&session, &[XK_N, XK_I, XK_H, XK_A, XK_O]);
    let ctx = session.context().unwrap();
    // 容忍 librime 在音节间插入分隔符的版本差异
    let preedit = ctx.preedit.clone().unwrap_or_default();
    assert_eq!(
        preedit.replace(' ', ""),
        "nihao",
        "preedit 应为 nihao（可含空格分隔）"
    );
    assert!(ctx.has_menu(), "应有候选");
    assert!(
        ctx.candidates.iter().any(|c| c.text == "你好"),
        "候选应包含“你好”: {:?}",
        ctx.candidates
    );

    // 选择首个候选 → 上屏文本为“你好”。
    let index = ctx
        .candidates
        .iter()
        .position(|c| c.text == "你好")
        .expect("候选列表应含“你好”");
    assert!(session.select_candidate(index).unwrap());
    let after = session.context().unwrap();
    let committed = session.commit().unwrap();
    assert_eq!(
        committed.as_deref(),
        Some("你好"),
        "选择后未上屏：preedit={:?} preview={:?} candidates={}",
        after.preedit,
        after.commit_text_preview,
        after.candidates.len()
    );

    // 上屏后回到空闲。
    let ctx = session.context().unwrap();
    assert!(!ctx.is_composing());

    // 单字母输入 → 候选多于 5 → 翻页后内容变化。
    type_seq(&session, &[XK_N]);
    let page1 = session.context().unwrap();
    assert!(
        page1.candidates.len() > 5,
        "候选应超过一页: {}",
        page1.candidates.len()
    );
    assert!(session.change_page(false).unwrap());
    let page2 = session.context().unwrap();
    assert_ne!(page1.candidates, page2.candidates, "翻页后候选应变化");
    session.clear_composition();

    // ascii 模式切换。
    assert!(!session.get_option("ascii_mode").unwrap());
    session.set_option("ascii_mode", true).unwrap();
    assert!(session.get_option("ascii_mode").unwrap());
    let status = session.status().unwrap();
    assert!(status.is_ascii_mode);
    assert_eq!(status.schema_id, "openkey_test");
    session.set_option("ascii_mode", false).unwrap();

    // userdb 快照导出：触发同步任务后快照目录应存在（或为空目录）。
    let export_dir = base.join("export");
    engine.export_sync_snapshot(&export_dir).unwrap();
    assert!(export_dir.exists());

    // 清理。
    drop(session);
    let _ = std::fs::remove_dir_all(&base);
}
