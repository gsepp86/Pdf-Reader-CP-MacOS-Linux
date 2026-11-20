//! macOS: il menu dell'applicazione accanto alla mela, in italiano.
//!
//! AppKit crea per conto suo un menu con le voci in inglese ("About …", "Hide …",
//! "Quit …"): qui lo si sostituisce con uno uguale ma tradotto.
#![cfg(target_os = "macos")]

use crate::lang::{english, t};
use objc2::rc::Retained;
use objc2::runtime::Sel;
use objc2::sel;
use objc2_app_kit::{NSApplication, NSMenu, NSMenuItem};
use objc2_foundation::{MainThreadMarker, NSString};

const NOME: &str = "PDF Reader CP";

/// Una voce di menu che manda un selettore standard di AppKit (o nessuno, per i sottomenu).
fn voce(mtm: MainThreadMarker, titolo: &str, azione: Option<Sel>, tasto: &str) -> Retained<NSMenuItem> {
    let titolo = NSString::from_str(titolo);
    let tasto = NSString::from_str(tasto);
    // SAFETY: `azione` e' un selettore standard di AppKit e la firma e' quella richiesta.
    unsafe { NSMenuItem::initWithTitle_action_keyEquivalent(mtm.alloc::<NSMenuItem>(), &titolo, azione, &tasto) }
}

/// Mette al posto del menu di AppKit la versione italiana. Va chiamata quando l'app
/// ha finito di avviarsi, cosi' il menu mostrato e' il nostro.
pub fn install() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let barra = NSMenu::new(mtm);
    let testa = voce(mtm, NOME, None, "");
    let menu = NSMenu::new(mtm);
    let ab = if english() { format!("About {NOME}") } else { format!("Informazioni su {NOME}") };
    let hide = if english() { format!("Hide {NOME}") } else { format!("Nascondi {NOME}") };
    let quit = if english() { format!("Quit {NOME}") } else { format!("Esci da {NOME}") };
    menu.addItem(&voce(mtm, &ab, Some(sel!(orderFrontStandardAboutPanel:)), ""));
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    menu.addItem(&voce(mtm, &hide, Some(sel!(hide:)), "h"));
    menu.addItem(&voce(mtm, t("Nascondi gli altri", "Hide others"), Some(sel!(hideOtherApplications:)), ""));
    menu.addItem(&voce(mtm, t("Mostra tutto", "Show all"), Some(sel!(unhideAllApplications:)), ""));
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    menu.addItem(&voce(mtm, &quit, Some(sel!(terminate:)), "q"));
    testa.setSubmenu(Some(&menu));
    barra.addItem(&testa);
    NSApplication::sharedApplication(mtm).setMainMenu(Some(&barra));
}
