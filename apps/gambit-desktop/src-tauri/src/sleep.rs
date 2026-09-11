//! Minimal `AppKit` bridge. Unsafe code is confined to registering/removing the
//! typed observer; the callback never dereferences Objective-C notification data.
#![allow(unsafe_code)]

use block2::RcBlock;
use objc2::{rc::Retained, runtime::ProtocolObject};
use objc2_app_kit::{NSWorkspace, NSWorkspaceWillSleepNotification};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSObjectProtocol};
use std::ptr::NonNull;

pub(super) struct SleepObserver {
    center: Retained<NSNotificationCenter>,
    token: Retained<ProtocolObject<dyn NSObjectProtocol>>,
}

impl SleepObserver {
    pub fn new(pause: impl Fn() + Send + Sync + 'static) -> Self {
        let center = NSWorkspace::sharedWorkspace().notificationCenter();
        let block = RcBlock::new(move |_: NonNull<NSNotification>| pause());
        // SAFETY: no object filter or operation queue is supplied. Foundation
        // copies the block; all captured data is owned, Send + Sync + 'static.
        // The notification is ignored rather than dereferenced. The returned
        // observer token is retained until Drop unregisters it from this center.
        let token = unsafe {
            center.addObserverForName_object_queue_usingBlock(
                Some(NSWorkspaceWillSleepNotification),
                None,
                None,
                &block,
            )
        };
        Self { center, token }
    }
}

impl Drop for SleepObserver {
    fn drop(&mut self) {
        // SAFETY: this is precisely the retained token returned by this center.
        unsafe {
            self.center.removeObserver((*self.token).as_ref());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[test]
    fn native_sleep_notification_calls_handler_and_drop_unregisters() {
        let count = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&count);
        let session = gambit_coaching::EngineWorker::default().session();
        let cancel = session.clone();
        let observer = SleepObserver::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            cancel.cancel();
        });
        let center = observer.center.clone();
        // SAFETY: posts only to this process's workspace notification center,
        // not the system power service. No object payload is supplied.
        unsafe {
            center.postNotificationName_object(NSWorkspaceWillSleepNotification, None);
        }
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert!(session.cancellation().is_cancelled());
        drop(observer);
        unsafe {
            center.postNotificationName_object(NSWorkspaceWillSleepNotification, None);
        }
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }
}
