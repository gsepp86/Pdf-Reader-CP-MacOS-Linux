//! Lingua dell'interfaccia: italiano (predefinito) o inglese.
//!
//! La scelta si fa in Impostazioni > Generale e viene salvata in settings.json.
//! Ogni testo mostrato passa da `t("italiano", "english")`: cosi' non serve
//! duplicare le funzioni dell'interfaccia per le due lingue.
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    It,
    En,
}

impl Default for Lang {
    fn default() -> Self {
        Lang::It
    }
}

static EN: AtomicBool = AtomicBool::new(false);

/// Imposta la lingua usata da `t()` (chiamata all'avvio e quando si salva in Impostazioni).
pub fn set(l: Lang) {
    EN.store(l == Lang::En, Ordering::Relaxed);
}

pub fn english() -> bool {
    EN.load(Ordering::Relaxed)
}

/// Testo dell'interfaccia: `it` per l'italiano, `en` per l'inglese.
pub fn t(it: &'static str, en: &'static str) -> &'static str {
    if EN.load(Ordering::Relaxed) {
        en
    } else {
        it
    }
}
