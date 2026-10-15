//! Word-free status sampled for the next preview, not an idle monitor or a claim about the composing word.
use super::Report;
use crate::learning_runtime::Phase;

impl Report {
    /// Only fixed phrases and numeric counters. No document, path, IPC or locking is used here.
    pub fn notice(&self, flags: Option<(bool, bool)>) -> String {
        let phase = self.storage.map(|s| s.phase);
        let state = match flags {
            None => "상태 확인 대기",
            Some((false, _)) => "사용 안 함",
            Some((true, true)) => "새 선택 기억 일시 정지",
            Some((true, false)) => match phase {
                None | Some(Phase::Failed) => "저장 불가",
                Some(Phase::Stopped) => "저장 종료",
                Some(Phase::Starting) => "준비 중",
                Some(Phase::Running) if !self.connected || self.snapshots_applied == 0 => "준비 중",
                Some(Phase::Running) => "준비됨",
            },
        };
        let mut parts = vec![format!("이 입력 세션의 선택 기억: {state}")];
        let (committed, discarded) = self.storage.map_or((0, 0), |s| (s.committed, s.discarded));
        // Accepted choices are one request each. Neither retry errors nor locally rejected choices are ACKs.
        let pending = committed.checked_add(discarded).and_then(|done| self.queued.checked_sub(done));
        if pending.is_none() || [self.queued, committed, discarded].contains(&u64::MAX) {
            parts.push("저장 집계 확인 필요".into());
        } else {
            if committed > 0 { parts.push(format!("누적 저장 확인 {committed}건")); }
            if let Some(n @ 1..) = pending {
                let label = if matches!(phase, Some(Phase::Starting | Phase::Running)) { "저장 확인 대기" } else { "저장 미확인" };
                parts.push(format!("{label} {n}건"));
            }
            if discarded > 0 { parts.push(format!("삭제·초기화로 제외 {discarded}건")); }
        }
        if self.recovery_pending > 0 { parts.push(format!("임시 기억 {}건 (미저장)", self.recovery_pending)); }
        if self.recovery_overflow > 0 { parts.push(format!("임시 기억 상한 초과 누적 {}건", self.recovery_overflow)); }
        if self.invalidated > 0 { parts.push(format!("저장 전 무효화 누적 {}건", self.invalidated)); }
        parts.join(" · ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::learning_runtime::Status;

    fn active() -> Report {
        Report { connected: true, snapshots_applied: 1, initial_snapshot_applied: Some(true),
            storage: Some(Status { phase: Phase::Running, committed: 0, discarded: 0, failures: 0, idle_waits: 0 }),
            ..Report::default() }
    }

    #[test]
    fn learning_notice_queue_acceptance_never_claims_storage_ack() {
        let mut r = active(); r.queued = 2;
        let text = r.notice(Some((true, false)));
        assert!(text.contains("저장 확인 대기 2건")); assert!(!text.contains("누적 저장 확인"));
        r.storage.as_mut().unwrap().committed = 1;
        let text = r.notice(Some((true, false)));
        assert!(text.contains("누적 저장 확인 1건")); assert!(text.contains("저장 확인 대기 1건"));
        r.storage.as_mut().unwrap().discarded = 1;
        let text = r.notice(Some((true, false)));
        assert!(text.contains("삭제·초기화로 제외 1건")); assert!(!text.contains("저장 확인 대기"));
        assert!(!text.contains("모두 저장"));
    }

    #[test]
    fn learning_notice_historical_failures_do_not_disable_recovered_storage() {
        let mut r = active(); r.queued = 1; r.storage.as_mut().unwrap().committed = 1;
        r.storage.as_mut().unwrap().failures = 7; r.rejected = 3;
        r.last_failure = Some(super::super::Failure::Disconnected);
        let text = r.notice(Some((true, false)));
        assert!(text.contains("준비됨")); assert!(text.contains("누적 저장 확인 1건"));
        assert!(!text.contains("저장 불가")); assert!(!text.contains("저장 확인 대기"));
        r.recovery_pending = 2; r.recovery_overflow = 1;
        let text = r.notice(Some((true, false)));
        assert!(text.contains("임시 기억 2건 (미저장)")); assert!(text.contains("상한 초과 누적 1건"));
    }

    #[test]
    fn learning_notice_pause_does_not_hide_previously_accepted_writes() {
        let mut r = active(); r.queued = 1;
        let text = r.notice(Some((true, true)));
        assert!(text.contains("새 선택 기억 일시 정지")); assert!(text.contains("저장 확인 대기 1건"));
        assert!(r.notice(Some((false, true))).contains("사용 안 함"));
        assert!(r.notice(None).contains("상태 확인 대기"));
    }

    #[test]
    fn learning_notice_starting_disconnected_failed_and_stopped_are_distinct() {
        let mut r = active(); r.queued = 2;
        for phase in [Phase::Starting, Phase::Running, Phase::Failed, Phase::Stopped] {
            r.storage.as_mut().unwrap().phase = phase;
            let text = r.notice(Some((true, false)));
            match phase {
                Phase::Starting => assert!(text.contains("준비 중")),
                Phase::Running => assert!(text.contains("준비됨")),
                Phase::Failed => { assert!(text.contains("저장 불가")); assert!(text.contains("저장 미확인 2건")); },
                Phase::Stopped => { assert!(text.contains("저장 종료")); assert!(text.contains("저장 미확인 2건")); },
            }
        }
        r.storage.as_mut().unwrap().phase = Phase::Running; r.connected = false;
        assert!(r.notice(Some((true, false))).contains("준비 중"));
        r.connected = true; r.snapshots_applied = 0;
        assert!(r.notice(Some((true, false))).contains("준비 중"));
        assert!(Report::default().notice(Some((true, false))).contains("저장 불가"));
    }

    #[test]
    fn learning_notice_inconsistent_or_exhausted_counts_never_claim_completion() {
        let mut r = active(); r.storage.as_mut().unwrap().committed = 1;
        assert!(r.notice(Some((true, false))).contains("집계 확인 필요"));
        r.queued = u64::MAX; r.storage.as_mut().unwrap().committed = u64::MAX;
        r.storage.as_mut().unwrap().discarded = 1; r.recovery_pending = usize::MAX;
        r.recovery_overflow = u64::MAX; r.invalidated = u64::MAX;
        let text = r.notice(Some((true, false)));
        assert!(text.contains("집계 확인 필요")); assert!(!text.contains("누적 저장 확인"));
        assert!(text.encode_utf16().count() <= 512); assert!(!text.contains('\0'));
    }
}
