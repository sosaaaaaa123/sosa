use atomic_refcell::AtomicRefMut;
use std::cell::Cell;
use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use vst3_sys::vst::IComponentHandler;

use crate::prelude::{
    GuiContext, InitContext, ParamPtr, PluginApi, PluginNoteEvent, PluginState, ProcessContext,
    Transport, Vst3Plugin,
};

use super::inner::{Task, WrapperInner};

/// An [`InitContext`] implementation for the wrapper.
///
/// # Note
///
/// Requests to change the latency are only sent when this object is dropped. Otherwise there's the
/// risk that the host will immediately deactivate/reactivate the plugin while still in the init
/// call. Reentrannt function calls are difficult to handle in Rust without forcing everything to
/// use interior mutability, so this will have to do for now. This does mean that `Plugin` mutex
/// lock has to be dropped before this object.
pub(crate) struct WrapperInitContext<'a, P: Vst3Plugin> {
    pub(super) inner: &'a WrapperInner<P>,
    pub(super) pending_requests: PendingInitContextRequests,
}

/// Any requests that should be sent out when the [`WrapperInitContext`] is dropped. See that
/// struct's docstring for mroe information.
#[derive(Debug, Default)]
pub(crate) struct PendingInitContextRequests {
    /// The value of the last `.set_latency_samples()` call.
    latency_changed: Cell<Option<u32>>,
}

/// A [`ProcessContext`] implementation for the wrapper. This is a separate object so it can hold on
/// to lock guards for event queues. Otherwise reading these events would require constant
/// unnecessary atomic operations to lock the uncontested locks.
pub(crate) struct WrapperProcessContext<'a, P: Vst3Plugin> {
    pub(super) inner: &'a WrapperInner<P>,
    pub(super) input_events_guard: AtomicRefMut<'a, VecDeque<PluginNoteEvent<P>>>,
    pub(super) output_events_guard: AtomicRefMut<'a, VecDeque<PluginNoteEvent<P>>>,
    pub(super) transport: Transport,
}

/// A [`GuiContext`] implementation for the wrapper. This is passed to the plugin in
/// [`Editor::spawn()`][crate::prelude::Editor::spawn()] so it can interact with the rest of the plugin and
/// with the host for things like setting parameters.
pub(crate) struct WrapperGuiContext<P: Vst3Plugin> {
    pub(super) inner: Arc<WrapperInner<P>>,
    #[cfg(debug_assertions)]
    pub(super) param_gesture_checker:
        atomic_refcell::AtomicRefCell<crate::wrapper::util::context_checks::ParamGestureChecker>,
}

impl<P: Vst3Plugin> Drop for WrapperInitContext<'_, P> {
    fn drop(&mut self) {
        if let Some(samples) = self.pending_requests.latency_changed.take() {
            self.inner.set_latency_samples(samples)
        }
    }
}

impl<P: Vst3Plugin> InitContext<P> for WrapperInitContext<'_, P> {
    fn plugin_api(&self) -> PluginApi {
        PluginApi::Vst3
    }

    fn execute(&self, task: P::BackgroundTask) {
        (self.inner.task_executor.lock())(task);
    }

    fn set_latency_samples(&self, samples: u32) {
        // See this struct's docstring
        self.pending_requests.latency_changed.set(Some(samples));
    }

    fn set_current_voice_capacity(&self, _capacity: u32) {
        // This is only supported by CLAP
    }
}

impl<P: Vst3Plugin> ProcessContext<P> for WrapperProcessContext<'_, P> {
    fn plugin_api(&self) -> PluginApi {
        PluginApi::Vst3
    }

    fn execute_background(&self, task: P::BackgroundTask) {
        let task_posted = self.inner.schedule_background(Task::PluginTask(task));
        nih_debug_assert!(task_posted, "The task queue is full, dropping task...");
    }

    fn execute_gui(&self, task: P::BackgroundTask) {
        let task_posted = self.inner.schedule_gui(Task::PluginTask(task));
        nih_debug_assert!(task_posted, "The task queue is full, dropping task...");
    }

    #[inline]
    fn transport(&self) -> &Transport {
        &self.transport
    }

    fn next_event(&mut self) -> Option<PluginNoteEvent<P>> {
        self.input_events_guard.pop_front()
    }

    fn send_event(&mut self, event: PluginNoteEvent<P>) {
        self.output_events_guard.push_back(event);
    }

    fn set_latency_samples(&self, samples: u32) {
        self.inner.set_latency_samples(samples)
    }

    fn set_current_voice_capacity(&self, _capacity: u32) {
        // This is only supported by CLAP
    }
}

impl<P: Vst3Plugin> GuiContext for WrapperGuiContext<P> {
    fn plugin_api(&self) -> PluginApi {
        PluginApi::Vst3
    }

    fn request_resize(&self) -> bool {
        let task_posted = self.inner.schedule_gui(Task::RequestResize);
        nih_debug_assert!(task_posted, "The task queue is full, dropping task...");

        // TODO: We don't handle resize request failures right now. In practice this should however
        //       not happen.
        true
    }

    // All of these functions are supposed to be called from the main thread, so we'll put some
    // trust in the caller and assume that this is indeed the case
    unsafe fn raw_begin_set_parameter(&self, param: ParamPtr) {
        match &*self.inner.component_handler.borrow() {
            Some(handler) => match self.inner.param_ptr_to_hash.get(&param) {
                Some(hash) => {
                    handler.begin_edit(*hash);
                }
                None => nih_debug_assert_failure!("Unknown parameter: {:?}", param),
            },
            None => nih_debug_assert_failure!("Component handler not yet set"),
        }

        #[cfg(debug_assertions)]
        match self.inner.param_id_from_ptr(param) {
            Some(param_id) => self
                .param_gesture_checker
                .borrow_mut()
                .begin_set_parameter(param_id),
            None => nih_debug_assert_failure!(
                "raw_begin_set_parameter() called with an unknown ParamPtr"
            ),
        }
    }

    unsafe fn raw_set_parameter_normalized(&self, param: ParamPtr, normalized: f32) {
        match self.inner.param_ptr_to_hash.get(&param) {
            Some(hash) => {
                // SosaTune: the plugin applies editor changes itself instead of relying on the
                // host to send them back to the processor (some hosts and plugin containers never
                // do, and then the controls appear dead):
                // - not processing, or the host stopped calling `process()` (transport stopped,
                //   REAPER's bypass...): apply right away;
                // - processing: queue the change, it is applied at the start of the next process
                //   call (never in the middle of one).
                let now_ns = self.inner.created_at.elapsed().as_nanos() as u64;
                let last_ns = self.inner.last_process_ns.load(Ordering::SeqCst);
                let idle = now_ns.saturating_sub(last_ns) > 150_000_000;
                if !self.inner.is_processing.load(Ordering::SeqCst) || idle {
                    self.inner.set_normalized_value_by_hash(
                        *hash,
                        normalized,
                        self.inner
                            .current_buffer_config
                            .load()
                            .map(|c| c.sample_rate),
                    );
                } else if self.inner.gui_param_queue.push((*hash, normalized)).is_err() {
                    nih_debug_assert_failure!("The editor parameter queue is full");
                }

                // Tell the host about the change (automation, undo, saving). If the host never
                // gave us a component handler the value has still been applied above.
                if let Some(handler) = &*self.inner.component_handler.borrow() {
                    handler.perform_edit(*hash, normalized as f64);
                }
            }
            None => nih_debug_assert_failure!("Unknown parameter: {:?}", param),
        }

        #[cfg(debug_assertions)]
        match self.inner.param_id_from_ptr(param) {
            Some(param_id) => self
                .param_gesture_checker
                .borrow_mut()
                .set_parameter(param_id),
            None => {
                nih_debug_assert_failure!("raw_set_parameter() called with an unknown ParamPtr")
            }
        }
    }

    unsafe fn raw_end_set_parameter(&self, param: ParamPtr) {
        match &*self.inner.component_handler.borrow() {
            Some(handler) => match self.inner.param_ptr_to_hash.get(&param) {
                Some(hash) => {
                    handler.end_edit(*hash);
                }
                None => nih_debug_assert_failure!("Unknown parameter: {:?}", param),
            },
            None => nih_debug_assert_failure!("Component handler not yet set"),
        }

        #[cfg(debug_assertions)]
        match self.inner.param_id_from_ptr(param) {
            Some(param_id) => self
                .param_gesture_checker
                .borrow_mut()
                .end_set_parameter(param_id),
            None => {
                nih_debug_assert_failure!("raw_end_set_parameter() called with an unknown ParamPtr")
            }
        }
    }

    fn get_state(&self) -> PluginState {
        self.inner.get_state_object()
    }

    fn set_state(&self, state: PluginState) {
        self.inner.set_state_object_from_gui(state)
    }
}
