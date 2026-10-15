//! 锁定状态机（docs/02 3.4.1 lock 模块）。
//!
//! ```text
//! Unlocked ──SoftLock──▶ SoftLocked ──Resume──▶ Unlocked
//!    │                       │
//!    └────────HardLock───────┴──▶ Locked（密钥已清除，需主密码/生物识别重解）
//! ```
//!
//! - **Unlocked**：密钥在内存，全部 vault 操作可用；
//! - **SoftLocked**：密钥仍在内存（如应用退后台的短时锁定），但 UI 层视为锁定，
//!   所有读写操作被拒绝，经生物识别等快速认证 `Resume` 回 Unlocked；
//! - **Locked**：密钥材料已从内存清零（[`UserKey`] 的 `ZeroizeOnDrop`），
//!   必须重新派生（主密码）才能恢复；状态机上 Locked 无出边——
//!   "重新解锁"由 [`LocalVault::unlock`](crate::local::LocalVault::unlock)
//!   按主密码重解实现，不属于本状态机的迁移。

use crate::error::{Error, Result};

/// 锁定状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockState {
    /// 已解锁：密钥在内存，操作可用。
    Unlocked,
    /// 软锁定：密钥仍在内存，但操作被拒绝（等待快速认证）。
    SoftLocked,
    /// 硬锁定：密钥已清零，需主密码/生物识别重解。
    Locked,
}

impl LockState {
    /// 密钥是否仍在内存（Unlocked / SoftLocked）。
    pub fn has_keys_in_memory(&self) -> bool {
        matches!(self, LockState::Unlocked | LockState::SoftLocked)
    }

    /// 是否处于可操作状态。
    pub fn is_unlocked(&self) -> bool {
        matches!(self, LockState::Unlocked)
    }
}

/// 锁定状态机事件。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockEvent {
    /// 软锁定（密钥保留在内存）。
    SoftLock,
    /// 从软锁定恢复（快速认证通过）。
    Resume,
    /// 硬锁定（清除内存中的密钥）。
    HardLock,
}

/// 锁定状态迁移；非法转换返回 [`Error::InvalidLockTransition`]。
///
/// Locked 为吸收态：无出边，重新解锁走
/// [`LocalVault::unlock`](crate::local::LocalVault::unlock) 而非本状态机。
pub fn lock_next(state: LockState, event: LockEvent) -> Result<LockState> {
    use LockEvent::*;
    use LockState::*;
    match (state, event) {
        (Unlocked, SoftLock) => Ok(SoftLocked),
        (SoftLocked, Resume) => Ok(Unlocked),
        (Unlocked | SoftLocked, HardLock) => Ok(Locked),
        _ => Err(Error::InvalidLockTransition { from: state, event }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path_soft_lock_cycle() {
        assert_eq!(
            lock_next(LockState::Unlocked, LockEvent::SoftLock).ok(),
            Some(LockState::SoftLocked)
        );
        assert_eq!(
            lock_next(LockState::SoftLocked, LockEvent::Resume).ok(),
            Some(LockState::Unlocked)
        );
    }

    #[test]
    fn hard_lock_from_unlocked_and_soft_locked() {
        for state in [LockState::Unlocked, LockState::SoftLocked] {
            assert_eq!(
                lock_next(state, LockEvent::HardLock).ok(),
                Some(LockState::Locked)
            );
        }
    }

    #[test]
    fn locked_is_absorbing() {
        for event in [LockEvent::SoftLock, LockEvent::Resume, LockEvent::HardLock] {
            assert!(lock_next(LockState::Locked, event).is_err());
        }
    }

    #[test]
    fn illegal_transitions_rejected() {
        // Unlocked 上不接受 Resume
        assert!(lock_next(LockState::Unlocked, LockEvent::Resume).is_err());
        // SoftLocked 上不接受再次 SoftLock / HardLock 之外的重复事件
        assert!(lock_next(LockState::SoftLocked, LockEvent::SoftLock).is_err());
    }

    #[test]
    fn state_helpers() {
        assert!(LockState::Unlocked.is_unlocked());
        assert!(!LockState::SoftLocked.is_unlocked());
        assert!(!LockState::Locked.is_unlocked());
        assert!(LockState::Unlocked.has_keys_in_memory());
        assert!(LockState::SoftLocked.has_keys_in_memory());
        assert!(!LockState::Locked.has_keys_in_memory());
    }

    #[test]
    fn error_message_contains_states() {
        let err = lock_next(LockState::Unlocked, LockEvent::Resume).err();
        let msg = err.map(|e| e.to_string()).unwrap_or_default();
        assert!(msg.contains("Unlocked"), "{msg}");
        assert!(msg.contains("Resume"), "{msg}");
    }
}
