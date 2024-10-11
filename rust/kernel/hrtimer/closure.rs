// SPDX-License-Identifier: GPL-2.0

use super::{pin_init, tbox::BoxTimerHandle, Timer, TimerCallback, TimerPointer, TimerRestart};
use crate::{
    alloc::{flags, Flags},
    impl_has_timer,
    irq::IrqDisabled,
    new_spinlock_irq,
    prelude::*,
    sync::SpinLockIrq,
    time::Ktime,
};
use macros::pin_data;

#[pin_data]
pub struct ClosureTimer<T> {
    #[pin]
    timer: Timer<ClosureTimer<T>>,
    #[pin]
    callback: SpinLockIrq<Option<T>>,
}

impl_has_timer! {
    impl{T} HasTimer<Self> for ClosureTimer<T> { self.timer }
}

impl<T> TimerCallback for ClosureTimer<T>
where
    T: FnOnce(IrqDisabled<'_>) + 'static,
{
    type CallbackTarget<'a> = Pin<Box<ClosureTimer<T>>>;
    type CallbackTargetParameter<'a> = &'a ClosureTimer<T>;

    fn run(this: Self::CallbackTargetParameter<'_>, irq: IrqDisabled<'_>) -> TimerRestart
    where
        Self: Sized,
    {
        if let Some(callback) = this.callback.lock_with(irq).take() {
            callback(irq);
        }
        TimerRestart::NoRestart
    }
}

impl<T> ClosureTimer<T>
where
    T: FnOnce(IrqDisabled<'_>) + 'static,
    T: Send,
    T: Sync,
{
    fn new(f: T, flags: Flags) -> Result<Pin<Box<Self>>> {
        Box::pin_init(
            pin_init!(
                Self {
                    timer <- Timer::new(super::TimerMode::Relative, super::ClockSource::Monotonic),
                    callback <- new_spinlock_irq!(Some(f)),
                }
            ),
            flags,
        )
    }
}

/// Start a timer that executes `f` after `expires` time.
pub fn start_function<T>(expires: Ktime, f: T) -> Result<BoxTimerHandle<ClosureTimer<T>>>
where
    T: FnOnce(IrqDisabled<'_>) + 'static,
    T: Send,
    T: Sync,
{
    let timer = ClosureTimer::<T>::new(f, flags::GFP_KERNEL)?;
    let handle = <Pin<Box<ClosureTimer<T>>> as TimerPointer>::start(timer, expires);
    Ok(handle)
}
