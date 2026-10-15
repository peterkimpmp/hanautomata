//! Exercise the actual TextService callbacks with an owned queue and synthetic COM document.
use super::*;
use hanautomata_core::{LearningChoice, learning::{client::{Worker, QueueError, Progress, MAX_QUEUED_CHOICES}, Error as StoreError, Operation, Reply, Request, Version}};

#[cfg(feature = "quality-probes")]
#[path = "service_learning_process_tests.rs"]
mod process_tests;

fn newer(generation: u64) -> Version {
    Version { store_id: "11111111111111111111111111111111".into(), generation, revision: generation + 8 }
}
fn ack(worker: &mut Worker) -> Request {
    let dispatch = worker.next().unwrap().unwrap(); let request = dispatch.request().clone();
    let revision = request.lease_revision + request.sequence;
    let reply = Reply { version: Version { store_id: request.store_id.clone(), generation: request.generation, revision },
        committed_revision: revision, sequence: request.sequence, changed: true, duplicate: false };
    assert_eq!(worker.finish(&dispatch, Ok(reply)).unwrap(), Progress::Committed); request
}
fn type_choice(service: &ComObject<TextService>, context: &ITfContext, keys: &[u8]) {
    for key in keys { assert!(paired_key(service, context, *key, edit::FieldAccess::Ordinary).as_bool()); }
    assert!(paired_key(service, context, crate::shell::vk::F2 as u8, edit::FieldAccess::Ordinary).as_bool());
}
fn commit(service: &ComObject<TextService>, context: &ITfContext) {
    assert!(paired_key(service, context, b' ', edit::FieldAccess::Ordinary).as_bool());
}

#[test]
fn f2_round_trips_queue_one_confirmed_choice_per_document_replacement() {
    let (service, host, context, mut worker) = queued_choice_fixture();
    commit(&service, &context); assert!(matches!(ack(&mut worker).operation, Operation::Learn { korean: false, .. }));
    for (expected_text, korean) in [("안녕 ", true), ("dkssud ", false)] {
        assert!(paired_key(&service, &context, crate::shell::vk::F2 as u8, edit::FieldAccess::Ordinary).as_bool());
        assert_eq!(&*host.text.borrow(), expected_text);
        let Operation::Learn { korean: actual, .. } = ack(&mut worker).operation else { panic!("not a learning request"); };
        assert_eq!(actual, korean); assert!(worker.next().unwrap().is_none());
    }
    assert_eq!(service.state.borrow().learning.report().queued, 3);
}

#[test]
fn failed_document_operations_never_reach_the_storage_queue() {
    for failure in 0..4 {
        let (service, host, context, mut worker) = queued_choice_fixture();
        match failure { 0 => host.fail_set.set(true), 1 => host.fail_caret.set(true), 2 => host.fail_end.set(true), _ => host.deny.set(true) }
        let _ = paired_key(&service, &context, b' ', edit::FieldAccess::Ordinary);
        assert!(worker.next().unwrap().is_none(), "failure mode {failure}");
        assert_eq!(service.state.borrow().learning.report().queued, 0);
    }
}

#[test]
fn a_word_keeps_its_pre_reset_generation_but_the_next_word_starts_fresh() {
    let (service, _, context, mut worker) = queued_choice_fixture();
    worker.observe(newer(2)).unwrap(); commit(&service, &context);
    let old = worker.next().unwrap().unwrap(); assert_eq!(old.request().generation, 1);
    assert_eq!(worker.finish(&old, Err(StoreError::StaleGeneration)).unwrap(), Progress::DroppedStale);
    worker.observe(newer(2)).unwrap();
    type_choice(&service, &context, b"HELLO"); commit(&service, &context);
    let new = worker.next().unwrap().unwrap(); assert_eq!(new.request().generation, 2);
    assert_eq!(new.request().sequence, 1, "rejected old choice must not consume a sequence");
}

#[test]
fn f2_captures_generation_before_the_host_write_callback() {
    let (service, host, context, mut worker) = queued_choice_fixture(); commit(&service, &context); ack(&mut worker);
    let worker = Rc::new(RefCell::new(worker)); let during_write = worker.clone();
    *host.before_set.borrow_mut() = Some(Box::new(move || during_write.borrow_mut().observe(newer(2)).unwrap()));
    assert!(paired_key(&service, &context, crate::shell::vk::F2 as u8, edit::FieldAccess::Ordinary).as_bool());
    assert_eq!(worker.borrow_mut().next().unwrap().unwrap().request().generation, 1,
        "the generation cannot be captured after document mutation");
}

#[test]
fn deferred_finish_keeps_its_word_stamp_and_duplicate_callbacks_cannot_queue_twice() {
    let (service, host, _, mut worker) = queued_choice_fixture(); host.defer.set(true);
    unsafe { service.to_interface::<ITfKeyEventSink>().OnSetFocus(false).unwrap(); }
    assert!(worker.next().unwrap().is_none()); worker.observe(newer(3)).unwrap();
    let pending = std::mem::take(&mut *host.pending.borrow_mut()); assert_eq!(pending.len(), 1);
    unsafe { pending[0].DoEditSession(1).unwrap(); pending[0].DoEditSession(1).unwrap(); }
    assert_eq!(worker.next().unwrap().unwrap().request().generation, 1);
    assert_eq!(service.state.borrow().learning.report().queued, 1);
}

#[test]
fn private_pause_reset_and_host_changes_discard_pending_choices_before_handoff() {
    for change in 0..4 {
        let (service, host, _, mut worker) = queued_choice_fixture(); host.defer.set(true);
        unsafe { service.to_interface::<ITfKeyEventSink>().OnSetFocus(false).unwrap(); }
        let engine = service.state.borrow().shell.as_ref().unwrap().engine().clone();
        match change {
            0 => { engine.set_learning_paused(true); engine.set_learning_paused(false); },
            1 => engine.clear_learning(), 2 => host.text.borrow_mut().push_str(" host edit"),
            _ => { service.apply_context_policy(edit::FieldAccess::Private, false, None).unwrap(); },
        }
        for pending in std::mem::take(&mut *host.pending.borrow_mut()) { unsafe { pending.DoEditSession(1).unwrap(); } }
        assert!(worker.next().unwrap().is_none(), "change mode {change}");
        assert_eq!(service.state.borrow().learning.report().queued, 0);
    }
}

#[test]
fn host_termination_and_deactivation_confirm_only_successful_owned_writes() {
    for termination in [false, true] { for refused in [false, true] {
        let (service, host, _, mut worker) = queued_choice_fixture();
        if termination {
            let composition = service.state.borrow().composition.clone().unwrap(); host.fail_set.set(refused);
            let sink = service.to_interface::<ITfCompositionSink>();
            let _ = unsafe { sink.OnCompositionTerminated(1, &composition) };
            unsafe { sink.OnCompositionTerminated(1, &composition).unwrap(); }
        } else {
            host.deny.set(refused);
            unsafe { service.to_interface::<ITfTextInputProcessorEx>().Deactivate().unwrap(); }
        }
        assert_eq!(worker.next().unwrap().is_some(), !refused, "termination={termination} refused={refused}");
        if !refused { ack(&mut worker); assert!(worker.next().unwrap().is_none()); }
    } }
}

#[test]
fn retired_async_callbacks_and_replaced_connections_cannot_rehome_old_learning() {
    for deactivate in [true, false] {
        let (service, host, _, mut old_worker) = queued_choice_fixture(); host.defer.set(true);
        unsafe { service.to_interface::<ITfKeyEventSink>().OnSetFocus(false).unwrap(); }
        let pending = std::mem::take(&mut *host.pending.borrow_mut());
        let (new_producer, mut new_worker) = queue_pair();
        if deactivate { host.deny.set(true); unsafe { service.to_interface::<ITfTextInputProcessorEx>().Deactivate().unwrap(); } }
        else { service.state.borrow_mut().learning.connect(new_producer); }
        host.deny.set(false);
        for pending in pending { unsafe { pending.DoEditSession(1).unwrap(); } }
        assert!(old_worker.next().unwrap().is_none()); assert!(new_worker.next().unwrap().is_none());
        if !deactivate { assert_eq!(service.state.borrow().learning.report().last_failure, Some(learning::Failure::Queue(QueueError::Paused))); }
    }
}

#[test]
fn full_or_closed_queue_preserves_text_and_reports_the_unsaved_choice() {
    for closed in [false, true] {
        let (service, host, context, producer, worker) = connected_fixture();
        if closed { drop(worker); } else {
            for _ in 0..MAX_QUEUED_CHOICES {
                producer.submit(producer.begin().unwrap(), &[LearningChoice { raw: "filler".into(), keys: "filler".into(), korean: false, context: String::new() }]).unwrap();
            }
            // Keep the worker alive through the document edit.
            type_choice(&service, &context, b"DKSSUD"); commit(&service, &context);
            assert_eq!(&*host.text.borrow(), "dkssud ");
            let report = service.state.borrow().learning.report(); assert_eq!(report.rejected, 1);
            assert_eq!(report.last_failure, Some(learning::Failure::Queue(QueueError::Full)));
            drop(worker); continue;
        }
        type_choice(&service, &context, b"DKSSUD"); commit(&service, &context);
        assert_eq!(&*host.text.borrow(), "dkssud ");
        let report = service.state.borrow().learning.report(); assert_eq!(report.rejected, 1);
        assert_eq!(report.last_failure, Some(learning::Failure::Queue(QueueError::Closed)));
    }
}

#[test]
fn attaching_after_a_word_started_does_not_relabel_it_with_the_new_lease() {
    let (service, host, context) = fixture(); host.caret_selection.set(true);
    service.state.borrow_mut().sink = Some(TestCompositionSink.into());
    for key in b"DKS" { assert!(paired_key(&service, &context, *key, edit::FieldAccess::Ordinary).as_bool()); }
    let (producer, mut worker) = queue_pair(); service.state.borrow_mut().learning.connect(producer);
    type_choice(&service, &context, b"SUD"); commit(&service, &context);
    assert_eq!(&*host.text.borrow(), "dkssud "); assert!(worker.next().unwrap().is_none());
    assert_eq!(service.state.borrow().learning.report().last_failure, Some(learning::Failure::Disconnected));
    type_choice(&service, &context, b"HELLO"); commit(&service, &context);
    assert!(worker.next().unwrap().is_some());
    let report = service.state.borrow().learning.report(); assert_eq!(report.queued, 1); assert_eq!(report.rejected, 1);
    assert_eq!(report.last_failure, Some(learning::Failure::Disconnected), "later success must not hide a lost choice");
}
