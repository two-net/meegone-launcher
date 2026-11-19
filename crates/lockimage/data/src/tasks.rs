//! Work the user asks of the lock-screen image that runs on the image job's thread (ADR 0030,
//! ADR 0032, ADR 0036): "Update now", which Resume and Unpin are too (WALL-11, WALL-14,
//! WALL-17), browsing the past week, pinning one of its images (WALL-17), and setup's preview of
//! today's image and its first check (ONB-01).
//!
//! Each of them blocks on the network or the wallpaper, so the holder, on the main thread,
//! only queues it here and asks the platform to run the queue on the job's thread. The job's
//! thread takes one task at a time, and marks it running until it ends. What is waiting or
//! running is what the screens show as on its way ("Updating…", "Pinning…"), whatever else
//! reports back meanwhile, such as an automatic check. Memory only: a task the process never
//! ran is gone with it.

use std::collections::VecDeque;
use std::sync::{Mutex, MutexGuard, PoisonError};

/// One piece of work for the job's thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageTask {
    /// Fetches today's image at once, over any connection, after putting the current image
    /// back on the lock screen and letting go of a pin (WALL-11, WALL-14, WALL-17).
    UpdateNow,
    /// Reads the past week's images and their previews (WALL-17).
    Browse,
    /// Pins the past week's image with the feed's id (WALL-17).
    Pin(String),
    /// Reads today's image and its preview for setup, without caching or setting anything
    /// (ONB-01, WALL-02).
    PreviewToday,
    /// Runs the daily check at once, within its rules, once setup has turned the daily image on
    /// (ONB-03, WALL-03, WALL-04).
    Check,
}

/// The tasks waiting, and the one running. Clones of its `Arc` share it.
#[derive(Debug, Default)]
pub struct ImageTasks {
    queue: Mutex<Queue>,
}

#[derive(Debug, Default)]
struct Queue {
    running: Option<ImageTask>,
    waiting: VecDeque<ImageTask>,
}

impl ImageTasks {
    fn queue(&self) -> MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Queues `task`, unless the same task is already waiting or running. Returns whether it
    /// was queued.
    pub fn push(&self, task: ImageTask) -> bool {
        let mut queue = self.queue();
        if queue.running.as_ref() == Some(&task) || queue.waiting.contains(&task) {
            return false;
        }
        queue.waiting.push_back(task);
        true
    }

    /// Takes back a waiting `task` that can't be run after all, such as when the platform
    /// refuses to run the queue.
    pub fn withdraw(&self, task: &ImageTask) {
        self.queue().waiting.retain(|waiting| waiting != task);
    }

    /// The tasks waiting or running, the running one first.
    pub fn pending(&self) -> Vec<ImageTask> {
        let queue = self.queue();
        queue
            .running
            .iter()
            .chain(&queue.waiting)
            .cloned()
            .collect()
    }

    /// Runs the waiting tasks one after another, each through `run`, until none waits. Each is
    /// marked running until `run` returns, or unwinds.
    pub fn run_each(&self, run: &mut dyn FnMut(&ImageTask)) {
        /// Ends the running task however `run` ends.
        struct Running<'a>(&'a ImageTasks);
        impl Drop for Running<'_> {
            fn drop(&mut self) {
                self.0.queue().running = None;
            }
        }

        loop {
            let task = {
                let mut queue = self.queue();
                let Some(task) = queue.waiting.pop_front() else {
                    return;
                };
                queue.running = Some(task.clone());
                task
            };
            let _running = Running(self);
            run(&task);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use super::*;

    #[test]
    fn a_task_is_queued_once_and_runs_in_order() {
        let tasks = ImageTasks::default();
        assert!(tasks.push(ImageTask::Browse));
        assert!(tasks.push(ImageTask::Pin("a1".into())));
        assert!(!tasks.push(ImageTask::Browse));
        assert!(tasks.push(ImageTask::Pin("b2".into())));
        assert_eq!(
            tasks.pending(),
            [
                ImageTask::Browse,
                ImageTask::Pin("a1".into()),
                ImageTask::Pin("b2".into())
            ]
        );

        let mut ran = Vec::new();
        tasks.run_each(&mut |task| {
            // While it runs, it is pending, and the same task isn't queued again.
            assert_eq!(tasks.pending()[0], *task);
            assert!(!tasks.push(task.clone()));
            ran.push(task.clone());
        });
        assert_eq!(
            ran,
            [
                ImageTask::Browse,
                ImageTask::Pin("a1".into()),
                ImageTask::Pin("b2".into())
            ]
        );
        assert!(tasks.pending().is_empty());
    }

    #[test]
    fn a_task_queued_while_another_runs_runs_next() {
        let tasks = ImageTasks::default();
        tasks.push(ImageTask::Browse);
        let mut ran = Vec::new();
        tasks.run_each(&mut |task| {
            if *task == ImageTask::Browse {
                tasks.push(ImageTask::UpdateNow);
            }
            ran.push(task.clone());
        });
        assert_eq!(ran, [ImageTask::Browse, ImageTask::UpdateNow]);
    }

    #[test]
    fn a_withdrawn_task_never_runs() {
        let tasks = ImageTasks::default();
        tasks.push(ImageTask::UpdateNow);
        tasks.withdraw(&ImageTask::UpdateNow);
        assert!(tasks.pending().is_empty());
        tasks.run_each(&mut |_| panic!("nothing waits"));
    }

    #[test]
    fn a_task_that_fails_badly_still_ends() {
        // ARCH-09: a bug never leaves "Pinning…" showing for good.
        let tasks = ImageTasks::default();
        tasks.push(ImageTask::Pin("a1".into()));
        let unwound = catch_unwind(AssertUnwindSafe(|| tasks.run_each(&mut |_| panic!("bug"))));

        assert!(unwound.is_err());
        assert!(tasks.pending().is_empty());
    }
}
