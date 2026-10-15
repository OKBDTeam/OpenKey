//! HLC（Hybrid Logical Clock，混合逻辑时钟）实现（docs/02 3.3.1）。
//!
//! HLC 将物理时钟（Unix epoch 毫秒）与逻辑计数器结合，字符串形式为
//! `"物理毫秒.计数器.deviceId"`。其关键性质：
//!
//! 1. **本地单调**：同一设备上新产生的 HLC 严格大于此前所有 HLC；
//! 2. **可合并**：接收远端 HLC 时执行标准合并算法（[`Hlc::merge`]），
//!    合并结果严格大于双方输入，从而在不信任任一设备物理时钟的前提下
//!    仍能构造跨设备全序；
//! 3. **确定性全序**：跨设备比较时 `device_id` 仅作最终 tie-break
//!    （详见 [`Hlc`] 的类型文档——这是收敛所需的确定性规则，不是因果序）。

use std::fmt;
use std::sync::Mutex;

use log::warn;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::{Error, Result};

/// 本地时钟回拨告警阈值（毫秒）。回拨超过该值时以计数器补偿并记录警告日志。
pub const CLOCK_DRIFT_WARN_MS: i64 = 5_000;

/// HLC 时间戳。
///
/// # 全序语义
///
/// [`Ord`] 按 `(physical_ms, counter, device_id)` 字典序比较，构成一个
/// **确定性全序**：任意两个 HLC 均可比较，且所有设备对同一对值做出一致
/// 裁决——这是条目级 LWW 合并得以收敛的前提（docs/02 3.3.3）。
///
/// 必须强调：该全序**不是因果序**。`device_id` tie-break 仅作用于
/// `(physical, counter)` 完全相同的跨设备事件，此时两事件并无因果关系，
/// 按设备 ID 字典序决胜只是为保证收敛而约定的确定性规则，获胜方不意味着
/// "发生得更晚"。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Hlc {
    /// 物理毫秒（Unix epoch 毫秒）。
    pub physical_ms: i64,
    /// 逻辑计数器。
    pub counter: u64,
    /// 设备标识。
    pub device_id: String,
}

impl Hlc {
    /// 构造 HLC。
    pub fn new(physical_ms: i64, counter: u64, device_id: impl Into<String>) -> Self {
        Self {
            physical_ms,
            counter,
            device_id: device_id.into(),
        }
    }

    /// 从字符串解析（`"物理毫秒.计数器.deviceId"`）。
    ///
    /// `device_id` 本身可含 `.`（取第一、二个分隔点之后的全部内容），
    /// 但不可为空。
    pub fn parse(s: &str) -> Result<Self> {
        let parts: Vec<&str> = s.splitn(3, '.').collect();
        let [physical, counter, device_id] = parts.as_slice() else {
            return Err(Error::Hlc(format!(
                "期望 \"物理毫秒.计数器.deviceId\"，得到 {s:?}"
            )));
        };
        let physical_ms = physical
            .parse::<i64>()
            .map_err(|e| Error::Hlc(format!("物理毫秒解析失败: {e}")))?;
        let counter = counter
            .parse::<u64>()
            .map_err(|e| Error::Hlc(format!("计数器解析失败: {e}")))?;
        if device_id.is_empty() {
            return Err(Error::Hlc("设备 ID 为空".to_string()));
        }
        Ok(Self {
            physical_ms,
            counter,
            device_id: (*device_id).to_string(),
        })
    }

    /// 标准 HLC 合并算法（HLC 论文）：物理分量取大，计数器按标准规则推进，
    /// 设备标识保持本地不变。
    ///
    /// 合并结果**严格大于** `self` 与 `remote` 双方（计数器至少 +1），
    /// 因此可安全作为本地时钟的新基线。
    pub fn merge(&self, remote: &Self) -> Self {
        let physical = self.physical_ms.max(remote.physical_ms);
        let counter = if physical == self.physical_ms && physical == remote.physical_ms {
            // 双方物理分量相同且均为最大值：计数器取大后 +1
            self.counter.max(remote.counter).saturating_add(1)
        } else if physical == self.physical_ms {
            // 本地物理分量更大：本地计数器 +1
            self.counter.saturating_add(1)
        } else {
            // 远端物理分量更大：远端计数器 +1
            remote.counter.saturating_add(1)
        };
        Self {
            physical_ms: physical,
            counter,
            device_id: self.device_id.clone(),
        }
    }
}

impl fmt::Display for Hlc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}.{}.{}",
            self.physical_ms, self.counter, self.device_id
        )
    }
}

impl Serialize for Hlc {
    /// 序列化为线格式字符串（oplog / 快照 / SQLite 中均为该形式）。
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Hlc {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Hlc::parse(&raw).map_err(serde::de::Error::custom)
    }
}

/// 本地 HLC 时钟。
///
/// 同一设备上连续取得的 HLC 严格递增；检测到本地时钟回拨时物理分量不回退、
/// 以计数器补偿（回拨超过 [`CLOCK_DRIFT_WARN_MS`] 时记录警告）。
#[derive(Debug)]
pub struct HlcClock {
    device_id: String,
    last: Mutex<Hlc>,
}

impl HlcClock {
    /// 以设备标识创建时钟（初始值：当前物理时间、计数器 0）。
    pub fn new(device_id: impl Into<String>) -> Self {
        Self::new_at(device_id, now_ms())
    }

    /// 以设备标识与指定起始物理时间创建时钟（测试与确定性场景使用）。
    pub fn new_at(device_id: impl Into<String>, physical_ms: i64) -> Self {
        let device_id = device_id.into();
        let initial = Hlc::new(physical_ms, 0, device_id.clone());
        Self {
            device_id,
            last: Mutex::new(initial),
        }
    }

    /// 本设备标识。
    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    /// 取一个新的单调递增 HLC（读取系统时钟）。
    pub fn now(&self) -> Hlc {
        self.tick_at(now_ms())
    }

    /// 以指定物理时间推进时钟（纯逻辑；测试与确定性场景使用）。
    pub(crate) fn tick_at(&self, physical_ms: i64) -> Hlc {
        let mut last = self
            .last
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (new_physical, new_counter) = if physical_ms > last.physical_ms {
            (physical_ms, 0)
        } else if physical_ms == last.physical_ms {
            (last.physical_ms, last.counter.saturating_add(1))
        } else {
            // 本地时钟回拨：物理分量保持不回退，仅推进计数器，保证单调性
            let drift = last.physical_ms.saturating_sub(physical_ms);
            if drift > CLOCK_DRIFT_WARN_MS {
                warn!(
                    "本地时钟回拨 {drift} ms（超过 {CLOCK_DRIFT_WARN_MS} ms 阈值），HLC 以计数器补偿保持单调递增"
                );
            }
            (last.physical_ms, last.counter.saturating_add(1))
        };
        let hlc = Hlc::new(new_physical, new_counter, self.device_id.clone());
        *last = hlc.clone();
        hlc
    }

    /// 观察远端 HLC 并推进本地时钟（标准 HLC 合并，docs/02 3.3.1）。
    ///
    /// 返回合并结果（严格大于此前已知的所有 HLC）。
    pub fn observe(&self, remote: &Hlc) -> Hlc {
        let mut last = self
            .last
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let merged = last.merge(remote);
        // merge 不变式保证 merged > last；计数器饱和（u64::MAX）时保守跳过
        if merged > *last {
            *last = merged.clone();
        }
        merged
    }
}

impl openkey_data::HlcClock for HlcClock {
    /// 取一个新的 HLC 并返回线格式字符串（data 层只消费字符串形式）。
    fn tick(&self) -> String {
        self.now().to_string()
    }
}

/// 当前墙钟毫秒（Unix epoch）。
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_parse_roundtrip() {
        let hlc = Hlc::new(1_760_000_000_123, 4, "win-abc");
        let s = hlc.to_string();
        assert_eq!(s, "1760000000123.4.win-abc");
        assert_eq!(Hlc::parse(&s).ok(), Some(hlc));
    }

    #[test]
    fn parse_accepts_dotted_device_id() {
        let hlc = Hlc::parse("100.2.dev.with.dots").ok();
        assert_eq!(
            hlc.as_ref().map(|h| h.device_id.as_str()),
            Some("dev.with.dots")
        );
    }

    #[test]
    fn parse_rejects_malformed() {
        assert!(Hlc::parse("").is_err());
        assert!(Hlc::parse("100").is_err());
        assert!(Hlc::parse("100.2").is_err());
        assert!(Hlc::parse("100.2.").is_err()); // 设备 ID 为空
        assert!(Hlc::parse("abc.2.dev").is_err());
        assert!(Hlc::parse("100.x.dev").is_err());
        assert!(Hlc::parse("100.-1.dev").is_err());
    }

    #[test]
    fn ord_is_total_order_by_physical_then_counter_then_device() {
        let a = Hlc::new(100, 5, "dev-a");
        let b = Hlc::new(100, 5, "dev-b");
        let c = Hlc::new(100, 6, "dev-a");
        let d = Hlc::new(101, 0, "dev-a");
        assert!(a < b); // physical/counter 相同 → device_id 字典序决胜
        assert!(b < c); // counter 大者大
        assert!(c < d); // physical 大者大
                        // 字典序陷阱防护：字符串比较 "9..." > "10..."，HLC 数值比较必须正确
        let small = Hlc::new(9, 0, "dev-a");
        let big = Hlc::new(10, 0, "dev-a");
        assert!(small < big);
        assert!(small.to_string() > big.to_string()); // 证明字符串比较确实不可靠
    }

    #[test]
    fn clock_is_monotonic_across_same_millisecond() {
        let clock = HlcClock::new_at("dev-a", 0);
        let first = clock.tick_at(1_000);
        let second = clock.tick_at(1_000);
        let third = clock.tick_at(1_000);
        assert_eq!(first.counter, 0);
        assert_eq!(second.counter, 1);
        assert_eq!(third.counter, 2);
        assert!(first < second && second < third);
        assert_eq!(third.device_id, "dev-a");
    }

    #[test]
    fn clock_advances_physical_and_resets_counter() {
        let clock = HlcClock::new_at("dev-a", 0);
        let first = clock.tick_at(1_000);
        let second = clock.tick_at(2_000);
        assert_eq!((first.physical_ms, first.counter), (1_000, 0));
        assert_eq!((second.physical_ms, second.counter), (2_000, 0));
    }

    #[test]
    fn clock_compensates_on_rollback() {
        let clock = HlcClock::new_at("dev-a", 0);
        let first = clock.tick_at(10_000);
        assert_eq!((first.physical_ms, first.counter), (10_000, 0));
        // 小幅回拨（≤5s）：物理分量保持，计数器推进
        let second = clock.tick_at(9_500);
        assert_eq!((second.physical_ms, second.counter), (10_000, 1));
        // 大幅回拨（>5s）：同样只推进计数器（并记警告日志）
        let third = clock.tick_at(1_000);
        assert_eq!((third.physical_ms, third.counter), (10_000, 2));
        assert!(first < second && second < third);
    }

    #[test]
    fn merge_standard_cases() {
        let local = Hlc::new(100, 5, "dev-a");
        let remote = Hlc::new(90, 9, "dev-b");
        let merged = local.merge(&remote);
        assert_eq!((merged.physical_ms, merged.counter), (100, 6));
        assert_eq!(merged.device_id, "dev-a");
        assert!(merged > local && merged > remote);

        let local = Hlc::new(90, 5, "dev-a");
        let remote = Hlc::new(100, 9, "dev-b");
        let merged = local.merge(&remote);
        assert_eq!((merged.physical_ms, merged.counter), (100, 10));
        assert_eq!(merged.device_id, "dev-a");
        assert!(merged > local && merged > remote);

        // 双方物理分量相同：计数器取大后 +1
        let local = Hlc::new(100, 5, "dev-a");
        let remote = Hlc::new(100, 9, "dev-b");
        let merged = local.merge(&remote);
        assert_eq!((merged.physical_ms, merged.counter), (100, 10));
        assert!(merged > local && merged > remote);

        // 完全相同的时间戳也要严格推进
        let local = Hlc::new(100, 5, "dev-a");
        let merged = local.merge(&local);
        assert!(merged > local);
    }

    #[test]
    fn observe_advances_local_clock() {
        let clock = HlcClock::new_at("dev-a", 0);
        let first = clock.tick_at(1_000);
        let remote = Hlc::new(5_000, 7, "dev-b");
        let merged = clock.observe(&remote);
        assert_eq!((merged.physical_ms, merged.counter), (5_000, 8));
        assert!(merged > first);
        // 后续 tick 以合并结果为基线，不回退
        let next = clock.tick_at(2_000);
        assert_eq!((next.physical_ms, next.counter), (5_000, 9));
        // 更旧的远端不会让时钟回退
        let older = clock.observe(&Hlc::new(1_000, 0, "dev-b"));
        assert!(older > next);
    }

    #[test]
    fn implements_data_layer_hlc_clock_trait() {
        use openkey_data::HlcClock as _;
        let clock = HlcClock::new("dev-a");
        let ticked = clock.tick();
        assert_eq!(
            Hlc::parse(&ticked).ok().map(|h| h.device_id),
            Some("dev-a".to_string())
        );
    }

    #[test]
    fn serde_uses_wire_string_form() {
        let hlc = Hlc::new(1_760_000_000_123, 4, "win-abc");
        let json = serde_json::to_string(&hlc).ok().unwrap_or_default();
        assert_eq!(json, "\"1760000000123.4.win-abc\"");
        let back: Hlc = serde_json::from_str(&json).ok().unwrap();
        assert_eq!(back, hlc);
    }
}
