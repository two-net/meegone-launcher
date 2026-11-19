//! [`SchedulerAdapter`] on Android: MeeGone's one scheduled job, the daily image's check
//! (ARCH-07, WALL-03, WALL-04, ADR 0029).
//!
//! Requests go to the Java `Bridge` object, whose method names and signatures must stay in step
//! with the ones looked up here. `ImageJobService` schedules the job with `JobScheduler`: no
//! earlier than asked, on the network asked for, and not while the battery is low. A job asked
//! for while the job runs is scheduled when it finishes, so the running job isn't stopped.

use super::jni::{Env, GlobalRef, JValue, LocalRef, MethodId, Vm};
use crate::adapters::SchedulerAdapter;
use crate::model::{DataError, ImageJob, JobNetwork};

pub struct AndroidScheduler {
    vm: Vm,
    bridge: GlobalRef,
    /// `String scheduleImageJob(long notBeforeMs, int anyNetwork)`: `null`, or a short message
    /// when the system can't schedule it.
    schedule_image_job: MethodId,
    /// `String cancelImageJob()`: `null`, or a short message when the system can't schedule.
    cancel_image_job: MethodId,
}

impl AndroidScheduler {
    /// Looks up the bridge's methods once, on the thread that started MeeGone.
    pub fn new(env: Env, bridge: &LocalRef) -> Result<Self, DataError> {
        Ok(Self {
            vm: env.vm()?,
            schedule_image_job: env.method_id(
                bridge,
                c"scheduleImageJob",
                c"(JI)Ljava/lang/String;",
            )?,
            cancel_image_job: env.method_id(bridge, c"cancelImageJob", c"()Ljava/lang/String;")?,
            bridge: env.global_ref(bridge)?,
        })
    }

    fn call(&self, method: MethodId, args: &[JValue]) -> Result<(), DataError> {
        let env = self.vm.env()?;
        match env.call_string(&self.bridge, method, args)? {
            None => Ok(()),
            // The image is then checked at the next start (SRS 8.1).
            Some(message) => Err(DataError::Unsupported(message)),
        }
    }
}

impl SchedulerAdapter for AndroidScheduler {
    fn schedule_image_job(&self, job: ImageJob) -> Result<(), DataError> {
        let not_before = JValue::long(i64::try_from(job.not_before_ms).unwrap_or(i64::MAX));
        let any_network = JValue::int(match job.network {
            JobNetwork::Unmetered => 0,
            JobNetwork::Any => 1,
        });
        self.call(self.schedule_image_job, &[not_before, any_network])
    }

    fn cancel_image_job(&self) -> Result<(), DataError> {
        self.call(self.cancel_image_job, &[])
    }
}
