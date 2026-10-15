use hanautomata_core::{context_key, LearningBatch, WordPreferences};
use std::sync::Arc;

fn propose(p: &WordPreferences) -> LearningBatch {
    assert!(p.remember("dkssud", "dkssud", true, &context_key("hello")));
    p.take_learning().unwrap()
}

#[test]
fn snapshot_recovery_replays_only_bounded_local_deltas_without_duplicating_on_refresh() {
    use hanautomata_core::{PreparedLearning, SnapshotInstall, LearningChoice};
    let p = WordPreferences::new(); p.defer_learning(true);
    let stored = WordPreferences::new(); stored.remember_global("hello", "hello", true);
    let local = LearningChoice { raw: "local".into(), keys: "local".into(), korean: false, context: context_key("hello") };
    for _ in 0..3 {
        let mut prepared = PreparedLearning::new(&stored.export().0);
        assert_eq!(p.try_install_snapshot_with_local(p.version(), &mut prepared, &[local.clone()]), SnapshotInstall::Applied);
        assert_eq!(p.count(), 3); assert!(p.take_learning().is_none());
        assert!(p.export().0.iter().all(|w| w.korean_weight + w.english_weight == 1.0));
    }
    let before = p.export().0; let mut prepared = PreparedLearning::new(&[]);
    assert_eq!(p.try_install_snapshot_with_local(p.version(), &mut prepared, &vec![local.clone(); 65]), SnapshotInstall::Changed);
    let mut invalid = local; invalid.context = "unhashed surrounding document".into();
    assert_eq!(p.try_install_snapshot_with_local(p.version(), &mut prepared, &[invalid]), SnapshotInstall::Changed);
    assert_eq!(p.export().0, before);
}

#[test]
#[ignore = "manual bounded recovery timing; not an application input-latency approval"]
fn measure_full_dictionary_local_recovery() {
    use hanautomata_core::{PreparedLearning, SnapshotInstall, LearningChoice, CAPACITY};
    let key = |prefix: &str, n: usize| format!("{prefix}{}{}{}", (b'a' + ((n / 676) % 26) as u8) as char,
        (b'a' + ((n / 26) % 26) as u8) as char, (b'a' + (n % 26) as u8) as char);
    let stored = WordPreferences::new();
    for n in 0..CAPACITY { let word = key("stored", n); stored.remember_global(&word, &word, true); }
    let entries = stored.export().0;
    let local: Vec<_> = (0..64).map(|n| { let word = key("local", n);
        LearningChoice { raw: word.clone(), keys: word, korean: false, context: context_key("context") } }).collect();
    let preferences = WordPreferences::new(); let mut samples = Vec::new();
    for _ in 0..128 {
        let mut prepared = PreparedLearning::new(&entries); let version = preferences.version();
        let start = std::time::Instant::now();
        assert_eq!(preferences.try_install_snapshot_with_local(version, &mut prepared, &local), SnapshotInstall::Applied);
        samples.push(start.elapsed().as_micros()); assert_eq!(preferences.count(), CAPACITY);
    }
    samples.sort();
    println!("local_recovery samples=128 stored=1024 deltas=64 contextual=true p95_us={} max_us={}", samples[121], samples[127]);
    assert!(samples[121] < 20_000, "bounded local recovery alone must fit the 20ms input budget");
}

#[test]
fn document_confirmation_is_required_and_clones_cannot_train_twice() {
    let p = WordPreferences::new(); p.defer_learning(true);
    let batch = propose(&p);
    assert_eq!(p.count(), 0);
    assert!(!p.can_undo());
    let clone = batch.clone();
    let deltas = p.confirm_learning(&batch);
    assert_eq!(deltas.len(), 1);
    assert_eq!(deltas[0].raw, "dkssud");
    assert_eq!(p.count(), 2, "global and contextual evidence belong to one choice");
    let first = p.export().0;
    assert!(p.confirm_learning(&clone).is_empty());
    assert_eq!(p.export().0, first);
    assert!(p.undo_last());
    assert_eq!(p.count(), 0, "one undo reverses both entries");
}

#[test]
fn reset_delete_undo_restore_and_privacy_changes_invalidate_detached_choices() {
    for change in 0..7 {
        let p = WordPreferences::new(); p.defer_learning(true);
        let batch = propose(&p);
        match change {
            0 => p.clear(),
            1 => { p.forget("dkssud", "dkssud"); },
            2 => { p.undo_last(); },
            3 => p.restore(&[]),
            4 => { p.set_recording(false); p.set_recording(true); },
            5 => { p.set_enabled(false); p.set_enabled(true); },
            _ => { p.defer_learning(false); p.defer_learning(true); },
        }
        assert!(p.confirm_learning(&batch).is_empty(), "change {change}");
        assert_eq!(p.count(), 0);
        assert_eq!(p.confirm_learning(&propose(&p)).len(), 1, "new actions remain usable");
    }
}

#[test]
fn a_ticket_cannot_be_confirmed_by_a_different_engine() {
    let first = WordPreferences::new(); first.defer_learning(true);
    let second = WordPreferences::new(); second.defer_learning(true);
    let batch = propose(&first);
    assert!(second.confirm_learning(&batch).is_empty());
    assert_eq!(second.count(), 0);
    assert_eq!(first.confirm_learning(&batch).len(), 1);
}

#[test]
fn invalidated_confirmation_reports_its_count_once_without_foreign_consumption() {
    use hanautomata_core::{LearningConfirmation, PreparedLearning, SnapshotInstall};
    let p = WordPreferences::new(); p.defer_learning(true); let other = WordPreferences::new(); other.defer_learning(true);
    let batch = propose(&p);
    assert_eq!(other.confirm_learning_with_status(&batch), LearningConfirmation::ConsumedOrForeign);
    let mut prepared = PreparedLearning::new(&[]);
    assert_eq!(p.try_install_snapshot(p.version(), &mut prepared), SnapshotInstall::Applied);
    assert_eq!(p.confirm_learning_with_status(&batch), LearningConfirmation::Invalidated(1));
    assert_eq!(p.confirm_learning_with_status(&batch.clone()), LearningConfirmation::ConsumedOrForeign);
    assert_eq!(p.count(), 0);
    assert!(matches!(p.confirm_learning_with_status(&propose(&p)), LearningConfirmation::Confirmed(choices) if choices.len() == 1));
}

#[test]
fn blocked_invalid_and_overflowed_choices_never_enter_a_confirmation_batch() {
    let p = WordPreferences::new(); p.defer_learning(true);
    p.set_recording(false);
    assert!(!p.remember_global("secret", "secret", true));
    p.set_recording(true);
    assert!(!p.remember_global("password123", "password123", true));
    assert!(!p.remember("abc", "abc", true, "unhashed surrounding text"));
    assert!(p.take_learning().is_none());
    for _ in 0..100 { p.remember_global("dkssud", "dkssud", true); }
    assert!(p.take_learning().is_none(), "bounded overflow discards the whole edit, never a prefix");
    assert_eq!(p.count(), 0);
    assert_eq!(p.confirm_learning(&propose(&p)).len(), 1, "next edit can recover");
}

#[test]
fn concurrent_duplicate_completions_apply_once_and_debug_is_redacted() {
    let p = Arc::new(WordPreferences::new()); p.defer_learning(true);
    let batch = propose(&p);
    assert!(!format!("{batch:?}").contains("dkssud"));
    let tasks: Vec<_> = (0..8).map(|_| {
        let (p, batch) = (p.clone(), batch.clone());
        std::thread::spawn(move || p.confirm_learning(&batch))
    }).collect();
    let choices: Vec<_> = tasks.into_iter().flat_map(|t| t.join().unwrap()).collect();
    assert_eq!(choices.len(), 1);
    assert!(!format!("{choices:?}").contains("dkssud"));
    assert_eq!(p.export().0[0].korean_weight, 1.0);
}

#[test]
fn legacy_callers_keep_immediate_learning_and_dropped_tickets_have_no_effect() {
    let p = WordPreferences::new();
    assert!(p.remember_global("hello", "hello", false));
    assert_eq!(p.count(), 1);
    assert!(p.take_learning().is_none());
    p.defer_learning(true);
    drop(propose(&p));
    assert_eq!(p.count(), 1);
    assert!(!p.try_get("dkssud", "dkssud", &context_key("hello")).found);
}
