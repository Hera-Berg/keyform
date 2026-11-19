//! Files opened from the Finder (double-click, "Open With", dropping on the
//! Dock icon) reach a macOS app as an `odoc` Apple Event, not as argv. This
//! crate installs a handler for that event and queues the paths; the app
//! drains the queue every frame. On other platforms both functions do
//! nothing (files arrive on the command line there).

use std::path::PathBuf;
use std::sync::Mutex;

static QUEUE: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Paths received since the last call.
pub fn take() -> Vec<PathBuf> {
    QUEUE
        .lock()
        .map(|mut q| std::mem::take(&mut *q))
        .unwrap_or_default()
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn push(p: PathBuf) {
    if let Ok(mut q) = QUEUE.lock() {
        q.push(p);
    }
}

/// Installs the open-documents handler. Call it before the event loop starts
/// and again once the app has finished launching (AppKit may install its own
/// default handler during launch; the last registration wins).
pub fn install() {
    #[cfg(target_os = "macos")]
    mac::install();
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2::rc::Retained;
    use objc2::runtime::NSObject;
    use objc2::{define_class, msg_send, sel, AllocAnyThread};
    use objc2_foundation::{NSAppleEventDescriptor, NSAppleEventManager};

    /// 'aevt' / 'odoc' / '----' (kCoreEventClass, kAEOpenDocuments,
    /// keyDirectObject).
    const CORE_EVENT_CLASS: u32 = u32::from_be_bytes(*b"aevt");
    const OPEN_DOCUMENTS: u32 = u32::from_be_bytes(*b"odoc");
    const DIRECT_OBJECT: u32 = u32::from_be_bytes(*b"----");

    define_class!(
        // SAFETY: NSObject has no subclassing requirements; no Drop impl.
        #[unsafe(super(NSObject))]
        #[name = "KeyformOpenDocumentsHandler"]
        struct Handler;

        impl Handler {
            #[unsafe(method(handleOpen:withReply:))]
            fn handle_open(&self, event: &NSAppleEventDescriptor, _reply: &NSAppleEventDescriptor) {
                // SAFETY: paramDescriptorForKeyword: takes an AEKeyword
                // (FourCharCode = u32) and returns a descriptor or nil.
                let list: Option<Retained<NSAppleEventDescriptor>> =
                    unsafe { msg_send![event, paramDescriptorForKeyword: DIRECT_OBJECT] };
                let Some(list) = list else { return };
                let n = list.numberOfItems();
                // A single file may come as the descriptor itself.
                let items: Vec<Retained<NSAppleEventDescriptor>> = if n <= 0 {
                    vec![list]
                } else {
                    (1..=n).filter_map(|i| list.descriptorAtIndex(i)).collect()
                };
                for d in items {
                    if let Some(url) = d.fileURLValue() {
                        if let Some(p) = url.path() {
                            super::push(p.to_string().into());
                        }
                    }
                }
            }
        }
    );

    pub fn install() {
        let handler: Retained<Handler> = {
            let this = Handler::alloc().set_ivars(());
            // SAFETY: plain NSObject initialisation.
            unsafe { msg_send![super(this), init] }
        };
        let mgr = NSAppleEventManager::sharedAppleEventManager();
        // SAFETY: the handler responds to the selector with the expected
        // signature; event class and ID are FourCharCodes (u32).
        unsafe {
            let _: () = msg_send![
                &*mgr,
                setEventHandler: &*handler,
                andSelector: sel!(handleOpen:withReply:),
                forEventClass: CORE_EVENT_CLASS,
                andEventID: OPEN_DOCUMENTS
            ];
        }
        // The manager doesn't retain its handler: keep it for the process.
        std::mem::forget(handler);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn queue_drains() {
        super::push("/tmp/a.cmo3".into());
        super::push("/tmp/b.can3".into());
        assert_eq!(super::take().len(), 2);
        assert!(super::take().is_empty());
        super::install(); // no-op off macOS
    }
}
