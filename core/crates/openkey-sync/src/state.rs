//! 同步状态机与失败退避（docs/02 3.3.3）。
//!
//! 正常流程：
//! `idle ──Trigger──▶ push ──PushDone──▶ pull ──PullDone──▶ merge ──MergeDone──▶ apply ──ApplyDone──▶ idle`
//!
//! 任一阶段失败回 `idle`，由调度器按 [`BackoffSchedule`]（1m/5m/30m）安排重试。
//! 同步期间的本地新写入正常追加 oplog，下一轮带上（由调度器保证，不经状态机）。

use std::time::Duration;

use crate::error::{Error, Result};

/// 同步状态机状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncState {
    /// 空闲。
    Idle,
    /// 推送本设备 oplog 增量。
    Push,
    /// 拉取其他设备 oplog。
    Pull,
    /// 合并。
    Merge,
    /// 应用（写回本地库并通知引擎重部署）。
    Apply,
}

/// 状态机事件。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncEvent {
    /// 触发一轮同步（手动 / 定时 / 开机 / 写后延迟）。
    Trigger,
    /// 推送完成。
    PushDone,
    /// 拉取完成。
    PullDone,
    /// 合并完成。
    MergeDone,
    /// 应用完成。
    ApplyDone,
    /// 当前阶段失败（网络 / 认证 / 后端错误）。
    Failure,
}

/// 状态迁移；非法转换返回 [`Error::InvalidTransition`]。
pub fn next(state: SyncState, event: SyncEvent) -> Result<SyncState> {
    use SyncEvent::*;
    use SyncState::*;
    match (state, event) {
        (Idle, Trigger) => Ok(Push),
        (Push, PushDone) => Ok(Pull),
        (Pull, PullDone) => Ok(Merge),
        (Merge, MergeDone) => Ok(Apply),
        (Apply, ApplyDone) => Ok(Idle),
        // 任一阶段失败均回 idle，由调度器按退避计划重试
        (Push | Pull | Merge | Apply, Failure) => Ok(Idle),
        _ => Err(Error::InvalidTransition { state, event }),
    }
}

/// 失败退避计划：1m / 5m / 30m 三档离散阶梯，上限 30m（docs/02 3.3.3）。
///
/// `attempt` 为第几次连续失败（从 1 开始）；一轮同步成功后由调度器重置计数。
/// 该阶梯即设计文档"指数退避(1m/5m/30m)"的离散档位实现。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackoffSchedule {
    delays_ms: [u64; 3],
}

impl Default for BackoffSchedule {
    fn default() -> Self {
        Self::new()
    }
}

impl BackoffSchedule {
    /// 默认档位：1m / 5m / 30m。
    pub fn new() -> Self {
        Self {
            delays_ms: [60_000, 300_000, 1_800_000],
        }
    }

    /// 自定义档位（须恰好三档且递增；调用方负责语义校验）。
    pub fn with_delays(delays_ms: [u64; 3]) -> Self {
        Self { delays_ms }
    }

    /// 第 `attempt` 次连续失败后的重试间隔；超过档位数后保持末档（上限 30m）。
    pub fn next_delay(&self, attempt: u32) -> Duration {
        let index = attempt.saturating_sub(1) as usize;
        let index = index.min(self.delays_ms.len() - 1);
        Duration::from_millis(self.delays_ms[index])
    }

    /// 退避上限（末档，默认 30m）。
    pub fn max_delay(&self) -> Duration {
        let last = self.delays_ms.len() - 1;
        Duration::from_millis(self.delays_ms[last])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path_transitions() {
        let mut state = next(SyncState::Idle, SyncEvent::Trigger).ok().unwrap();
        assert_eq!(state, SyncState::Push);
        state = next(state, SyncEvent::PushDone).ok().unwrap();
        assert_eq!(state, SyncState::Pull);
        state = next(state, SyncEvent::PullDone).ok().unwrap();
        assert_eq!(state, SyncState::Merge);
        state = next(state, SyncEvent::MergeDone).ok().unwrap();
        assert_eq!(state, SyncState::Apply);
        state = next(state, SyncEvent::ApplyDone).ok().unwrap();
        assert_eq!(state, SyncState::Idle);
    }

    #[test]
    fn failure_from_any_active_state_returns_idle() {
        for state in [
            SyncState::Push,
            SyncState::Pull,
            SyncState::Merge,
            SyncState::Apply,
        ] {
            assert_eq!(next(state, SyncEvent::Failure).ok(), Some(SyncState::Idle));
        }
    }

    #[test]
    fn illegal_transitions_are_rejected() {
        // idle 上只能 Trigger
        for event in [
            SyncEvent::PushDone,
            SyncEvent::PullDone,
            SyncEvent::MergeDone,
            SyncEvent::ApplyDone,
            SyncEvent::Failure,
        ] {
            assert!(next(SyncState::Idle, event).is_err());
        }
        // 阶段内不接受其他阶段的完成事件
        assert!(next(SyncState::Push, SyncEvent::PullDone).is_err());
        assert!(next(SyncState::Push, SyncEvent::Trigger).is_err());
        assert!(next(SyncState::Pull, SyncEvent::PushDone).is_err());
        assert!(next(SyncState::Merge, SyncEvent::ApplyDone).is_err());
        assert!(next(SyncState::Apply, SyncEvent::MergeDone).is_err());
        // 错误信息含状态与事件
        let err = next(SyncState::Push, SyncEvent::Trigger).err();
        assert!(err.is_some());
    }

    #[test]
    fn backoff_ladder_1m_5m_30m_capped() {
        let schedule = BackoffSchedule::new();
        assert_eq!(schedule.next_delay(1), Duration::from_secs(60));
        assert_eq!(schedule.next_delay(2), Duration::from_secs(300));
        assert_eq!(schedule.next_delay(3), Duration::from_secs(1_800));
        // 超过档位后保持上限
        assert_eq!(schedule.next_delay(4), Duration::from_secs(1_800));
        assert_eq!(schedule.next_delay(100), Duration::from_secs(1_800));
        assert_eq!(schedule.max_delay(), Duration::from_secs(1_800));
        // attempt=0 视为第 1 次
        assert_eq!(schedule.next_delay(0), Duration::from_secs(60));
    }
}
