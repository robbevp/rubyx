//! Shared test infrastructure for Python API initialization and GIL management.
//!
//! This module provides a single, centralized point for Python initialization across all test
//! modules. It prevents the SIGSEGV that occurs when multiple test modules each initialize
//! Python independently.
//!
//! # Pattern
//!
//! The key insight is that `PyGILState_Ensure` / `PyGILState_Release` (our `ensure_gil` /
//! `release_gil`) are **thread-safe**: they create per-thread state automatically. This
//! makes them safe to call from any OS thread in Cargo's test-runner pool.
//!
//! In contrast, `PyEval_SaveThread` / `PyEval_RestoreThread` bind to a specific OS thread
//! and deadlock when restored on a different thread — which happens with `#[serial]` tests
//! because Cargo's thread pool may schedule successive tests on different threads.
//!
//! We:
//! 1. Initialize Python once via `OnceLock`.
//! 2. Immediately call `save_thread()` to release the GIL (discard the returned state).
//! 3. Each test acquires the GIL with `ensure_gil()` (via `GilGuard`) and releases it on drop.
//!
//! # Ruby Threading
//!
//! Ruby's GVL (Global VM Lock) binds to the thread that called `embed::init()`, and Ruby
//! C API functions require proper thread-local state (`ruby_current_ec_ptr`). Rust's test
//! harness spawns a **new OS thread for every test**, so neither `Ruby::get()` nor direct
//! C API calls work from test threads.
//!
//! The solution uses an executor pattern:
//! 1. A dedicated long-lived thread calls `embed::init()` and then releases the GVL via
//!    `rb_thread_call_without_gvl`, running an executor loop that waits for work items.
//! 2. Test threads send closures to the executor via a channel.
//! 3. The executor calls `rb_thread_call_with_gvl` for each work item — this works because
//!    the executor thread is inside `rb_thread_call_without_gvl` and is still registered
//!    with Ruby. Inside `rb_thread_call_with_gvl`, `Ruby::get()` succeeds normally.
//! 4. Results are sent back to the test thread via a one-shot channel.

use crate::python_api::PythonApi;
use crate::python_ffi::PyGILState;
use crate::python_finder::find_libpython;
use magnus::Ruby;
use std::any::Any;
use std::ffi::c_void;
use std::panic::{self, AssertUnwindSafe};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Mutex, OnceLock};

extern "C" {
    fn rb_thread_call_without_gvl(
        func: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
        data1: *mut c_void,
        ubf: Option<unsafe extern "C" fn(*mut c_void)>,
        data2: *mut c_void,
    ) -> *mut c_void;

    fn rb_thread_call_with_gvl(
        func: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
        data1: *mut c_void,
    ) -> *mut c_void;
}

/// Tracks whether Python initialization has been attempted.
static PYTHON_INIT: OnceLock<bool> = OnceLock::new();

/// Get the shared Python API instance, initializing if necessary.
///
/// Stores the `PythonApi` in `crate::API` so that both test helpers and
/// production code paths (e.g. `method_missing` → `crate::api()`) share
/// the same instance.
///
/// After this returns, the GIL is **not** held. Use `skip_if_no_python()` to
/// acquire the GIL via a `GilGuard`.
pub fn get_api() -> Option<&'static PythonApi> {
    let success = PYTHON_INIT.get_or_init(|| {
        let path = match find_libpython() {
            Some(p) => p,
            None => return false,
        };
        let mut api = match unsafe { PythonApi::load(&path) } {
            Ok(a) => a,
            Err(_) => return false,
        };
        api.initialize();
        let _ = api.install_async_to_sync_class();

        // Release the GIL that Py_Initialize left us holding.
        // We intentionally discard the returned PyThreadState — from here on,
        // all GIL acquisition goes through the thread-safe ensure_gil/release_gil.
        let _ = api.save_thread();

        // Store in the crate-level API so crate::api() works in production code
        // paths called from tests (e.g. method_missing).
        let _ = crate::API.set(api);
        true
    });
    if *success {
        crate::API.get()
    } else {
        None
    }
}

/// RAII guard that manages GIL acquisition and release for tests.
///
/// Created by `skip_if_no_python()`. Holds the GIL for the duration of its lifetime.
/// Uses `PyGILState_Ensure` / `PyGILState_Release` which are thread-safe and work
/// correctly regardless of which OS thread the test is scheduled on.
pub struct GilGuard<'a> {
    api: &'a PythonApi,
    gil_state: PyGILState,
}

impl<'a> GilGuard<'a> {
    /// Access the Python API while holding the GIL.
    pub fn api(&self) -> &'a PythonApi {
        self.api
    }
}

impl<'a> Drop for GilGuard<'a> {
    fn drop(&mut self) {
        self.api.release_gil(self.gil_state);
    }
}

/// Skip the test if Python is not available, otherwise return a GIL guard.
///
/// This is the main entry point for tests. It:
/// 1. Initializes Python (if not already done)
/// 2. Acquires the GIL via `ensure_gil()` (thread-safe)
/// 3. Returns a guard that releases the GIL on drop
///
/// # Example
///
/// ```ignore
/// #[test]
/// fn test_something() {
///     let Some(guard) = skip_if_no_python() else { return; };
///     let api = guard.api();
///     api.run_simple_string("x = 42").unwrap();
/// }
/// ```
pub fn skip_if_no_python() -> Option<GilGuard<'static>> {
    let api = get_api()?;
    let gil_state = api.ensure_gil();
    Some(GilGuard { api, gil_state })
}

// ---------------------------------------------------------------------------
// Ruby executor pattern
// ---------------------------------------------------------------------------

/// Type-erased work item sent from test threads to the Ruby executor.
type WorkFn = Box<dyn FnOnce() + Send>;

/// Carries a work item through the C callback interface, with space to
/// store a panic payload if the work item panics.
struct WorkSlot {
    work: Option<WorkFn>,
    panic: Option<Box<dyn Any + Send>>,
}

/// Holds the sender half of the executor channel, wrapped in a Mutex because
/// `std::sync::mpsc::Sender` is `!Sync` (required for statics via `OnceLock`).
/// The Mutex is effectively uncontended since `#[serial]` ensures only one test
/// runs at a time.
static RUBY_EXECUTOR: OnceLock<Mutex<Sender<WorkFn>>> = OnceLock::new();

/// Executor loop that runs inside `rb_thread_call_without_gvl` on the Ruby
/// init thread. Receives work items from test threads and dispatches them
/// via `rb_thread_call_with_gvl`.
///
/// This works because the executor thread is still registered with Ruby
/// (it called `embed::init()`), so `rb_thread_call_with_gvl` is valid here.
unsafe extern "C" fn executor_loop(data: *mut c_void) -> *mut c_void {
    let rx = &*(data as *const Receiver<WorkFn>);
    while let Ok(work) = rx.recv() {
        let mut slot = WorkSlot {
            work: Some(work),
            panic: None,
        };
        rb_thread_call_with_gvl(run_work_with_gvl, &mut slot as *mut WorkSlot as *mut c_void);
        // If the work item panicked, the panic payload is in slot.panic.
        // The test thread will see the result channel drop (no send) and
        // the with_ruby_python function handles this. But we can't
        // resume_unwind here (we're in a C callback). The panic info
        // was already sent via the result channel by the work item itself.
    }
    std::ptr::null_mut()
}

/// Callback for `rb_thread_call_with_gvl`. Runs the work item with
/// proper Ruby thread-local state and GVL held.
///
/// Panics are caught via `catch_unwind` to prevent unwinding across the
/// FFI boundary (which is UB). The panic payload is stored back into the
/// work slot so the caller can propagate it.
unsafe extern "C" fn run_work_with_gvl(data: *mut c_void) -> *mut c_void {
    let slot = &mut *(data as *mut WorkSlot);
    if let Some(f) = slot.work.take() {
        if let Err(payload) = panic::catch_unwind(AssertUnwindSafe(f)) {
            slot.panic = Some(payload);
        }
    }
    std::ptr::null_mut()
}

/// Initialize the Ruby VM once on a dedicated long-lived thread and start
/// the executor loop.
///
/// The dedicated thread:
/// 1. Calls `embed::init()` — becoming Ruby's "main" thread
/// 2. Defines the `RubyxObject` class
/// 3. Releases the GVL via `rb_thread_call_without_gvl` and enters the
///    executor loop, waiting for work items from test threads
fn ensure_ruby_vm() {
    RUBY_EXECUTOR.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<WorkFn>();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();

        // Spawn a dedicated thread that will be Ruby's "main" thread for the
        // entire test process lifetime.
        std::thread::spawn(move || {
            let cleanup = unsafe { magnus::embed::init() };
            let ruby: &Ruby = &cleanup;
            ruby.define_class("RubyxObject", ruby.class_object())
                .expect("Failed to define RubyxObject class for tests");

            // Signal that Ruby is ready before releasing the GVL.
            ready_tx.send(()).expect("ready channel send failed");

            // Leak the receiver so it lives forever (the executor loop
            // borrows it via raw pointer through the C callback interface).
            let rx_ptr = Box::into_raw(Box::new(rx));

            // Release the GVL and enter the executor loop. The loop receives
            // work items from test threads and dispatches them via
            // rb_thread_call_with_gvl.
            unsafe {
                rb_thread_call_without_gvl(
                    executor_loop,
                    rx_ptr as *mut c_void,
                    None,
                    std::ptr::null_mut(),
                );
            }

            // Never reached, but prevent cleanup from running.
            std::mem::forget(cleanup);
        });

        ready_rx.recv().expect("Ruby init thread failed");
        Mutex::new(tx)
    });
}

/// Run a closure with both Ruby GVL and Python GIL held.
///
/// Returns `None` if Python is not available (test should be skipped).
///
/// This is the main entry point for tests that need both Ruby and Python.
/// The closure is sent to the Ruby executor thread, which runs it inside
/// `rb_thread_call_with_gvl` with proper Ruby thread-local state.
/// The Python GIL is also acquired on the executor thread before the
/// closure runs.
///
/// # Example
///
/// ```ignore
/// #[test]
/// #[serial]
/// fn test_something() {
///     with_ruby_python(|ruby, api| {
///         let py_str = api.string_from_str("hello");
///         let rb_str = "hello".into_value_with(ruby);
///         // ...
///     });
/// }
/// ```
pub fn with_ruby_python<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&Ruby, &'static PythonApi) -> R + Send + 'static,
    R: Send + 'static,
{
    // Initialize Python first — must happen before Ruby VM init to avoid
    // interference with Python C extension loading.
    let api = get_api()?;
    ensure_ruby_vm();

    let (result_tx, result_rx) = std::sync::mpsc::channel::<Result<R, Box<dyn Any + Send>>>();

    let work: WorkFn = Box::new(move || {
        // Inside rb_thread_call_with_gvl: Ruby GVL is held, thread-local
        // state is set up, Ruby::get_unchecked() is safe.
        let ruby = unsafe { Ruby::get_unchecked() };

        // Acquire the Python GIL on the executor thread.
        let gil = api.ensure_gil();
        let result = panic::catch_unwind(AssertUnwindSafe(|| f(&ruby, api)));
        api.release_gil(gil);

        let _ = result_tx.send(result);
    });

    // Send the work item to the executor thread.
    RUBY_EXECUTOR
        .get()
        .expect("executor not initialized")
        .lock()
        .expect("executor mutex poisoned")
        .send(work)
        .expect("executor channel closed");

    // Block until the executor finishes running our closure.
    // If the closure panicked, resume the panic on the test thread.
    match result_rx.recv().expect("executor result channel closed") {
        Ok(value) => Some(value),
        Err(payload) => panic::resume_unwind(payload),
    }
}

// Keep the old API available for backward compatibility during migration.
// These can be removed once all tests are migrated to with_ruby_python.

/// RAII guard that holds both a Ruby VM handle and a Python GIL.
pub struct RubyPythonGuard<'a> {
    ruby: Ruby,
    gil_guard: GilGuard<'a>,
}

impl<'a> RubyPythonGuard<'a> {
    pub fn api(&self) -> &'a PythonApi {
        self.gil_guard.api()
    }
    pub fn ruby(&self) -> &Ruby {
        &self.ruby
    }
}

/// Skip the test if Python is not available, and ensure the Ruby VM is initialized.
///
/// **Deprecated**: Use `with_ruby_python` instead. This function only works when
/// the test happens to run on the Ruby init thread (which is unreliable with Cargo's
/// test harness).
pub fn skip_if_no_ruby_python() -> Option<RubyPythonGuard<'static>> {
    let _ = get_api()?;
    ensure_ruby_vm();
    let gil_guard = skip_if_no_python()?;
    let ruby = Ruby::get().ok()?;
    Some(RubyPythonGuard { ruby, gil_guard })
}
