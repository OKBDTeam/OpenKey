# tools/bench-corpus —— 黄金基准语料（docs/02 7.2）

## 构成

`corpus.tsv` 共 500 句，按行序分四类（行号区间即分类）：

| 分类 | 行号区间 | 句数 | 比例 | 题材 |
|---|---|---|---|---|
| 新闻 | 1–250 | 250 | 5/10 | 科技、经济、民生、体育、文化（不涉时政敏感） |
| 聊天 | 251–400 | 150 | 3/10 | 口语对话体 |
| 技术 | 401–450 | 50 | 1/10 | IT / 互联网技术叙述 |
| 古文 | 451–500 | 50 | 1/10 | 公认名句 |

## 格式

每行 `句子\t拼音`，TAB 分隔，UTF-8：

- 句子为纯汉字（无标点、无数字、无字母）；
- 拼音为该句逐字**无声调**拼音，字与字之间单个空格；
- 多音字按句中语义取常见读音。

## 指标定义（与 tools/bench 实现一致）

- **句命中率**：输入整句拼音（字母流，librime 自动分词）后，句子全部字符可在候选**第一页**按候选顺序贪心拼接覆盖的句数占比；
- **字符首屏命中率**：被覆盖字符数 / 总字符数；
- **回归门禁**：跨版本评测劣化 >1% 需在发布说明中解释（docs/02 7.2）。

## 运行

```sh
# 先构建词库（generated dict 位于 shared/schemas/openkey_base.dict.yaml）
cargo run --manifest-path tools/lexicon/Cargo.toml -- build \
    --seed shared/schemas/lexicon/openkey-base.seed.tsv --out shared/schemas --version 0.1.0

# 评测（librime 需可用；CI 的 core-linux job 自动执行）
cargo run --manifest-path tools/bench/Cargo.toml -- run \
    --corpus tools/bench-corpus/corpus.tsv --shared shared/schemas \
    --report bench-report.md
```

## 多音字处理规则

按语义取最常见读音，例如：银行（yin hang）、重要（zhong yao）、
重庆（chong qing）、音乐（yin yue）、快乐（kuai le）、头发（tou fa）、
干净（gan jing）、子弹（zi dan）、重复（chong fu）、便宜（pian yi）。
边界争议读音以《现代汉语词典》常见口径为准，改动需在提交说明中指出。

## 维护

- 语料为 OpenKey 自制（CC0-1.0，DR-001），不从第三方语料拷贝；
- 新增句子须保持纯汉字 + 逐字无声调拼音；类别只追加到对应区间末尾
  （改动行号区间会改变分类，需同步本表）；
- 500 句规模按 7.2 确定；扩充需同步更新本 README 的构成表。
