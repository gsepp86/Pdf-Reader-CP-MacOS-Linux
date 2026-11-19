//! macOS: i PDF che il Finder chiede di aprire (doppio clic, "Apri con", icona nel Dock).
//!
//! Su macOS i file non arrivano sulla riga di comando: LaunchServices manda l'Apple Event
//! "open documents" (kAEOpenDocuments). AppKit lo passa al suo NSDocumentController, che
//! non sa aprire i PDF di questo programma e fa comparire l'errore di formato. Qui ci si
//! mette al posto suo con un gestore registrato quando l'app sta per finire di avviarsi,
//! cosi' arriva anche l'evento che l'ha lanciata (e quelli che arrivano a programma aperto).
#![cfg(target_os = "macos")]

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, sel, MainThreadOnly};
use objc2_foundation::{
    MainThreadMarker, NSAppleEventDescriptor, NSAppleEventManager, NSNotification,
    NSNotificationCenter, NSObject, NSString, NSURL,
};

/// kCoreEventClass ('aevt') e kAEOpenDocuments ('odoc').
const CORE_EVENT_CLASS: u32 = u32::from_be_bytes(*b"aevt");
const OPEN_DOCUMENTS: u32 = u32::from_be_bytes(*b"odoc");
/// keyDirectObject ('----'): il parametro che porta i file.
const DIRECT_OBJECT: u32 = u32::from_be_bytes(*b"----");

/// File ricevuti e non ancora aperti, dal piu' vecchio.
static PENDING: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
/// La finestra, per svegliarla quando un file arriva mentre il programma e' fermo.
static CTX: OnceLock<eframe::egui::Context> = OnceLock::new();

define_class!(
    // SAFETY: NSObject non ha requisiti di sottoclasse e questa classe non implementa Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "PDFReaderCPOpenDocuments"]
    struct OpenDocuments;

    impl OpenDocuments {
        /// NSApplication lo manda appena prima di finire l'avvio: a quel punto AppKit ha
        /// gia' installato il suo gestore, quindi il nostro lo sostituisce.
        #[unsafe(method(willFinishLaunching:))]
        fn will_finish_launching(&self, _notification: &NSNotification) {
            self.register();
        }

        /// Gestore dell'Apple Event "open documents".
        #[unsafe(method(openDocuments:withReplyEvent:))]
        fn open_documents(&self, event: &NSAppleEventDescriptor, _reply: &NSAppleEventDescriptor) {
            let files = files(event);
            if files.is_empty() {
                return;
            }
            if let Ok(mut queue) = PENDING.lock() {
                queue.extend(files);
            }
            if let Some(ctx) = CTX.get() {
                ctx.request_repaint();
            }
        }
    }
);

impl OpenDocuments {
    fn register(&self) {
        let manager = NSAppleEventManager::sharedAppleEventManager();
        let target: &NSObject = self;
        // SAFETY: `target` risponde a openDocuments:withReplyEvent: con la firma che
        // NSAppleEventManager chiama, e vive quanto il programma (vedi `install`).
        unsafe {
            let _: () = msg_send![
                &*manager,
                setEventHandler: target,
                andSelector: sel!(openDocuments:withReplyEvent:),
                forEventClass: CORE_EVENT_CLASS,
                andEventID: OPEN_DOCUMENTS,
            ];
        }
    }
}

/// Percorso di un descrittore che punta a un file (nil per tutto il resto).
fn file_path(desc: &NSAppleEventDescriptor) -> Option<PathBuf> {
    // SAFETY: il descrittore e' valido; fileURLValue e path danno nil se non e' un file.
    let url: Option<Retained<NSURL>> = unsafe { msg_send![desc, fileURLValue] };
    let path: Option<Retained<NSString>> = unsafe { msg_send![&*url?, path] };
    Some(PathBuf::from(path?.to_string()))
}

/// I file dell'evento: l'oggetto diretto puo' essere un solo file o una lista di file.
fn files(event: &NSAppleEventDescriptor) -> Vec<PathBuf> {
    // SAFETY: paramDescriptorForKeyword: restituisce nil se il parametro non c'e'.
    let direct: Option<Retained<NSAppleEventDescriptor>> =
        unsafe { msg_send![event, paramDescriptorForKeyword: DIRECT_OBJECT] };
    let Some(direct) = direct else {
        return Vec::new();
    };
    if let Some(path) = file_path(&direct) {
        return vec![path];
    }
    // SAFETY: numberOfItems e descriptorAtIndex: sono le accessor di NSAppleEventDescriptor.
    let count: i64 = unsafe { msg_send![&*direct, numberOfItems] };
    let mut out = Vec::new();
    for i in 1..=count {
        let item: Option<Retained<NSAppleEventDescriptor>> =
            unsafe { msg_send![&*direct, descriptorAtIndex: i] };
        if let Some(path) = item.and_then(|d| file_path(&d)) {
            out.push(path);
        }
    }
    out
}

/// Inizia a ricevere i file che macOS chiede di aprire: va chiamata sul thread principale,
/// prima di `eframe::run_native`.
pub fn install() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    // SAFETY: `init` di NSObject su una classe registrata da define_class!.
    let handler: Retained<OpenDocuments> = unsafe { msg_send![mtm.alloc::<OpenDocuments>(), init] };
    handler.register(); // nel caso l'app abbia gia' finito di avviarsi
    let center = NSNotificationCenter::defaultCenter();
    let name = NSString::from_str("NSApplicationWillFinishLaunchingNotification");
    let target: &NSObject = &handler;
    let no_object: Option<&AnyObject> = None;
    // SAFETY: `target` implementa il selettore e vive quanto il programma.
    unsafe {
        let _: () = msg_send![
            &*center,
            addObserver: target,
            selector: sel!(willFinishLaunching:),
            name: &*name,
            object: no_object,
        ];
    }
    // ne' NSNotificationCenter ne' NSAppleEventManager trattengono l'oggetto
    std::mem::forget(handler);
}

/// Collega la finestra: un file che arriva a programma fermo la fa ridisegnare.
pub fn attach(ctx: eframe::egui::Context) {
    let _ = CTX.set(ctx);
}

/// File ricevuti dall'ultima chiamata.
pub fn take() -> Vec<PathBuf> {
    PENDING.lock().map(|mut queue| std::mem::take(&mut *queue)).unwrap_or_default()
}
