# shared/schemas/lexicon —— CC0 自制种子词库（docs/02 5.6）

## 词料来源

- `openkey-base.seed.tsv` 为 OpenKey **自制**词料，以 CC0-1.0 发布（DR-001 许可证策略：
  内置内容一律 CC0/自制，不从第三方语料或词典拷贝，规避 GPL 传染）。
- 词料由项目维护者人工整理：常用单字按现代汉语使用频率排序，词语覆盖
  日常、新闻、聊天、技术等域；未使用任何外部词库文件。
- 本文件是**唯一词料源**；`shared/schemas/openkey_base.dict.yaml` 等产物均由
  `tools/lexicon` 从本文件生成，请勿手改产物。

## 格式

TSV，三列，TAB 分隔，UTF-8，无表头：

```text
词条	拼音	词频
```

- **词条**：单字（1 个汉字）或词语（2–4 个汉字）；
- **拼音**：无声调拼音，多音节以单个空格分隔（如 `sou gou`、`shou ji`）；
  每个音节必须落在 `tools/lexicon/src/syllables.rs` 的有效音节表内
  （含 `lv/nv/lve/nve` 等边缘音节）；
- **词频**：1–100000 的整数，仅表示**相对**排序权重（见下）；
- 多音字占多行，每行一个读音（如 `行 xing` / `行 hang`），主读音词频高于次要读音；
- 以 `#` 开头的行与空行被构建器忽略。

## 词频语义

词频是构建器使用的**唯一排序信号**（输出 rime 词典 `sort: by_weight`），语义为：

- 同一拼音下的候选排序：词频越高越靠前（决定首屏命中）；
- 主读音 vs 次要读音：多音字主读音词频约为次要读音的 2.5 倍，
  保证常见语义排在候选第一页（如 `重要 zhong yao` 先于 `重复 chong fu`）；
- 词频值为人工设定的相对量（非真实语料统计频次），跨词条比较只在
  「同一输入拼音的候选竞争」中有意义；绝对值无语义。

## 构建方式

```sh
cargo run --manifest-path tools/lexicon/Cargo.toml -- build \
    --seed shared/schemas/lexicon/openkey-base.seed.tsv \
    --out shared/schemas --version 0.1.0
```

产物（`shared/schemas/`，随包分发、由词库中心组件做版本+哈希管理）：

- `openkey_base.dict.yaml`：rime 词典源（`词条\t拼音\t词频`），
  与各方案 `translator.dictionary: openkey_base` 对应，部署期由 librime
  编译为 `table.bin / prism.bin`；
- `openkey-base.manifest.json`：词库包清单（版本 / 条目数 / SHA-256 / 生成时间）。

构建器会校验：三列格式、词频 1..=100000、音节合法、`词条+拼音` 无重复。

> 已知问题：`tools/lexicon/src/syllables.rs` 的有效音节表缺失 `ma`、`bi`
> 两个常用音节（含「马/吗/码/比/笔」等高频字词），重新生成前需先补全音节表，
> 否则构建器会对种子词库中相关条目报 `BadSyllable`。

## 质量目标

- **首屏命中率 ≥ 90%**：以 `tools/bench-corpus/corpus.tsv` 黄金语料评测，
  指标定义与运行方式见该目录 README；跨版本回归不得劣化 >1%（docs/02 7.2）。
- 覆盖要求：语料全部用字及其语义读音必须存在于本词库（当前 100% 覆盖），
  常用多音字的各常见读音均有条目。
- 一期演进目标（docs/02 5.6）：规范汉字全表 + 6–10 万高频词条 + 地名/常用英文；
  当前种子库为基础子集（约 3.8k 条），按语料评测结果持续扩充。

## 当前规模

| 项 | 数量 |
|---|---|
| 总条目 | 3807 |
| 单字（唯一字） | 1454 |
| 单字行数（含多音字多读音） | 1485 |
| 多音字 | 30 |
| 词语 | 2322 |

维护约定：新增/修改词条后必须重跑上表统计与 `tools/bench` 评测；
扩充只追加或调整词频，不改变格式约定。
