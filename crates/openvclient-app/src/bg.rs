//! Bridge between the UI thread and the tokio runtime. The UI never awaits:
//! it spawns a [`Job`] and polls it each frame; completion triggers a repaint.

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use parking_lot::Mutex;
use tokio::sync::oneshot;

#[derive(Clone)]
pub struct TaskInfo {
    pub id: u64,
    pub label: String,
    pub started: Instant,
    pub abort: tokio::task::AbortHandle,
}

#[derive(Clone)]
pub struct Bg {
    pub rt: tokio::runtime::Handle,
    pub ctx: egui::Context,
    tasks: Arc<Mutex<Vec<TaskInfo>>>,
    next: Arc<AtomicU64>,
}

impl Bg {
    pub fn new(rt: tokio::runtime::Handle, ctx: egui::Context) -> Self {
        Self { rt, ctx, tasks: Default::default(), next: Arc::new(AtomicU64::new(1)) }
    }

    /// Run `fut` in the background. `label` shows in the status bar while it runs.
    pub fn spawn<T, F>(&self, label: impl Into<String>, fut: F) -> Job<T>
    where
        T: Send + 'static,
        F: Future<Output = T> + Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        let ctx = self.ctx.clone();
        let tasks = self.tasks.clone();
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let handle = self.rt.spawn(async move {
            let out = fut.await;
            tasks.lock().retain(|t| t.id != id);
            let _ = tx.send(out);
            ctx.request_repaint();
        });
        let abort = handle.abort_handle();
        let label = label.into();
        if !label.is_empty() {
            self.tasks.lock().push(TaskInfo { id, label, started: Instant::now(), abort: abort.clone() });
        }
        Job { rx: Some(rx), abort, started: Instant::now(), done: None, id, tasks: self.tasks.clone() }
    }

    pub fn running(&self) -> Vec<TaskInfo> {
        self.tasks.lock().clone()
    }

    pub fn cancel_task(&self, id: u64) {
        let mut t = self.tasks.lock();
        if let Some(pos) = t.iter().position(|x| x.id == id) {
            t[pos].abort.abort();
            t.remove(pos);
        }
    }
}

/// A background result, polled from the UI thread.
pub struct Job<T> {
    rx: Option<oneshot::Receiver<T>>,
    abort: tokio::task::AbortHandle,
    pub started: Instant,
    done: Option<T>,
    id: u64,
    tasks: Arc<Mutex<Vec<TaskInfo>>>,
}

impl<T> Job<T> {
    fn pump(&mut self) {
        if let Some(rx) = &mut self.rx {
            match rx.try_recv() {
                Ok(v) => {
                    self.done = Some(v);
                    self.rx = None;
                }
                Err(oneshot::error::TryRecvError::Empty) => {}
                Err(oneshot::error::TryRecvError::Closed) => self.rx = None,
            }
        }
    }

    /// Take the result once it is ready.
    pub fn take(&mut self) -> Option<T> {
        self.pump();
        self.done.take()
    }

    /// True when the task was aborted before producing a value.
    pub fn is_dead(&mut self) -> bool {
        self.pump();
        self.rx.is_none() && self.done.is_none()
    }

    pub fn abort(&self) {
        self.abort.abort();
        self.tasks.lock().retain(|t| t.id != self.id);
    }
}

impl<T> Drop for Job<T> {
    fn drop(&mut self) {
        // Results nobody waits for still finish (e.g. a save), but their
        // status-bar entry goes away with the owner.
        if self.rx.is_some() {
            self.tasks.lock().retain(|t| t.id != self.id);
        }
    }
}

/// Poll an optional job slot: returns the value when it completes and
/// clears the slot.
pub fn poll<T>(slot: &mut Option<Job<T>>) -> Option<T> {
    let job = slot.as_mut()?;
    if let Some(v) = job.take() {
        *slot = None;
        return Some(v);
    }
    if job.is_dead() {
        *slot = None;
    }
    None
}
