// PDF Reader CP 3.1 - Programma opensource progettato da Peppe CP con l'aiuto della IA.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use eframe::egui::{self, Button, Color32, RichText, Vec2};
use pdfium_render::prelude::*;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf, time::{Duration, Instant}};
use std::io::{BufRead as _, BufReader, Write as _};
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "macos")]
mod mac_open; // PDF aperti dal Finder (Apple Event), vedi il modulo
#[cfg(target_os = "macos")]
mod mac_menu; // menu dell'applicazione (accanto alla mela) in italiano

const ACCENT: Color32 = Color32::from_rgb(198, 47, 47);
const BAR: Color32 = Color32::from_rgb(229, 231, 235);
const BAR_LIT: Color32 = Color32::from_rgb(252, 240, 214); // barra "illuminata" per 2 s dopo un clic
const A4: (f32, f32) = (595.0, 842.0); // punti (1/72")
const ICON: &[u8] = include_bytes!("../assets/icon.png");
const DONATE_URL: &str = "https://paypal.me/PeppeCP";
const GITHUB_URL: &str = "https://github.com/gsepp86/Pdf-Reader-CP";
const NUPS: [u32; 6] = [1, 2, 4, 6, 9, 16]; // pagine per foglio
// nome della libreria PDFium attesa accanto all'eseguibile (usato nel messaggio d'errore)
#[cfg(windows)]
const PDFIUM_LIB: &str = "pdfium.dll";
#[cfg(target_os = "macos")]
const PDFIUM_LIB: &str = "libpdfium.dylib";
#[cfg(all(unix, not(target_os = "macos")))]
const PDFIUM_LIB: &str = "libpdfium.so";

// ---------- impostazioni di stampa (salvate: servono a "Stampa subito") ----------
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq)]
enum PageSel { All, Current, Custom }

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq)]
enum ScaleMode { Fit, Actual, Custom }

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq)]
enum ColorMode {
    Color, // colori come nel PDF
    Auto,  // decide la stampante / il driver (nessuna conversione)
    /// Bianco e nero: in pratica una stampa in scala di grigi (molti driver non offrono
    /// la voce "scala di grigi", quindi la conversione la fa il programma).
    #[serde(alias = "Bw")] // impostazioni salvate dalle versioni precedenti
    Gray,
}

/// Impostazioni di stampa all'avvio: ultime usate oppure quelle scelte dall'utente.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq)]
enum StartMode { Last, Mine }

/// "Le mie impostazioni predefinite" di stampa (stampante, copie, colore e funzioni del driver, es. fronte/retro).
#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
struct PrintPrefs { printer: String, copies: u32, color: ColorMode, bright: i32, contrast: i32, drv_printer: String, drv_mode: String }
impl Default for PrintPrefs {
    fn default() -> Self { Self { printer: String::new(), copies: 1, color: ColorMode::Color, bright: 0, contrast: 0, drv_printer: String::new(), drv_mode: String::new() } }
}
impl PrintPrefs {
    fn from_settings(st: &Settings) -> Self {
        Self { printer: st.printer.clone(), copies: st.copies, color: st.color, bright: st.bright, contrast: st.contrast, drv_printer: st.drv_printer.clone(), drv_mode: st.drv_mode.clone() }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
struct Settings { printer: String, copies: u32, collate: bool, sel: PageSel, custom: String, scale: ScaleMode, pct: u32, rotate: bool, center: bool, nup: u32, border: bool, color: ColorMode, bright: i32, contrast: i32, drv_printer: String, drv_mode: String, background: bool, start_max: bool, start_pan: bool, fit_page: bool, ask_copies: bool, start_mode: StartMode, reset_std: bool, use_driver_defaults: bool, poster: bool, poster_cols: u32, poster_rows: u32, poster_ov: f32, mine: PrintPrefs, tabs_pin: bool, thumbs: bool }
impl Default for Settings {
    fn default() -> Self {
        Self { printer: String::new(), copies: 1, collate: true, sel: PageSel::All, custom: "1-".into(), scale: ScaleMode::Fit, pct: 100, rotate: true, center: true, nup: 1, border: true, color: ColorMode::Color, bright: 0, contrast: 0, drv_printer: String::new(), drv_mode: String::new(), background: false, start_max: true, start_pan: false, fit_page: false, ask_copies: true, start_mode: StartMode::Last, reset_std: true, use_driver_defaults: true, poster: false, poster_cols: 2, poster_rows: 2, poster_ov: 5.0, mine: PrintPrefs::default(), tabs_pin: false, thumbs: false }
    }
}
impl Settings {
    /// Politica di avvio per la stampa: ultime impostazioni o "le mie predefinite"; poi ripristino della stampa standard
    /// (1 su 1, tutte le pagine, scala adatta) e delle impostazioni predefinite del driver (es. fronte/retro).
    fn apply_startup(&mut self, printers: &[String]) {
        if self.start_mode == StartMode::Mine {
            let m = self.mine.clone();
            if !m.printer.is_empty() && printers.contains(&m.printer) { self.printer = m.printer; }
            self.copies = m.copies.max(1);
            self.color = m.color;
            self.bright = m.bright;
            self.contrast = m.contrast;
            self.drv_printer = m.drv_printer;
            self.drv_mode = m.drv_mode;
        } else {
            if self.reset_std { self.copies = 1; }
            if self.use_driver_defaults { self.drv_printer.clear(); self.drv_mode.clear(); } // si usano le impostazioni predefinite del driver
        }
        if self.reset_std {
            self.nup = 1; self.sel = PageSel::All; self.custom = "1-".into();
            self.scale = ScaleMode::Fit; self.pct = 100; self.collate = true;
        }
    }

    /// Griglia della stampa poster (colonne x righe di fogli in cui si divide la pagina).
    fn poster_grid(&self) -> (usize, usize) {
        (self.poster_cols.clamp(1, 6) as usize, self.poster_rows.clamp(1, 6) as usize)
    }

    /// Fogli in cui viene divisa ogni pagina (1 se la stampa poster e' spenta).
    fn poster_tiles(&self) -> usize {
        if self.poster { let (c, r) = self.poster_grid(); c * r } else { 1 }
    }

    fn path() -> Option<PathBuf> { dirs::config_dir().map(|d| d.join("PDFReaderCP").join("settings.json")) }
    fn load() -> Self {
        Self::path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }
    fn save(&self) {
        if let Some(p) = Self::path() {
            let _ = std::fs::create_dir_all(p.parent().unwrap());
            let _ = std::fs::write(p, serde_json::to_string_pretty(self).unwrap());
        }
    }
}

fn pages_for(st: &Settings, cur: usize, n: usize) -> Vec<usize> {
    match st.sel {
        PageSel::All => (0..n).collect(),
        PageSel::Current => vec![cur],
        PageSel::Custom => {
            // es. "1-3, 5"  |  "1,1" = pagina 1 due volte  |  "2x3" = pagina 2 per 3 volte (anche "1-3x2": ogni pagina due volte)
            let mut v = vec![];
            for part in st.custom.split(',') {
                let part = part.trim().to_lowercase();
                let (spec, rep) = match part.split_once('x') {
                    Some((a, k)) => (a.trim().to_string(), k.trim().parse::<usize>().unwrap_or(1).clamp(1, 99)),
                    None => (part.clone(), 1),
                };
                let (a, b) = match spec.split_once('-') {
                    Some((a, b)) => (a.trim().parse::<usize>().unwrap_or(1), if b.trim().is_empty() { n } else { b.trim().parse::<usize>().unwrap_or(n) }),
                    None => { let x = spec.trim().parse::<usize>().unwrap_or(0); (x, x) }
                };
                for p in a.max(1)..=b.min(n) { for _ in 0..rep { v.push(p - 1); } }
            }
            v
        }
    }
}

/// Fogli prodotti da `pages` pagine, senza contare le copie.
fn sheet_count(st: &Settings, pages: usize) -> usize {
    let nup = st.nup.max(1) as usize;
    ((pages + nup - 1) / nup) * st.poster_tiles()
}

/// Pagine di un file secondo il testo scritto nella finestra di STAMPA TUTTO (vuoto = tutte).
fn spec_pages(spec: &str, n: usize, cur: usize) -> Vec<usize> {
    if spec.trim().is_empty() { return (0..n).collect(); }
    let mut st = Settings::default();
    st.sel = PageSel::Custom;
    st.custom = spec.to_string();
    pages_for(&st, cur, n)
}

fn short(s: &str, n: usize) -> String {
    if s.chars().count() > n { format!("{}…", s.chars().take(n - 1).collect::<String>()) } else { s.to_string() }
}

// ---------- rendering ----------
fn rot90(w: usize, h: usize, d: &[u8]) -> (usize, usize, Vec<u8>) {
    let mut o = vec![0u8; d.len()];
    for y in 0..h { for x in 0..w {
        let (s, t) = ((y * w + x) * 4, (x * h + (h - 1 - y)) * 4);
        o[t..t + 4].copy_from_slice(&d[s..s + 4]);
    } }
    (h, w, o)
}

/// true se i byte grezzi restituiti da PDFium sono BGRA (da scambiare in RGBA). Impostato all'avvio da detect_byte_order().
static BGRA_RAW: AtomicBool = AtomicBool::new(false);

/// Rileva l'ordine dei byte di PDFium disegnando un minuscolo PDF tutto rosso: cosi' i colori sono giusti con qualsiasi versione della libreria.
fn detect_byte_order(pdfium: &Pdfium) {
    const PROBE: &[u8] = b"%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 20 20]/Contents 4 0 R>>endobj\n4 0 obj<</Length 24>>stream\n1 0 0 rg 0 0 20 20 re f\nendstream\nendobj\ntrailer<</Root 1 0 R>>\n";
    let Ok(doc) = pdfium.load_pdf_from_byte_vec(PROBE.to_vec(), None) else { return };
    let Ok(page) = doc.pages().get(0) else { return };
    let Ok(bmp) = page.render_with_config(&PdfRenderConfig::new().set_target_width(20)) else { return };
    let raw = bmp.as_raw_bytes();
    let (w, h) = (bmp.width() as usize, bmp.height() as usize);
    let i = ((h / 2) * w + w / 2) * 4;
    if raw.len() < i + 4 { return; }
    // rosso puro: in RGBA e' [255,0,0], in BGRA e' [0,0,255]
    BGRA_RAW.store(raw[i] < 128 && raw[i + 2] > 128, Ordering::Relaxed);
}

/// Rende una pagina in RGBA con la larghezza indicata (px).
fn render(doc: &PdfDocument, page: usize, width: i32) -> Option<(usize, usize, Vec<u8>)> {
    let p = doc.pages().get(page as u16).ok()?;
    let bmp = p.render_with_config(&PdfRenderConfig::new().set_target_width(width)).ok()?;
    let (w, h) = (bmp.width() as usize, bmp.height() as usize);
    let mut d = bmp.as_raw_bytes().to_vec();
    let bgra = BGRA_RAW.load(Ordering::Relaxed);
    for c in d.chunks_exact_mut(4) { if bgra { c.swap(0, 2); } c[3] = 255; }
    Some((w, h, d))
}

fn blit(dst: &mut [u8], dw: usize, dh: usize, src: &[u8], sw: usize, sh: usize, x: i32, y: i32) {
    for r in 0..sh {
        let ty = y + r as i32;
        if ty < 0 || ty >= dh as i32 { continue; }
        let x0 = x.max(0) as usize;
        let x1 = (x + sw as i32).min(dw as i32).max(0) as usize;
        if x1 <= x0 { continue; }
        let (sx, len) = ((x0 as i32 - x) as usize, x1 - x0);
        let (d, s) = ((ty as usize * dw + x0) * 4, (r * sw + sx) * 4);
        dst[d..d + len * 4].copy_from_slice(&src[s..s + len * 4]);
    }
}

fn outline(dst: &mut [u8], dw: usize, dh: usize, x: i32, y: i32, w: usize, h: usize) {
    let mut px = |xx: i32, yy: i32| {
        if xx >= 0 && yy >= 0 && (xx as usize) < dw && (yy as usize) < dh {
            let i = (yy as usize * dw + xx as usize) * 4;
            dst[i..i + 3].copy_from_slice(&[140, 140, 140]);
        }
    };
    for i in 0..w as i32 { px(x + i, y); px(x + i, y + h as i32 - 1); }
    for j in 0..h as i32 { px(x, y + j); px(x + w as i32 - 1, y + j); }
}

/// Compone UN foglio (1, 2, 4, 6, 9 o 16 pagine) pescando le pagine anche da file diversi:
/// `items` = (indice del documento in `docs`, pagina). Serve alla stampa a gruppo, dove la
/// pagina 1 di un file e la pagina 3 di un altro finiscono sullo stesso foglio (es. 2 su 1).
fn compose_group(docs: &[Doc], items: &[(usize, usize)], pap: (f32, f32), st: &Settings, ppp: f32, apply_tone: bool) -> Option<(usize, usize, Vec<u8>)> {
    let n = st.nup.max(1) as usize;
    let (fd, fp) = *items.first()?;
    let (pw0, ph0) = *docs.get(fd)?.sizes.get(fp)?;
    let land = pw0 > ph0;
    let pg = match n { 2 => (1, 2), 4 => (2, 2), 6 => (2, 3), 9 => (3, 3), 16 => (4, 4), _ => (1, 1) }; // colonne x righe su foglio verticale
    // 2 su 1 / 6 su 1 con pagine verticali: foglio orizzontale (pagine affiancate)
    let sheet_land = st.rotate && if n == 1 { land } else if pg.0 != pg.1 { !land } else { land };
    let (cols, rows) = if sheet_land { (pg.1, pg.0) } else { pg };
    let (mx, mn) = (pap.0.max(pap.1), pap.0.min(pap.1));
    let (sw, sh) = if sheet_land { (mx, mn) } else { (mn, mx) };
    let rot = sheet_land != (pap.0 > pap.1);
    let (bw, bh) = (((sw * ppp) as usize).max(1), ((sh * ppp) as usize).max(1));
    let mut buf = vec![255u8; bw * bh * 4];
    let (cw, ch) = (sw / cols as f32, sh / rows as f32);
    let m = if n > 1 { 6.0 } else { 0.0 };
    for (i, &(di, p)) in items.iter().enumerate() {
        let d = docs.get(di)?;
        let (pw, ph) = *d.sizes.get(p)?;
        let (aw, ah) = (cw - 2.0 * m, ch - 2.0 * m);
        let fit = (aw / pw).min(ah / ph);
        let s = if n > 1 { fit } else { match st.scale { ScaleMode::Fit => fit, ScaleMode::Actual => 1.0, ScaleMode::Custom => st.pct.clamp(25, 200) as f32 / 100.0 } };
        let (w, h) = (pw * s, ph * s);
        let (cx, cy) = ((i % cols) as f32 * cw + m, (i / cols) as f32 * ch + m);
        let (x, y) = if st.center || n > 1 { (cx + (aw - w) / 2.0, cy + (ah - h) / 2.0) } else { (cx, cy) };
        let (iw, ih, data) = render(&d.pdf, p, ((w * ppp) as i32).max(8))?;
        let (xi, yi) = ((x * ppp) as i32, (y * ppp) as i32);
        blit(&mut buf, bw, bh, &data, iw, ih, xi, yi);
        if n > 1 && st.border {
            for t in 0..((ppp / 2.0).max(1.0) as i32) {
                outline(&mut buf, bw, bh, xi + t, yi + t, (iw as i32 - 2 * t).max(1) as usize, (ih as i32 - 2 * t).max(1) as usize);
            }
        }
    }
    if apply_tone { tone(&mut buf, st); }
    Some(if rot { rot90(bw, bh, &buf) } else { (bw, bh, buf) })
}

/// Compone UN foglio da un solo documento (stampa normale e anteprima).
fn compose(_pdfium: &Pdfium, d: &Doc, pages: &[usize], pap: (f32, f32), st: &Settings, ppp: f32, apply_tone: bool) -> Option<(usize, usize, Vec<u8>)> {
    let items: Vec<(usize, usize)> = pages.iter().map(|&p| (0usize, p)).collect();
    compose_group(std::slice::from_ref(d), &items, pap, st, ppp, apply_tone)
}

/// Rettangolo pieno: segni di taglio e numerazione dei fogli del poster.
fn mark_rect(dst: &mut [u8], dw: usize, dh: usize, x: i32, y: i32, w: i32, h: i32, c: [u8; 3]) {
    for yy in y..y + h {
        for xx in x..x + w {
            if xx >= 0 && yy >= 0 && (xx as usize) < dw && (yy as usize) < dh {
                let i = (yy as usize * dw + xx as usize) * 4;
                dst[i..i + 3].copy_from_slice(&c);
            }
        }
    }
}

/// Glifo 3x5 per la numerazione dei fogli (solo i caratteri usati: R, C e cifre).
fn mark_glyph(ch: char) -> [u8; 5] {
    match ch {
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        '3' => [0b111, 0b001, 0b111, 0b001, 0b111],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        '6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b001, 0b001, 0b001],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        'R' => [0b110, 0b101, 0b110, 0b101, 0b101],
        'C' => [0b011, 0b100, 0b100, 0b100, 0b011],
        _ => [0; 5],
    }
}

/// Testo minuscolo (glifi 3x5) per la numerazione dei fogli del poster.
fn mark_text(dst: &mut [u8], dw: usize, dh: usize, x: i32, y: i32, s: &str, px: i32, c: [u8; 3]) {
    let mut cx = x;
    for ch in s.chars() {
        let g = mark_glyph(ch);
        for (r, bits) in g.iter().enumerate() {
            for b in 0..3 {
                if bits & (1 << (2 - b)) != 0 {
                    mark_rect(dst, dw, dh, cx + b * px, y + r as i32 * px, px, px, c);
                }
            }
        }
        cx += 4 * px;
    }
}

/// Segni di taglio e numero del foglio (R<riga>C<colonna>) nella striscia di sovrapposizione:
/// cadono nella parte che viene tagliata o coperta dal foglio vicino, quindi non nascondono il disegno.
fn poster_marks(buf: &mut [u8], bw: usize, bh: usize, eff: f32, ov_pt: f32, cols: usize, rows: usize, col: usize, row: usize) {
    if ov_pt <= 0.3 { return; }
    let band = (ov_pt * eff) as i32;
    if band < 2 { return; }
    let c = [90u8, 90, 90];
    let w = ((1.5 * eff) as i32).max(1);
    let tick = ((4.0 * 72.0 / 25.4 * eff) as i32).min(band).max(2);
    let (bwi, bhi) = (bw as i32, bh as i32);
    // tacche perpendicolari alla linea di taglio, dentro la striscia che viene tolta
    if col > 0 { // linea di taglio verticale sinistra
        mark_rect(buf, bw, bh, band - tick, 0, tick, w, c);
        mark_rect(buf, bw, bh, band - tick, bhi - w, tick, w, c);
    }
    if col + 1 < cols { // linea di taglio verticale destra
        mark_rect(buf, bw, bh, bwi - band, 0, tick, w, c);
        mark_rect(buf, bw, bh, bwi - band, bhi - w, tick, w, c);
    }
    if row > 0 { // linea di taglio orizzontale superiore
        mark_rect(buf, bw, bh, 0, band - tick, w, tick, c);
        mark_rect(buf, bw, bh, bwi - w, band - tick, w, tick, c);
    }
    if row + 1 < rows { // linea di taglio orizzontale inferiore
        mark_rect(buf, bw, bh, 0, bhi - band, w, tick, c);
        mark_rect(buf, bw, bh, bwi - w, bhi - band, w, tick, c);
    }
    let s = band / 8; // altezza del testo: circa il 60% della striscia
    if s < 1 { return; }
    let label = format!("R{}C{}", row + 1, col + 1);
    let tw = label.chars().count() as i32 * 4 * s - s;
    let th = 5 * s;
    let pad = (2.0 * 72.0 / 25.4 * eff) as i32 + 2;
    let (tx, ty) = if row + 1 < rows { (pad, bhi - band + (band - th) / 2) }
        else if row > 0 { (pad, (band - th) / 2) }
        else if col + 1 < cols { (bwi - band + (band - tw).max(0) / 2, (band - th) / 2) }
        else if col > 0 { ((band - tw).max(0) / 2, (band - th) / 2) }
        else { return };
    mark_text(buf, bw, bh, tx, ty, &label, s, c);
}

/// Compone UN foglio della stampa poster: la pagina viene ingrandita sull'intero poster
/// (cols x rows fogli) e ogni foglio ne stampa la porzione `(col, row)`. La sovrapposizione
/// `poster_ov` (mm) viene ripetuta sui lati interni, per unire i fogli senza tagliare il disegno.
fn compose_poster(d: &Doc, page: usize, pap: (f32, f32), st: &Settings, ppp: f32, apply_tone: bool, col: usize, row: usize) -> Option<(usize, usize, Vec<u8>)> {
    let (pw, ph) = d.sizes[page];
    if pw <= 0.0 || ph <= 0.0 { return None; }
    let (cols, rows) = st.poster_grid();
    if col >= cols || row >= rows { return None; }
    let sheet_land = st.rotate && pw > ph;
    let (mx, mn) = (pap.0.max(pap.1), pap.0.min(pap.1));
    let (sw, sh) = if sheet_land { (mx, mn) } else { (mn, mx) };
    let rot = sheet_land != (pap.0 > pap.1);
    let (cw, ch) = (sw * cols as f32, sh * rows as f32); // tela del poster intero (punti)
    let fit = (cw / pw).min(ch / ph);
    let (iwp, ihp) = (pw * fit, ph * fit);
    let (ox, oy) = ((cw - iwp) / 2.0, (ch - ihp) / 2.0); // pagina centrata sulla tela
    let ov = st.poster_ov.clamp(0.0, 30.0) * 72.0 / 25.4; // mm -> punti
    let x0 = (col as f32 * sw - ov).max(0.0);
    let x1 = ((col as f32 + 1.0) * sw + ov).min(cw);
    let y0 = (row as f32 * sh - ov).max(0.0);
    let y1 = ((row as f32 + 1.0) * sh + ov).min(ch);
    if x1 <= x0 || y1 <= y0 { return None; }
    // la pagina intera si rende a `ppp` con un tetto di pixel: i poster grandi restano
    // stampabili senza occupare memoria sproporzionata (i fogli vengono ritagliati da qui)
    let cap = (24_000_000.0f32 / (iwp * ppp * ihp * ppp).max(1.0)).sqrt().min(1.0);
    let (rw, rh, data) = render(&d.pdf, page, ((iwp * ppp * cap) as i32).max(8))?;
    let eff = rw as f32 / pw; // pixel per punto effettivi
    let bw = (((x1 - x0) * eff) as usize).max(1);
    let bh = (((y1 - y0) * eff) as usize).max(1);
    let mut buf = vec![255u8; bw * bh * 4];
    blit(&mut buf, bw, bh, &data, rw, rh, ((ox - x0) * eff).round() as i32, ((oy - y0) * eff).round() as i32);
    if apply_tone { tone(&mut buf, st); }
    poster_marks(&mut buf, bw, bh, eff, ov, cols, rows, col, row);
    Some(if rot { rot90(bw, bh, &buf) } else { (bw, bh, buf) })
}

/// Legge i caratteri (con posizione) di una pagina dal livello di testo del PDF.
fn load_chars(doc: &PdfDocument, page: usize) -> Vec<TCh> {
    let mut v = vec![];
    let Ok(p) = doc.pages().get(page as u16) else { return v };
    let Ok(t) = p.text() else { return v };
    for ch in t.chars().iter() {
        let c = ch.unicode_char().unwrap_or('\0');
        let r = ch.loose_bounds().map(|b| [b.left().value, b.bottom().value, b.right().value, b.top().value]).unwrap_or([0.0; 4]);
        v.push(TCh { c, r });
    }
    v
}

/// Carattere piu' vicino al punto (x, y) in coordinate pagina PDF.
fn hit(chars: &[TCh], x: f32, y: f32) -> Option<usize> {
    let mut best: Option<(usize, f32)> = None;
    for (i, c) in chars.iter().enumerate() {
        let [l, b, r, t] = c.r;
        if r - l < 0.01 || t - b < 0.01 { continue; }
        let dx = if x < l { l - x } else if x > r { x - r } else { 0.0 };
        let dy = if y < b { b - y } else if y > t { y - t } else { 0.0 };
        let d = dx * dx + dy * dy * 4.0; // privilegia la stessa riga
        if best.map_or(true, |(_, bd)| d < bd) { best = Some((i, d)); }
        if d == 0.0 { break; }
    }
    best.map(|(i, _)| i)
}

/// Colore / automatico / bianco e nero (in pratica scala di grigi), con chiaro-scuro e contrasto:
/// il bianco della carta resta bianco.
fn tone(buf: &mut [u8], st: &Settings) {
    let mono = st.color == ColorMode::Gray;
    if !mono && st.bright == 0 && st.contrast == 0 { return; }
    let g = 2f32.powf(-(st.bright as f32) / 50.0); // >0 = piu' chiaro, <0 = piu' scuro
    let k = 1.0 + st.contrast as f32 / 100.0; // -100 = grigio piatto, +100 = contrasto doppio
    let lut: Vec<u8> = (0..256).map(|v| {
        let mut x = (v as f32 / 255.0).powf(g);
        if st.contrast != 0 { x = ((x - 0.5) * k + 0.5).clamp(0.0, 1.0); } // contrasto intorno al grigio medio
        (255.0 * x).round().clamp(0.0, 255.0) as u8
    }).collect();
    for p in buf.chunks_exact_mut(4) {
        if mono {
            let lum = ((0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32) as usize).min(255);
            let l = lut[lum];
            p[0] = l; p[1] = l; p[2] = l;
        } else {
            p[0] = lut[p[0] as usize]; p[1] = lut[p[1] as usize]; p[2] = lut[p[2] as usize];
        }
    }
}

/// Posizioni (indice di carattere) di tutte le occorrenze di `q` in `t`.
fn find_all(t: &[char], q: &[char]) -> Vec<usize> {
    let mut v = vec![];
    if q.is_empty() || t.len() < q.len() { return v; }
    for i in 0..=t.len() - q.len() { if t[i..i + q.len()] == *q { v.push(i); } }
    v
}

fn to_hex(b: &[u8]) -> String { b.iter().map(|x| format!("{x:02x}")).collect() }
fn from_hex(s: &str) -> Vec<u8> { (0..s.len() / 2).filter_map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).ok()).collect() }

/// Ultima release pubblicata su GitHub: (messaggio per l'utente, pagina da aprire).
/// Usa curl (presente su Windows 10+, macOS e Linux) in un thread separato: l'interfaccia non si blocca.
fn latest_release() -> (String, String) {
    let url = "https://api.github.com/repos/gsepp86/Pdf-Reader-CP-MacOS-Linux/releases/latest";
    let out = std::process::Command::new("curl")
        .args(["-sS", "-L", "-m", "12", "-H", "Accept: application/vnd.github+json", url])
        .output();
    let Ok(out) = out else { return ("Controllo non riuscito: curl non disponibile".into(), String::new()) };
    if !out.status.success() { return ("Controllo non riuscito: nessuna connessione".into(), String::new()) }
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&out.stdout) else {
        return ("Risposta di GitHub non leggibile".into(), String::new())
    };
    let tag = v.get("tag_name").and_then(|t| t.as_str()).unwrap_or("").trim_start_matches('v').to_string();
    let page = v.get("html_url").and_then(|t| t.as_str()).unwrap_or(GITHUB_URL).to_string();
    if tag.is_empty() {
        let why = v.get("message").and_then(|t| t.as_str()).unwrap_or("");
        let m = if why.contains("rate limit") { "Controllo non riuscito: GitHub ha limitato le richieste, riprova piu' tardi" }
                else { "Nessuna release pubblicata su GitHub" };
        return (m.into(), String::new())
    }
    let cur = app_version();
    if version_newer(&tag, cur) { (format!("Nuova versione disponibile: {tag} (questa e' {cur})"), page) }
    else { (format!("Hai gia' la versione piu' recente ({cur})"), page) }
}

/// Versione mostrata all'utente (3.1): Cargo nel manifest vuole anche il numero di patch,
/// ma qui si toglie lo zero finale.
fn app_version() -> &'static str {
    let v: &'static str = env!("CARGO_PKG_VERSION");
    v.strip_suffix(".0").unwrap_or(v)
}

fn version_newer(a: &str, b: &str) -> bool {
    fn t(v: &str) -> (u32, u32, u32) {
        let mut n = v.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).map(|s| s.parse::<u32>().unwrap_or(0));
        (n.next().unwrap_or(0), n.next().unwrap_or(0), n.next().unwrap_or(0))
    }
    t(a) > t(b)
}

fn confirm(title: &str, text: &str) -> bool {
    rfd::MessageDialog::new().set_title(title).set_description(text).set_buttons(rfd::MessageButtons::YesNo).set_level(rfd::MessageLevel::Warning).show() == rfd::MessageDialogResult::Yes
}

fn err(msg: &str) {
    rfd::MessageDialog::new().set_title("PDF Reader CP").set_description(msg).set_level(rfd::MessageLevel::Error).show();
}

// ---------- spooler Windows (GDI) ----------
#[cfg(windows)]
mod spool {
    use windows::{core::{w, PCWSTR}, Win32::Foundation::HANDLE, Win32::Graphics::Gdi::*, Win32::Graphics::Printing::{ClosePrinter, DocumentPropertiesW, OpenPrinterW}, Win32::Storage::Xps::*, Win32::UI::WindowsAndMessaging::GetForegroundWindow};
    pub struct Job { dc: HDC, aborted: bool }

    /// Crea il contesto di stampa, applicando (se presenti) le impostazioni del driver (DEVMODE).
    unsafe fn make_dc(printer: &str, devmode: Option<&[u8]>) -> HDC {
        let p: Vec<u16> = printer.encode_utf16().chain([0]).collect();
        let mut al: Vec<u64> = Vec::new(); // copia allineata del DEVMODE
        let dm: Option<*const DEVMODEW> = match devmode {
            Some(d) if d.len() >= std::mem::size_of::<DEVMODEW>() => {
                al = vec![0u64; (d.len() + 7) / 8];
                std::ptr::copy_nonoverlapping(d.as_ptr(), al.as_mut_ptr() as *mut u8, d.len());
                Some(al.as_ptr() as *const DEVMODEW)
            }
            _ => None,
        };
        let dc = CreateDCW(w!("WINSPOOL"), PCWSTR(p.as_ptr()), PCWSTR::null(), dm);
        drop(al);
        dc
    }

    /// Finestra "Proprieta'" del driver (fronte/retro, vassoio, qualita', colore...). Some(DEVMODE) se l'utente preme OK.
    pub fn driver_dialog(printer: &str, current: Option<&[u8]>) -> Option<Vec<u8>> {
        unsafe {
            let name: Vec<u16> = printer.encode_utf16().chain([0]).collect();
            let pn = PCWSTR(name.as_ptr());
            let mut h = HANDLE::default();
            OpenPrinterW(pn, &mut h, None).ok()?;
            let hwnd = GetForegroundWindow();
            let size = DocumentPropertiesW(hwnd, h, pn, None, None, 0);
            let mut out = None;
            if size > 0 {
                let mut buf = vec![0u64; (size as usize + 7) / 8];
                let dm = buf.as_mut_ptr() as *mut DEVMODEW;
                let mut mode = 4u32 | 2u32; // DM_IN_PROMPT | DM_OUT_BUFFER
                let mut input: Option<*const DEVMODEW> = None;
                if let Some(c) = current {
                    if c.len() == size as usize {
                        std::ptr::copy_nonoverlapping(c.as_ptr(), buf.as_mut_ptr() as *mut u8, c.len());
                        mode |= 8; // DM_IN_BUFFER: riparte dalle impostazioni attuali
                        input = Some(dm as *const DEVMODEW);
                    }
                }
                if DocumentPropertiesW(hwnd, h, pn, Some(dm), input, mode) == 1 { // IDOK
                    out = Some(std::slice::from_raw_parts(buf.as_ptr() as *const u8, size as usize).to_vec());
                }
            }
            let _ = ClosePrinter(h);
            out
        }
    }

    /// Dimensione fisica della carta (punti) con le impostazioni correnti del driver.
    pub fn paper_pts(printer: &str, devmode: Option<&[u8]>) -> Option<(f32, f32)> {
        unsafe {
            let dc = make_dc(printer, devmode);
            if dc.is_invalid() { return None; }
            let (dx, dy) = (GetDeviceCaps(dc, LOGPIXELSX) as f32, GetDeviceCaps(dc, LOGPIXELSY) as f32);
            let r = (GetDeviceCaps(dc, PHYSICALWIDTH) as f32 / dx * 72.0, GetDeviceCaps(dc, PHYSICALHEIGHT) as f32 / dy * 72.0);
            let _ = DeleteDC(dc);
            if r.0 > 50.0 && r.1 > 50.0 { Some(r) } else { None }
        }
    }

    /// Preferenze predefinite del driver (DEVMODE senza dialogo): colore e copie.
    #[derive(Default)]
    pub struct Defaults {
        /// Some(true) = bianco e nero, Some(false) = colori.
        pub color: Option<bool>,
        pub copies: Option<u32>,
        pub nup: Option<u32>,
    }

    pub fn defaults(printer: &str) -> Defaults {
        let mut d = Defaults::default();
        unsafe {
            let name: Vec<u16> = printer.encode_utf16().chain([0]).collect();
            let pn = PCWSTR(name.as_ptr());
            let mut h = HANDLE::default();
            if OpenPrinterW(pn, &mut h, None).is_err() { return d; }
            let hwnd = GetForegroundWindow();
            let size = DocumentPropertiesW(hwnd, h, pn, None, None, 0);
            if size > 0 {
                let mut buf = vec![0u64; (size as usize + 7) / 8];
                let dm = buf.as_mut_ptr() as *mut DEVMODEW;
                // DM_OUT_BUFFER (2): riempie il buffer con le impostazioni predefinite del driver
                if DocumentPropertiesW(hwnd, h, pn, Some(dm), None, 2) == 1 {
                    let d0 = &*dm;
                    if d0.dmFields & 0x0000_0800 != 0 { d.color = Some(d0.dmColor == 1); } // DM_COLOR / DMCOLOR_MONOCHROME
                    if d0.dmFields & 0x0000_1000 != 0 && d0.dmCopies > 0 { d.copies = Some(d0.dmCopies as u32); } // DM_COPIES
                }
            }
            let _ = ClosePrinter(h);
        }
        d
    }

    impl Job {
        pub fn new(printer: &str, name: &str, devmode: Option<&[u8]>) -> Result<Job, String> {
            unsafe {
                let dc = make_dc(printer, devmode);
                if dc.is_invalid() { return Err(format!("Impossibile aprire la stampante «{printer}»")); }
                let n: Vec<u16> = name.encode_utf16().chain([0]).collect();
                let di = DOCINFOW { cbSize: std::mem::size_of::<DOCINFOW>() as i32, lpszDocName: PCWSTR(n.as_ptr()), ..Default::default() };
                if StartDocW(dc, &di) <= 0 { let _ = DeleteDC(dc); return Err("Avvio del lavoro di stampa non riuscito".into()); }
                Ok(Job { dc, aborted: false })
            }
        }
        /// Area stampabile in punti e risoluzione (dpi).
        pub fn area(&self) -> (f32, f32, f32) {
            unsafe {
                let d = GetDeviceCaps(self.dc, LOGPIXELSX) as f32;
                (GetDeviceCaps(self.dc, HORZRES) as f32 / d * 72.0, GetDeviceCaps(self.dc, VERTRES) as f32 / d * 72.0, d)
            }
        }
        /// Annulla il lavoro: non viene mandato alla stampante.
        pub fn abort(&mut self) { if !self.aborted { self.aborted = true; unsafe { AbortDoc(self.dc); } } }
        pub fn page(&self, bgra: &[u8], w: i32, h: i32, x: i32, y: i32, dw: i32, dh: i32) {
            unsafe {
                StartPage(self.dc);
                let mut bi = BITMAPINFO::default();
                bi.bmiHeader = BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, biCompression: 0, ..Default::default() };
                StretchDIBits(self.dc, x, y, dw, dh, 0, 0, w, h, Some(bgra.as_ptr() as *const _), &bi, DIB_RGB_COLORS, SRCCOPY);
                EndPage(self.dc);
            }
        }
    }
    impl Drop for Job { fn drop(&mut self) { unsafe { if !self.aborted { EndDoc(self.dc); } let _ = DeleteDC(self.dc); } } }
}
#[cfg(not(windows))]
#[path = "spool_unix.rs"]
mod spool;

// ---------- applicazione ----------
struct Doc { id: u64, pdf: PdfDocument<'static>, path: PathBuf, name: String, sizes: Vec<(f32, f32)>, cur: usize, zoom: f32, fitpage: bool, tex: HashMap<(usize, i32), egui::TextureHandle>, text: HashMap<usize, Vec<TCh>>, ptext: HashMap<usize, Vec<char>>, thumbs: HashMap<usize, egui::TextureHandle> }
#[derive(PartialEq, Clone, Copy)]
enum Tab { Home, Doc(usize) }
type PvKey = (u64, ColorMode, i32, i32); // versione del foglio + colore + chiaro/scuro + contrasto
type RawKey = (usize, usize, PageSel, String, u32, bool, ScaleMode, u32, bool, bool, (i32, i32), (bool, (u32, u32), f32));
/// Carattere del testo di una pagina: unicode + riquadro [sinistra, basso, destra, alto] in punti PDF.
struct TCh { c: char, r: [f32; 4] }
/// Selezione di testo: documento, pagina, indice del primo e dell'ultimo carattere.
struct Sel { doc: usize, page: usize, a: usize, b: usize }

/// Richiesta della password di un PDF protetto: la finestra compare solo quando il file non si apre senza.
struct PwdAsk { path: PathBuf, text: String, wrong: bool, focus: bool, queue: Vec<PathBuf> }

/// Lavoro di stampa in corso: procede un passo (un foglio) per volta cosi' l'interfaccia resta viva e mostra il cerchio di caricamento.
struct PrintTask {
    ids: Vec<u64>, st: Settings, di: usize, job: Option<spool::Job>,
    sheets: Vec<Vec<usize>>, order: Vec<usize>, pos: usize, area: (f32, f32, f32),
    last: Option<(usize, usize, usize, Vec<u8>)>, total: usize, done: usize, cancel: bool,
}

/// Stampa a gruppo: le pagine scelte nei vari file diventano UN solo lavoro, impaginate insieme
/// (la pagina 1 di un file e la pagina 3 di un altro possono stare sullo stesso foglio 2 su 1).
struct GroupJob {
    st: Settings, sheets: Vec<Vec<(u64, usize)>>, order: Vec<usize>, pos: usize, job: Option<spool::Job>,
    area: (f32, f32, f32), last: Option<(usize, usize, usize, Vec<u8>)>, total: usize, done: usize, cancel: bool,
}

struct App {
    pdfium: &'static Pdfium, docs: Vec<Doc>, tab: Tab,
    st: Settings, draft: Settings, printers: Vec<String>,
    show_print: bool, prev: usize, pv: Option<(PvKey, egui::TextureHandle)>,
    msg: String, msg_at: Option<Instant>, logo: egui::TextureHandle, fs: bool, show_info: bool, scroll_tab: bool,
    ipc: Option<std::sync::mpsc::Receiver<Ipc>>, started: bool, sel: Option<Sel>, paper: (f32, f32), paper_key: (String, String),
    upd: Option<std::sync::mpsc::Receiver<(String, String)>>, upd_msg: String, upd_url: String,
    pwd: Option<PwdAsk>,
    pv_raw: Option<(RawKey, usize, usize, Vec<u8>)>, pv_gen: u64, last_size: (i32, i32), last_change: Instant,
    pan: bool, search: String, search_hit: Option<(usize, usize)>, search_focus: bool, jump: bool,
    show_cfg: bool, cfg: Settings, cfg_tab: usize,
    show_pall: bool, pa_mode: bool, pall_spec: Vec<(u64, String)>, drv_seen: String, gjob: Option<GroupJob>,
    over_acc: f32, over_at: Instant, edge: (bool, bool), scroll_reset: Option<bool>, tabs_overflow: bool,
    task: Option<PrintTask>, tab_drag: Option<usize>, next_id: u64,
    bar_hide: Instant, bar_lit: Instant, // barra a scomparsa su macOS: quando ritirarla e quando e' evidenziata
}

impl App {
    fn new(cc: &eframe::CreationContext, listener: Option<std::net::TcpListener>, init: Vec<PathBuf>) -> Self {
        let exe = std::env::current_exe().ok().and_then(|p| p.parent().map(|p| p.to_path_buf())).unwrap_or_default();
        let lib = Pdfium::pdfium_platform_library_name_at_path(&exe);
        let bind = match Pdfium::bind_to_library(lib).or_else(|_| Pdfium::bind_to_system_library()) {
            Ok(b) => b,
            Err(_) => { err(&format!("Libreria {PDFIUM_LIB} non trovata accanto al programma. Reinstalla il programma.")); std::process::exit(1) }
        };
        let mut v = egui::Visuals::light();
        v.selection.bg_fill = ACCENT;
        cc.egui_ctx.set_visuals(v);
        let ic = eframe::icon_data::from_png_bytes(ICON).expect("icona");
        let logo = cc.egui_ctx.load_texture("logo", egui::ColorImage::from_rgba_unmultiplied([ic.width as usize, ic.height as usize], &ic.rgba), Default::default());
        let mut printers: Vec<String> = printers::get_printers().into_iter().map(|p| p.name).collect();
        printers.sort_by_key(|p| p.to_lowercase()); // ordine alfabetico
        printers.dedup();
        let mut st = Settings::load();
        st.apply_startup(&printers);
        if !printers.contains(&st.printer) {
            st.printer = printers::get_default_printer().map(|p| p.name).or_else(|| printers.first().cloned()).unwrap_or_default();
        }
        let pdfium: &'static Pdfium = Box::leak(Box::new(Pdfium::new(bind))); // vive per tutto il programma: i PDF aperti restano in memoria
        detect_byte_order(pdfium);
        let start_pan = st.start_pan;
        let ipc = listener.map(|l| { let (tx, rx) = std::sync::mpsc::channel(); serve(l, tx, cc.egui_ctx.clone()); rx });
        let mut app = App { pdfium, docs: vec![], tab: Tab::Home, draft: st.clone(), st, printers, show_print: false, prev: 0, pv: None, msg: String::new(), msg_at: None, logo, fs: false, show_info: false, scroll_tab: false, ipc, started: false, sel: None, paper: A4, paper_key: (String::new(), String::new()), pv_raw: None, pv_gen: 0, last_size: (0, 0), last_change: Instant::now(), pan: start_pan, search: String::new(), search_hit: None, search_focus: false, jump: false, show_cfg: false, cfg: Settings::default(), cfg_tab: 0, show_pall: false, pa_mode: false, pall_spec: Vec::new(), drv_seen: String::new(), gjob: None, over_acc: 0.0, over_at: Instant::now(), edge: (true, false), scroll_reset: None, tabs_overflow: false, task: None, tab_drag: None, next_id: 1, bar_hide: Instant::now() + Duration::from_secs(3), bar_lit: Instant::now(), upd: None, upd_msg: String::new(), upd_url: String::new(), pwd: None };
        #[cfg(target_os = "macos")]
        {
            mac_open::attach(cc.egui_ctx.clone());
            for p in mac_open::take() { app.open(p); } // PDF con cui e' stato lanciato dal Finder
        }
        for a in init { app.open(a); }
        app
    }

    /// Sposta una scheda (trascinandola): STAMPA TUTTO stampa le schede nell'ordine in cui sono disposte.
    fn move_tab(&mut self, from: usize, to: usize) {
        if from >= self.docs.len() || to >= self.docs.len() { return; }
        let cur_id = if let Tab::Doc(i) = self.tab { self.docs.get(i).map(|d| d.id) } else { None };
        let d = self.docs.remove(from);
        self.docs.insert(to, d);
        if let Some(id) = cur_id { if let Some(i) = self.docs.iter().position(|d| d.id == id) { self.tab = Tab::Doc(i); } }
        self.sel = None; self.pv = None; self.pv_raw = None; self.show_print = false; self.scroll_tab = true;
    }

    fn toast(&mut self, s: String) { self.msg = s; self.msg_at = Some(Instant::now()); }

    /// Cerca su GitHub la release piu' recente in un thread separato (l'interfaccia resta viva).
    fn check_update(&mut self, ctx: &egui::Context) {
        if self.upd.is_some() { return; }
        self.upd_msg = "Controllo in corso...".to_string();
        self.upd_url.clear();
        let (tx, rx) = std::sync::mpsc::channel();
        self.upd = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(latest_release());
            ctx.request_repaint();
        });
    }

    fn ensure_chars(&mut self, di: usize, page: usize) {
        if di >= self.docs.len() || self.docs[di].text.contains_key(&page) { return; }
        let v = load_chars(&self.docs[di].pdf, page);
        let d = &mut self.docs[di];
        if d.text.len() >= 20 { d.text.clear(); }
        d.text.insert(page, v);
    }

    /// Testo (minuscolo) di una pagina, per la ricerca.
    fn ensure_ptext(&mut self, di: usize, page: usize) {
        if di >= self.docs.len() || self.docs[di].ptext.contains_key(&page) { return; }
        let v: Vec<char> = self.docs[di].pdf.pages().get(page as u16).ok()
            .and_then(|p| p.text().ok().map(|t| t.all().chars().map(|c| c.to_lowercase().next().unwrap_or(c)).collect::<Vec<char>>()))
            .unwrap_or_default();
        self.docs[di].ptext.insert(page, v);
    }

    /// Cerca il risultato successivo/precedente (anche in altre pagine) e porta li' la vista.
    fn search_step(&mut self, di: usize, forward: bool) {
        let q: Vec<char> = self.search.chars().map(|c| c.to_lowercase().next().unwrap_or(c)).collect();
        if q.is_empty() { return; }
        let n = self.docs[di].sizes.len();
        let p0 = self.docs[di].cur;
        let from = self.search_hit.filter(|(p, _)| *p == p0).map(|(_, s)| s);
        let mut found = None;
        for k in 0..=n {
            let p = if forward { (p0 + k) % n } else { (p0 + n - k % n) % n };
            self.ensure_ptext(di, p);
            let hits = find_all(&self.docs[di].ptext[&p], &q);
            let pick = if forward { hits.iter().copied().find(|&h| k > 0 || from.map_or(true, |f| h > f)) }
                       else { hits.iter().copied().rev().find(|&h| k > 0 || from.map_or(true, |f| h < f)) };
            if let Some(h) = pick { found = Some((p, h)); break; }
        }
        match found {
            Some((p, h)) => { self.docs[di].cur = p; self.search_hit = Some((p, h)); self.jump = true; }
            None => { let t = self.search.clone(); self.toast(format!("Nessun risultato per «{t}»")); }
        }
    }

    fn selected_text(&self) -> Option<String> {
        let s = self.sel.as_ref()?;
        let ch = self.docs.get(s.doc)?.text.get(&s.page)?;
        let (lo, hi) = (s.a.min(s.b), s.a.max(s.b));
        let t: String = ch.get(lo..=hi)?.iter().map(|c| c.c).filter(|c| *c >= ' ' || matches!(*c, '\n' | '\r' | '\t')).collect();
        if t.trim().is_empty() { None } else { Some(t) }
    }

    fn open(&mut self, path: PathBuf) {
        // gia' aperto: passa a quella scheda invece di duplicarla
        if let Some(i) = self.docs.iter().position(|d| d.path == path) { self.tab = Tab::Doc(i); self.scroll_tab = true; return; }
        // se e' in corso una richiesta di password, i file aperti nel frattempo si accodano
        if let Some(p) = self.pwd.as_mut() { p.queue.push(path); return; }
        self.open_with(path, None, Vec::new());
    }

    /// Apre un PDF; se e' protetto da password la chiede nella finestra di dialogo, altrimenti mostra l'errore.
    /// `queue` sono i file aperti mentre era in corso la richiesta di password.
    fn open_with(&mut self, path: PathBuf, password: Option<&'static str>, mut queue: Vec<PathBuf>) {
        match self.pdfium.load_pdf_from_file(&path, password) {
            Ok(pdf) => {
                let sizes: Vec<(f32, f32)> = pdf.pages().iter().map(|p| (p.width().value, p.height().value)).collect();
                if sizes.is_empty() {
                    err(&format!("Impossibile aprire il file:\n{}", path.display()));
                } else {
                    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    let id = self.next_id; self.next_id += 1;
                    self.docs.push(Doc { id, pdf, path, name, sizes, cur: 0, zoom: 1.0, fitpage: self.st.fit_page, tex: HashMap::new(), text: HashMap::new(), ptext: HashMap::new(), thumbs: HashMap::new() });
                    self.tab = Tab::Doc(self.docs.len() - 1);
                    self.scroll_tab = true;
                }
            }
            Err(PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError)) => {
                // protetto: solo in questo caso si chiede la password, senza bloccare il resto
                self.pwd = Some(PwdAsk { path, text: String::new(), wrong: password.is_some(), focus: true, queue });
                return;
            }
            Err(_) => err(&format!("Impossibile aprire il file:\n{}", path.display())),
        }
        if !queue.is_empty() { // il file successivo aperto mentre si aspettava la password
            let next = queue.remove(0);
            self.open_with(next, None, queue);
        }
    }

    /// Finestra della password di un PDF protetto: si apre solo quando il file non si apre senza.
    fn pwd_window(&mut self, ctx: &egui::Context) {
        let Some(mut p) = self.pwd.take() else { return };
        let (mut open, mut go, mut cancel) = (true, false, ctx.input(|i| i.key_pressed(egui::Key::Escape)));
        let name = p.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let pos = ctx.screen_rect().center() - Vec2::new(200.0, 120.0);
        egui::Window::new("PDF protetto da password").open(&mut open).collapsible(false).resizable(false).movable(true)
            .default_pos(egui::pos2(pos.x.max(8.0), pos.y.max(8.0))).show(ctx, |ui| {
                ui.set_min_width(400.0);
                ui.label(RichText::new(format!("Il file «{name}» e' protetto da password.")).strong().size(16.0));
                ui.label("Inserisci la password per aprirlo.");
                ui.add_space(4.0);
                let r = ui.add(egui::TextEdit::singleline(&mut p.text).password(true).hint_text("Password").desired_width(360.0));
                if p.focus { r.request_focus(); p.focus = false; }
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) { go = true; }
                if p.wrong { ui.label(RichText::new("Password errata, riprova.").strong().color(ACCENT)); }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.add(Button::new(RichText::new("Apri").strong().color(Color32::WHITE)).fill(ACCENT).min_size(Vec2::new(110.0, 36.0))).clicked() { go = true; }
                    if ui.add(Button::new("Annulla").min_size(Vec2::new(100.0, 36.0))).clicked() { cancel = true; }
                });
            });
        if go && !p.text.is_empty() {
            // PdfDocument prende in prestito la password per tutta la sua vita: la stringa resta
            // in memoria (pochi byte) per potere tenere il documento aperto in una scheda
            let path = p.path.clone();
            let pw: &'static str = Box::leak(p.text.clone().into_boxed_str());
            self.pwd = None;
            self.open_with(path.clone(), Some(pw), p.queue);
            // password errata: la finestra resta aperta con il testo digitato, per correggerlo
            if let Some(a) = self.pwd.as_mut() { if a.path == path { a.text = pw.to_string(); a.focus = true; } }
        } else if go {
            p.wrong = true;
            self.pwd = Some(p);
        } else if cancel || !open {
            self.pwd = None;
            if !p.queue.is_empty() { let mut q = p.queue; let next = q.remove(0); self.open_with(next, None, q); }
        } else {
            self.pwd = Some(p);
        }
    }

    fn pick(&mut self) {
        // su Linux i filtri del file dialog distinguono maiuscole/minuscole: servono"pdf" e "PDF"
        if let Some(v) = rfd::FileDialog::new().add_filter("PDF", &["pdf", "PDF"]).set_title("Apri PDF").pick_files() {
            for p in v { self.open(p); }
        }
    }

    /// Mini anteprima di una pagina per la colonna di sinistra (resa una volta sola, poi in cache).
    fn thumb(&mut self, ctx: &egui::Context, di: usize, page: usize) -> Option<egui::TextureHandle> {
        if let Some(t) = self.docs[di].thumbs.get(&page) { return Some(t.clone()); }
        let (w, h, d) = render(&self.docs[di].pdf, page, 104)?;
        let t = ctx.load_texture("thumb", egui::ColorImage::from_rgba_unmultiplied([w, h], &d), egui::TextureOptions::LINEAR);
        let doc = &mut self.docs[di];
        if doc.thumbs.len() >= 400 { doc.thumbs.clear(); }
        doc.thumbs.insert(page, t.clone());
        Some(t)
    }

    fn get_tex(&mut self, ctx: &egui::Context, di: usize, page: usize, width: i32) -> Option<egui::TextureHandle> {
        if let Some(t) = self.docs[di].tex.get(&(page, width)) { return Some(t.clone()); }
        // mentre ridimensioni o fai zoom mostro la versione gia' pronta (scalata) e rifaccio il rendering a riposo: niente blocchi
        if self.last_change.elapsed() < Duration::from_millis(180) {
            if let Some(t) = self.docs[di].tex.iter().filter(|(k, _)| k.0 == page).min_by_key(|(k, _)| (k.1 - width).abs()).map(|(_, t)| t.clone()) {
                ctx.request_repaint_after(Duration::from_millis(200));
                return Some(t);
            }
        }
        let (w, h, d) = render(&self.docs[di].pdf, page, width)?;
        let t = ctx.load_texture("page", egui::ColorImage::from_rgba_unmultiplied([w, h], &d), egui::TextureOptions::LINEAR);
        let doc = &mut self.docs[di];
        doc.tex.retain(|k, _| k.0 != page);
        if doc.tex.len() >= 6 { doc.tex.clear(); }
        doc.tex.insert((page, width), t.clone());
        Some(t)
    }

    /// Allinea le scelte della finestra di stampa alle preferenze attuali del driver della stampante
    /// scelta (es. stampante impostata in bianco e nero -> "Bianco e nero" gia' selezionato).
    /// Si esegue quando cambia la stampante, non a ogni fotogramma: le scelte fatte a mano restano.
    fn sync_printer_defaults(&mut self) {
        let p = self.draft.printer.clone();
        if p.is_empty() { return; }
        if self.draft.drv_printer != p { self.draft.drv_mode.clear(); self.draft.drv_printer = p.clone(); }
        let d = spool::defaults(&p);
        if let Some(mono) = d.color { self.draft.color = if mono { ColorMode::Gray } else { ColorMode::Color }; }
        if let Some(c) = d.copies { self.draft.copies = c.clamp(1, 99); }
        if let Some(n) = d.nup { if NUPS.contains(&n) { self.draft.nup = n; } }
        self.paper_key = (String::new(), String::new()); // la carta si rilegge dal driver
        self.drv_seen = p;
    }

    /// Prepara la stampa a gruppo: `items` sono (id scheda, pagina) nell'ordine voluto. Le pagine
    /// dei vari file vengono impaginate insieme (N su 1) in UN solo lavoro di stampa.
    fn print_group(&mut self, items: Vec<(u64, usize)>) {
        if self.task.is_some() || self.gjob.is_some() { self.toast("Stampa gia' in corso".into()); return; }
        if self.st.printer.is_empty() { err("Nessuna stampante selezionata"); return; }
        if self.st.poster { self.toast("La stampa poster non si combina con la stampa a gruppo".into()); return; }
        if items.is_empty() { self.toast("Nessuna pagina selezionata per il gruppo".into()); return; }
        let st = self.st.clone();
        let nup = st.nup.max(1) as usize;
        let sheets: Vec<Vec<(u64, usize)>> = items.chunks(nup).map(|c| c.to_vec()).collect();
        let c = st.copies.max(1) as usize;
        let order: Vec<usize> = if st.collate {
            (0..c).flat_map(|_| 0..sheets.len()).collect()
        } else {
            (0..sheets.len()).flat_map(|s| std::iter::repeat(s).take(c)).collect()
        };
        let total = order.len();
        self.gjob = Some(GroupJob { st, sheets, order, pos: 0, job: None, area: (595.0, 842.0, 72.0), last: None, total, done: 0, cancel: false });
    }

    /// Un passo della stampa a gruppo (un foglio per volta, come la stampa normale).
    fn group_step(&mut self, ctx: &egui::Context) {
        let Some(mut g) = self.gjob.take() else { return };
        if g.cancel {
            if let Some(j) = g.job.as_mut() { j.abort(); }
            self.toast("Stampa del gruppo annullata".into());
            return;
        }
        match self.group_advance(&mut g) {
            Ok(true) => { self.gjob = Some(g); ctx.request_repaint(); }
            Ok(false) => {
                let (p, n) = (g.st.printer.clone(), g.sheets.len());
                self.toast(format!("Gruppo inviato a «{p}» ({n} fogli in un solo lavoro)"));
            }
            Err(e) => { if let Some(j) = g.job.as_mut() { j.abort(); } err(&e); }
        }
    }

    fn group_advance(&self, g: &mut GroupJob) -> Result<bool, String> {
        if g.job.is_none() { // un solo lavoro per tutto il gruppo
            let dm: Option<Vec<u8>> = if g.st.drv_printer == g.st.printer && !g.st.drv_mode.is_empty() { Some(from_hex(&g.st.drv_mode)) } else { None };
            let job = spool::Job::new(&g.st.printer, "PDF Reader CP - gruppo", dm.as_deref())
                .or_else(|e| if dm.is_some() { spool::Job::new(&g.st.printer, "PDF Reader CP - gruppo", None) } else { Err(e) })?;
            g.area = job.area(); g.job = Some(job); g.pos = 0; g.last = None;
            return Ok(true);
        }
        if g.pos < g.order.len() {
            let s = g.order[g.pos];
            if g.last.as_ref().map(|l| l.0) != Some(s) {
                let (aw, ah, _) = g.area;
                // le schede chiuse nel frattempo vengono saltate; il foglio si compone con quelle rimaste
                let items: Vec<(usize, usize)> = g.sheets[s].iter()
                    .filter_map(|&(id, p)| self.docs.iter().position(|d| d.id == id).map(|di| (di, p)))
                    .collect();
                if items.is_empty() { g.pos += 1; return Ok(true); }
                #[allow(unused_mut)] // `b` serve mutabile solo su Windows (RGBA -> BGRA)
                let (w, h, mut b) = compose_group(&self.docs, &items, (aw, ah), &g.st, 300.0 / 72.0, true)
                    .ok_or("Rendering della pagina non riuscito")?;
                #[cfg(windows)]
                for q in b.chunks_exact_mut(4) { q.swap(0, 2); }
                g.last = Some((s, w, h, b));
            }
            if let (Some(l), Some(job)) = (g.last.as_ref(), g.job.as_ref()) {
                let (aw, ah, dpi) = g.area;
                let f = dpi / 72.0;
                job.page(&l.3, l.1 as i32, l.2 as i32, 0, 0, (aw * f) as i32, (ah * f) as i32);
            }
            g.pos += 1; g.done += 1;
            return Ok(true);
        }
        g.job = None; // chiude il lavoro: il gruppo va in coda alla stampante tutto insieme
        Ok(false)
    }

    /// Avvia la stampa delle schede indicate (nell'ordine dato). `all_pages` = true: tutte le pagine di ogni scheda (STAMPA TUTTO).
    fn print_docs(&mut self, idx: &[usize], all_pages: bool) {
        if self.task.is_some() { self.toast("Stampa gia' in corso".into()); return; }
        if self.st.printer.is_empty() { err("Nessuna stampante selezionata"); return; }
        let mut st = self.st.clone();
        if all_pages { st.sel = PageSel::All; }
        let ids: Vec<u64> = idx.iter().filter_map(|&i| self.docs.get(i).map(|d| d.id)).collect();
        if ids.is_empty() { return; }
        let nup = st.nup.max(1) as usize;
        let c = st.copies.max(1) as usize;
        let total: usize = ids.iter().filter_map(|id| self.docs.iter().find(|d| d.id == *id))
            .map(|d| ((pages_for(&st, d.cur, d.sizes.len()).len() + nup - 1) / nup) * c * st.poster_tiles()).sum();
        self.task = Some(PrintTask { ids, st, di: 0, job: None, sheets: vec![], order: vec![], pos: 0, area: (595.0, 842.0, 72.0), last: None, total, done: 0, cancel: false });
    }

    /// Un passo del lavoro di stampa. Quando finisce, il lavoro e' in coda nella stampante e il cerchio di caricamento sparisce.
    fn print_step(&mut self, ctx: &egui::Context) {
        let Some(mut t) = self.task.take() else { return };
        if t.cancel {
            if let Some(j) = t.job.as_mut() { j.abort(); }
            self.toast("Stampa annullata".into());
            return;
        }
        match self.advance(&mut t) {
            Ok(true) => { self.task = Some(t); ctx.request_repaint(); }
            Ok(false) => { let (p, n) = (t.st.printer.clone(), t.ids.len()); self.toast(format!("Lavoro inviato a «{p}» ({n} schede in coda)")); }
            Err(e) => { if let Some(j) = t.job.as_mut() { j.abort(); } err(&e); }
        }
    }

    /// Esegue un'unita' di lavoro: apre il lavoro di una scheda, oppure invia un foglio, oppure chiude il lavoro (che va in coda).
    fn advance(&self, t: &mut PrintTask) -> Result<bool, String> {
        if t.job.is_none() { // apri la scheda successiva
            if t.di >= t.ids.len() { return Ok(false); }
            let Some(d) = self.docs.iter().find(|d| d.id == t.ids[t.di]) else { t.di += 1; return Ok(true); }; // scheda chiusa nel frattempo
            let pages = pages_for(&t.st, d.cur, d.sizes.len());
            if pages.is_empty() { return Err(format!("Nessuna pagina da stampare in {}", d.name)); }
            let nup = t.st.nup.max(1) as usize;
            let per = t.st.poster_tiles(); // con il poster ogni pagina occupa piu' fogli
            t.sheets = pages.chunks(nup).flat_map(|c| (0..per).map(move |_| c.to_vec())).collect();
            let (c, ns) = (t.st.copies.max(1) as usize, t.sheets.len());
            t.order = if t.st.collate { (0..c).flat_map(|_| 0..ns).collect() } else { (0..ns).flat_map(|s| std::iter::repeat(s).take(c)).collect() };
            let dm: Option<Vec<u8>> = if t.st.drv_printer == t.st.printer && !t.st.drv_mode.is_empty() { Some(from_hex(&t.st.drv_mode)) } else { None };
            let job = spool::Job::new(&t.st.printer, &d.name, dm.as_deref()).or_else(|e| if dm.is_some() { spool::Job::new(&t.st.printer, &d.name, None) } else { Err(e) })?;
            t.area = job.area(); t.pos = 0; t.last = None; t.job = Some(job);
            return Ok(true);
        }
        if t.pos < t.order.len() { // invia un foglio
            let Some(d) = self.docs.iter().find(|d| d.id == t.ids[t.di]) else {
                if let Some(j) = t.job.as_mut() { j.abort(); }
                t.job = None; t.di += 1; return Ok(true);
            };
            let s = t.order[t.pos];
            if t.last.as_ref().map(|l| l.0) != Some(s) {
                let (aw, ah, _) = t.area;
                #[allow(unused_mut)] // `b` serve mutabile solo su Windows (RGBA -> BGRA)
                let (w, h, mut b) = if t.st.poster {
                    let (cols, _rows) = t.st.poster_grid();
                    let tile = s % t.st.poster_tiles();
                    compose_poster(d, t.sheets[s][0], (aw, ah), &t.st, 300.0 / 72.0, true, tile % cols, tile / cols)
                } else {
                    compose(&self.pdfium, d, &t.sheets[s], (aw, ah), &t.st, 300.0 / 72.0, true)
                }.ok_or("Rendering della pagina non riuscito")?;
                #[cfg(windows)]
                for q in b.chunks_exact_mut(4) { q.swap(0, 2); } // RGBA -> BGRA per GDI (su macOS/Linux resta RGBA per il PNG)
                t.last = Some((s, w, h, b));
            }
            if let (Some(l), Some(job)) = (t.last.as_ref(), t.job.as_ref()) {
                let (aw, ah, dpi) = t.area;
                let f = dpi / 72.0;
                job.page(&l.3, l.1 as i32, l.2 as i32, 0, 0, (aw * f) as i32, (ah * f) as i32);
            }
            t.pos += 1; t.done += 1;
            return Ok(true);
        }
        t.job = None; // chiude il lavoro: la scheda e' in coda nella stampante
        t.di += 1;
        Ok(true)
    }

    /// STAMPA TUTTO: tutte le schede aperte (ognuna per intero), in sequenza, con un solo messaggio di conferma.
    fn print_all(&mut self) {
        if !self.docs.is_empty() { self.show_pall = true; }
    }

    /// Richiesta di STAMPA TUTTO / A GRUPPO: si scelgono le pagine di ogni file, si vedono i totali
    /// e si decide se stampare ogni file come lavoro separato o impaginare tutto insieme (gruppo).
    fn pall_window(&mut self, ctx: &egui::Context) {
        let n = self.docs.len();
        if n == 0 { self.show_pall = false; return; }
        // intervalli per file: restano attaccati alla scheda (id) anche riordinandole o chiudendone una
        let ids: Vec<u64> = self.docs.iter().map(|d| d.id).collect();
        let mut specs: Vec<String> = ids.iter()
            .map(|id| self.pall_spec.iter().find(|(k, _)| k == id).map(|(_, s)| s.clone()).unwrap_or_default())
            .collect();
        let (mut open, mut go, mut grp, mut one, mut change, mut cancel) = (true, false, false, false, false, ctx.input(|i| i.key_pressed(egui::Key::Escape)));
        let st = self.st.clone();
        let c = st.copies.max(1);
        let nup = st.nup.max(1) as usize;
        let totale_file: usize = self.docs.iter().map(|d| d.sizes.len()).sum();
        let warn = c > 1 && st.ask_copies;
        let colore = match st.color { ColorMode::Color => "COLORI", ColorMode::Auto => "AUTOMATICO", ColorMode::Gray => "BIANCO E NERO (SCALA DI GRIGI)" };
        let contrasto = if st.contrast == 0 { String::new() } else { format!(" · CONTRASTO {:+}", st.contrast) };
        let stampante = if st.printer.is_empty() { "(nessuna)".to_string() } else { st.printer.clone() };
        let pp = ctx.screen_rect().center() - Vec2::new(400.0, 350.0);
        egui::Window::new("STAMPA TUTTO / A GRUPPO").open(&mut open).collapsible(false).resizable(true).movable(true)
            .default_width(800.0).default_pos(egui::pos2(pp.x.max(8.0), pp.y.max(8.0))).show(ctx, |ui| {
            ui.set_min_width(760.0);
            { // testi e pulsanti piu' grandi, come nella finestra di stampa
                let s = ui.style_mut();
                s.text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(15.5));
                s.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(14.5));
                s.spacing.button_padding = Vec2::new(12.0, 6.0);
                s.spacing.interact_size.y = 30.0;
            }
            ui.label(RichText::new(format!("{n} SCHEDE APERTE · {totale_file} PAGINE IN TUTTE NEI FILE")).strong().size(20.0));
            ui.add_space(4.0);
            ui.group(|ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(format!("STAMPANTE: {}", stampante.to_uppercase())).strong());
                    ui.separator();
                    ui.label(format!("COLORE: {colore}{contrasto}"));
                    ui.separator();
                    ui.label(format!("COPIE: {c}"));
                    ui.separator();
                    ui.label(format!("{nup} PAGINE PER FOGLIO"));
                    ui.separator();
                    ui.label(if st.collate { "FASCICOLA: SI'" } else { "FASCICOLA: NO" });
                });
                if warn { ui.label(RichText::new(format!("ATTENZIONE: SONO IMPOSTATE {c} COPIE.")).strong().color(ACCENT)); }
            });
            ui.add_space(6.0);
            ui.label(RichText::new("PAGINE DA STAMPARE, FILE PER FILE").strong().color(ACCENT));
            ui.label(RichText::new("Vuoto = tutte le pagine. Esempi: 1-3, 5  ·  1,1 (pagina 1 due volte)  ·  2x3 (pagina 2 tre volte).").weak().small());
            egui::ScrollArea::vertical().id_salt("pall_files").max_height(270.0).auto_shrink([false, true]).show(ui, |ui| {
                ui.set_width(ui.available_width());
                for i in 0..n {
                    let d = &self.docs[i];
                    let pp = spec_pages(&specs[i], d.sizes.len(), d.cur);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("{}. {}", i + 1, short(&d.name, 46))).strong());
                        ui.label(RichText::new(format!("{} pag.", d.sizes.len())).weak().small());
                        ui.add(egui::TextEdit::singleline(&mut specs[i]).hint_text("tutte").desired_width(150.0));
                        ui.label(RichText::new(format!("→ {} pagine", pp.len())).strong());
                    });
                }
            });
            let mut tot = 0usize;
            for (i, s) in specs.iter().enumerate() { tot += spec_pages(s, self.docs[i].sizes.len(), self.docs[i].cur).len(); }
            let fogli = (tot + nup - 1) / nup;
            let fogli_tot = fogli * c as usize;
            ui.separator();
            egui::Grid::new("pall_tot").num_columns(2).spacing([20.0, 3.0]).show(ui, |ui| {
                ui.label("PAGINE TOTALI DA STAMPARE");
                ui.label(RichText::new(format!("{tot}")).strong().size(19.0).color(ACCENT)); ui.end_row();
                ui.label("FOGLI PER UNA COPIA");
                ui.label(RichText::new(format!("{fogli}  ({nup} pagine per foglio)")).strong()); ui.end_row();
                ui.label("COPIE");
                ui.label(RichText::new(format!("{c}")).strong()); ui.end_row();
                ui.label("TOTALE FOGLI DA STAMPARE");
                ui.label(RichText::new(format!("{fogli_tot}")).strong().size(19.0)); ui.end_row();
            });
            ui.add_space(4.0);
            ui.label(RichText::new("STAMPA A GRUPPO: un solo lavoro, con le pagine scelte impaginate insieme: la pagina 1 del file 1 e la pagina 3 del file 2 possono stare sullo stesso foglio (es. 2 su 1).").weak().small());
            ui.label(RichText::new("STAMPA TUTTO: ogni file resta un lavoro separato con tutte le sue pagine, come nella stampa standard (per la fascicolazione del driver).").weak().small());
            if st.poster { ui.label(RichText::new("Con la stampa poster attiva e' disponibile solo STAMPA TUTTO (ogni pagina su piu' fogli).").strong().color(ACCENT)); }
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(!st.poster, Button::new(RichText::new("STAMPA A GRUPPO").strong().size(16.0).color(Color32::WHITE)).fill(ACCENT).min_size(Vec2::new(185.0, 40.0)))
                    .on_hover_text("Un solo lavoro: le pagine scelte qui sopra vengono impaginate insieme").clicked() { grp = true; }
                if ui.add(Button::new(RichText::new("STAMPA TUTTO").size(16.0)).min_size(Vec2::new(150.0, 40.0)))
                    .on_hover_text("Un lavoro per ogni file, con tutte le sue pagine").clicked() { go = true; }
                if ui.add(Button::new(RichText::new("CAMBIA IMPOSTAZIONI").size(16.0)).min_size(Vec2::new(180.0, 40.0)))
                    .on_hover_text("Colore, N su 1, stampante, driver, stampa poster").clicked() { change = true; }
                if warn && ui.add(Button::new(RichText::new("1 COPIA").size(16.0)).min_size(Vec2::new(100.0, 40.0))).clicked() { one = true; }
                if ui.add(Button::new(RichText::new("ANNULLA").size(16.0)).min_size(Vec2::new(100.0, 40.0))).clicked() { cancel = true; }
            });
        });
        self.pall_spec = ids.into_iter().zip(specs.into_iter()).collect();
        let all: Vec<usize> = (0..n).collect();
        if grp {
            let mut items: Vec<(u64, usize)> = Vec::new();
            for d in self.docs.iter() {
                let spec = self.pall_spec.iter().find(|(k, _)| *k == d.id).map(|(_, s)| s.as_str()).unwrap_or("");
                for p in spec_pages(spec, d.sizes.len(), d.cur) { items.push((d.id, p)); }
            }
            self.show_pall = false;
            self.print_group(items);
        }
        else if go { self.show_pall = false; self.print_docs(&all, true); }
        else if one { self.show_pall = false; self.st.copies = 1; self.st.save(); self.print_docs(&all, true); }
        else if change { // porta alle impostazioni di stampa: li' "STAMPA TUTTO" stampa tutte le schede con le nuove impostazioni
            self.show_pall = false;
            if !matches!(self.tab, Tab::Doc(_)) { self.tab = Tab::Doc(0); }
            self.show_print = true; self.draft = self.st.clone(); self.prev = 0; self.pv = None; self.pv_raw = None; self.pa_mode = true;
        }
        else if cancel || !open { self.show_pall = false; }
    }

    /// Stampa subito la scheda indicata; con piu' di una copia chiede conferma.
    fn print_one(&mut self, di: usize) {
        let c = self.st.copies;
        if c > 1 && self.st.ask_copies && !confirm("Stampa", &format!("Sicuro di voler stampare {c} copie del file?\n\n{}", self.docs[di].name)) { return; }
        self.print_docs(&[di], false);
    }

    /// Finestra Impostazioni: si modifica una copia e si applica/salva con "Salva".
    fn settings_window(&mut self, ctx: &egui::Context) {
        let (mut open, mut save, mut cancel, mut reset) = (true, false, false, false);
        #[allow(unused_mut)] // cfg_drv si usa solo su Windows
        let (mut cfg_drv, mut take) = (false, false);
        egui::Window::new("Impostazioni").open(&mut open).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.set_min_width(470.0);
            ui.horizontal(|ui| {
                for (i, n) in ["Generale", "Stampa"].into_iter().enumerate() {
                    if ui.selectable_label(self.cfg_tab == i, RichText::new(n).size(16.0)).clicked() { self.cfg_tab = i; }
                }
            });
            ui.separator();
            if self.cfg_tab == 0 {
                ui.group(|ui| {
                    ui.label(RichText::new("Avvio e finestre").strong().color(ACCENT));
                    ui.checkbox(&mut self.cfg.background, "Apri sempre la finestra in secondo piano (non ruba il focus)");
                    ui.label(RichText::new("Aprendo altri PDF con il programma già aperto, restano nella sua finestra senza portarla davanti.").weak());
                    ui.checkbox(&mut self.cfg.start_max, "Avvia sempre ingrandita");
                    #[cfg(target_os = "macos")]
                    {
                        ui.separator();
                        ui.checkbox(&mut self.cfg.tabs_pin, "Fissa la barra delle schede");
                        ui.label(RichText::new("La finestra usa la barra del titolo di macOS: se la barra non e' fissata, quella con File, stampa e schede compare solo portando il mouse in cima alla finestra.").weak());
                    }
                });
                ui.group(|ui| {
                    ui.label(RichText::new("Visualizzazione").strong().color(ACCENT));
                    ui.horizontal(|ui| {
                        ui.label("Strumento iniziale:");
                        ui.radio_value(&mut self.cfg.start_pan, false, "Seleziona testo");
                        ui.radio_value(&mut self.cfg.start_pan, true, "Sposta pagina");
                    });
                    ui.horizontal(|ui| {
                        ui.label("Vista iniziale:");
                        ui.radio_value(&mut self.cfg.fit_page, false, "Larghezza");
                        ui.radio_value(&mut self.cfg.fit_page, true, "Pagina intera");
                    });
                });
            } else {
                ui.group(|ui| {
                    ui.label(RichText::new("All'avvio del programma").strong().color(ACCENT));
                    ui.radio_value(&mut self.cfg.start_mode, StartMode::Last, "Usa le ultime impostazioni di stampa utilizzate");
                    ui.radio_value(&mut self.cfg.start_mode, StartMode::Mine, "Usa le mie impostazioni predefinite (qui sotto)");
                    ui.checkbox(&mut self.cfg.reset_std, "Ripristina sempre la stampa standard (1 su 1, tutte le pagine)");
                    ui.checkbox(&mut self.cfg.use_driver_defaults, "Usa le impostazioni predefinite del driver (es. fronte/retro)");
                });
                ui.add_enabled_ui(self.cfg.start_mode == StartMode::Mine, |ui| {
                    ui.group(|ui| {
                        ui.label(RichText::new("Le mie impostazioni predefinite").strong().color(ACCENT));
                        ui.horizontal(|ui| {
                            ui.label("Stampante:");
                            egui::ComboBox::from_id_salt("cfg_pr").width(300.0).selected_text(self.cfg.mine.printer.clone()).show_ui(ui, |ui| {
                                for p in &self.printers { ui.selectable_value(&mut self.cfg.mine.printer, p.clone(), p); }
                            });
                        });
                        ui.horizontal(|ui| {
                            ui.label("Copie:");
                            ui.add(egui::DragValue::new(&mut self.cfg.mine.copies).range(1..=99));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Colore:");
                            ui.radio_value(&mut self.cfg.mine.color, ColorMode::Color, "Colori");
                            ui.radio_value(&mut self.cfg.mine.color, ColorMode::Auto, "Automatico");
                            ui.radio_value(&mut self.cfg.mine.color, ColorMode::Gray, "Bianco e nero");
                        });
                        #[cfg(windows)]
                        ui.horizontal(|ui| {
                            if ui.add(Button::new("Impostazioni driver stampante…").min_size(Vec2::new(250.0, 32.0))).clicked() { cfg_drv = true; }
                            if !self.cfg.mine.drv_mode.is_empty() { ui.label(RichText::new("attive").weak()); }
                        });
                        #[cfg(not(windows))]
                        {
                            ui.label(RichText::new("Opzioni stampante (CUPS)").strong().color(ACCENT));
                            cups_options(ui, &self.cfg.mine.printer, &mut self.cfg.mine.drv_printer, &mut self.cfg.mine.drv_mode);
                        }
                        if ui.add(Button::new("Prendi le impostazioni di stampa attuali").min_size(Vec2::new(250.0, 32.0))).clicked() { take = true; }
                    });
                });
                ui.group(|ui| {
                    ui.label(RichText::new("Conferme").strong().color(ACCENT));
                    ui.checkbox(&mut self.cfg.ask_copies, "Chiedi conferma quando stampo più di una copia");
                });
            }
            ui.separator();
            ui.label(RichText::new(format!("Salvate in: {}", Settings::path().map(|p| p.display().to_string()).unwrap_or_default())).weak().small());
            ui.horizontal(|ui| {
                if ui.add(Button::new(RichText::new("Salva").strong().color(Color32::WHITE)).fill(ACCENT).min_size(Vec2::new(110.0, 34.0))).clicked() { save = true; }
                if ui.add(Button::new("Annulla").min_size(Vec2::new(90.0, 34.0))).clicked() { cancel = true; }
                if ui.add(Button::new("Ripristina predefinite").min_size(Vec2::new(170.0, 34.0))).clicked() { reset = true; }
            });
        });
        if reset { let p = self.cfg.printer.clone(); self.cfg = Settings::default(); self.cfg.printer = p; }
        #[cfg(windows)]
        if cfg_drv {
            if self.cfg.mine.printer.is_empty() { self.cfg.mine.printer = self.st.printer.clone(); }
            let cur = if self.cfg.mine.drv_printer == self.cfg.mine.printer && !self.cfg.mine.drv_mode.is_empty() { Some(from_hex(&self.cfg.mine.drv_mode)) } else { None };
            if let Some(dm) = spool::driver_dialog(&self.cfg.mine.printer, cur.as_deref()) {
                self.cfg.mine.drv_printer = self.cfg.mine.printer.clone();
                self.cfg.mine.drv_mode = to_hex(&dm);
            }
        }
        #[cfg(not(windows))]
        let _ = cfg_drv;
        if take { self.cfg.mine = PrintPrefs::from_settings(&self.st); }
        if save { self.st = self.cfg.clone(); self.st.save(); self.show_cfg = false; self.toast("Impostazioni salvate".into()); }
        if cancel || !open { self.show_cfg = false; }
    }

    fn print_dialog(&mut self, ctx: &egui::Context, di: usize) {
        #[allow(unused_mut)] // drv si usa solo su Windows
        let (mut open, mut go, mut cancel, mut drv, mut drv_reset) = (true, false, false, false, false);
        let n = self.docs[di].sizes.len();
        // cambiando stampante le scelte seguono le preferenze del driver (colore, copie, N su 1, carta)
        if self.drv_seen != self.draft.printer { self.sync_printer_defaults(); }
        // le impostazioni del driver valgono solo per la stampante per cui sono state fatte
        if self.draft.drv_printer != self.draft.printer { self.draft.drv_mode.clear(); }
        let pk = (self.draft.printer.clone(), self.draft.drv_mode.clone());
        if self.paper_key != pk {
            let dm = if pk.1.is_empty() { None } else { Some(from_hex(&pk.1)) };
            self.paper = spool::paper_pts(&pk.0, dm.as_deref()).unwrap_or(A4);
            self.paper_key = pk;
        }
        let nup = self.draft.nup.max(1) as usize;
        let pages = pages_for(&self.draft, self.docs[di].cur, n);
        let per = self.draft.poster_tiles(); // fogli per pagina (1 se il poster e' spento)
        let sheets = sheet_count(&self.draft, pages.len());
        let prev = self.prev.min(sheets.saturating_sub(1));
        // anteprima: il foglio si ricompone solo se cambiano pagine/scala/carta; colore e chiaro/scuro si applicano al volo
        let d0 = &self.draft;
        let rk: RawKey = (di, prev, d0.sel, d0.custom.clone(), d0.nup, d0.border, d0.scale, d0.pct, d0.rotate, d0.center, (self.paper.0 as i32, self.paper.1 as i32), (d0.poster, (d0.poster_cols, d0.poster_rows), d0.poster_ov));
        if self.pv_raw.as_ref().map(|r| &r.0) != Some(&rk) {
            self.pv_gen += 1;
            self.pv_raw = pages.chunks(nup).nth(prev / per)
                .and_then(|chunk| if self.draft.poster {
                    let (cols, _rows) = self.draft.poster_grid();
                    let tile = prev % per;
                    compose_poster(&self.docs[di], chunk[0], self.paper, &self.draft, 1.5, false, tile % cols, tile / cols)
                } else {
                    compose(&self.pdfium, &self.docs[di], chunk, self.paper, &self.draft, 1.5, false)
                })
                .map(|(w, h, b)| (rk, w, h, b));
        }
        let tk: PvKey = (self.pv_gen, self.draft.color, self.draft.bright, self.draft.contrast);
        if self.pv.as_ref().map(|p| &p.0) != Some(&tk) {
            self.pv = self.pv_raw.as_ref().map(|(_, w, h, b)| {
                let mut t = b.clone();
                tone(&mut t, &self.draft);
                (tk, ctx.load_texture("pv", egui::ColorImage::from_rgba_unmultiplied([*w, *h], &t), egui::TextureOptions::LINEAR))
            });
        }
        let maxh = (ctx.screen_rect().height() - 140.0).max(320.0);
        // finestra spostabile: si trascina dalla barra del titolo (egui ricorda la posizione)
        let pos = ctx.screen_rect().center() - Vec2::new(430.0, 330.0);
        egui::Window::new("Stampa").open(&mut open).collapsible(false).resizable(false).movable(true)
            .default_pos(egui::pos2(pos.x.max(8.0), pos.y.max(8.0))).show(ctx, |ui| {
            { // testi compatti ma leggibili
                let s = ui.style_mut();
                s.text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(14.5));
                s.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(13.5));
                s.spacing.button_padding = Vec2::new(10.0, 5.0);
                s.spacing.interact_size.y = 26.0;
                s.spacing.icon_width = 18.0;
                s.spacing.slider_width = 180.0;
            }
            if self.pa_mode {
                ui.label(RichText::new(format!("STAMPA TUTTO: LE IMPOSTAZIONI VALGONO PER TUTTE LE {} SCHEDE APERTE (TUTTE LE PAGINE DI OGNI SCHEDA)", self.docs.len())).strong().color(ACCENT));
                ui.add_space(4.0);
            }
            ui.horizontal_top(|ui| {
                ui.spacing_mut().scroll = egui::style::ScrollStyle::solid(); // barra di scorrimento ben visibile quando serve
                egui::ScrollArea::vertical().max_height(maxh).auto_shrink([true, true]).show(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.set_width(390.0);
                        ui.group(|ui| {
                            ui.label(RichText::new("Stampante").strong().color(ACCENT));
                            egui::ComboBox::from_id_salt("pr").width(350.0).selected_text(self.draft.printer.clone()).show_ui(ui, |ui| {
                                for p in &self.printers { ui.selectable_value(&mut self.draft.printer, p.clone(), p); }
                            });
                            #[cfg(windows)]
                            {
                                ui.horizontal(|ui| {
                                    if ui.add(Button::new("Impostazioni driver stampante…").min_size(Vec2::new(270.0, 34.0)))
                                        .on_hover_text("Apre le proprietà del driver: fronte/retro, vassoio, qualità, colore, finitura...").clicked() { drv = true; }
                                    if !self.draft.drv_mode.is_empty() && ui.add(Button::new("Azzera").min_size(Vec2::new(70.0, 34.0))).on_hover_text("Torna alle impostazioni predefinite del driver").clicked() { drv_reset = true; }
                                });
                                if !self.draft.drv_mode.is_empty() { ui.label(RichText::new("Impostazioni del driver attive").weak()); }
                                else { ui.label(RichText::new("Impostazioni predefinite del driver (es. fronte/retro)").weak()); }
                            }
                            #[cfg(not(windows))]
                            {
                                // interfaccia di stampa: le opzioni della coda CUPS si scelgono qui
                                ui.label(RichText::new("Opzioni stampante (CUPS)").strong().color(ACCENT));
                                cups_options(ui, &self.draft.printer, &mut self.draft.drv_printer, &mut self.draft.drv_mode);
                                if !self.draft.drv_mode.is_empty()
                                    && ui.add(Button::new("Azzera opzioni").min_size(Vec2::new(160.0, 30.0))).clicked()
                                {
                                    drv_reset = true;
                                }
                            }
                            ui.horizontal(|ui| {
                                ui.label("Copie");
                                if ui.add(Button::new(RichText::new("-").size(18.0)).min_size(Vec2::new(38.0, 30.0))).clicked() { self.draft.copies = self.draft.copies.saturating_sub(1).max(1); }
                                ui.add(egui::DragValue::new(&mut self.draft.copies).range(1..=99));
                                if ui.add(Button::new(RichText::new("+").size(18.0)).min_size(Vec2::new(38.0, 30.0))).clicked() { self.draft.copies = (self.draft.copies + 1).min(99); }
                                ui.checkbox(&mut self.draft.collate, "Fascicola");
                            });
                        });
                        ui.group(|ui| {
                            ui.label(RichText::new(format!("Intervallo pagine · {n} totali")).strong().color(ACCENT));
                            ui.horizontal(|ui| {
                                ui.radio_value(&mut self.draft.sel, PageSel::All, "Tutto");
                                ui.radio_value(&mut self.draft.sel, PageSel::Current, "Corrente");
                                ui.radio_value(&mut self.draft.sel, PageSel::Custom, "Personalizzato");
                            });
                            if self.draft.sel == PageSel::Custom {
                                ui.text_edit_singleline(&mut self.draft.custom);
                                ui.label(RichText::new("Esempi: 1-3, 5  ·  1,1 (pagina 1 due volte)  ·  2x3 (pagina 2 tre volte)").weak().small());
                            }
                        });
                        ui.group(|ui| {
                            ui.label(RichText::new("Pagine per foglio e scala").strong().color(ACCENT));
                            if ui.checkbox(&mut self.draft.poster, "Stampa poster (una pagina su piu' fogli)").changed() && self.draft.poster {
                                self.draft.nup = 1; // il poster divide sempre una sola pagina
                            }
                            if self.draft.poster {
                                ui.horizontal(|ui| {
                                    ui.label("Griglia di fogli");
                                    let mut g = (self.draft.poster_cols, self.draft.poster_rows);
                                    egui::ComboBox::from_id_salt("po").width(90.0).selected_text(format!("{} x {}", g.0, g.1)).show_ui(ui, |ui| {
                                        for (c, r) in [(1u32, 2u32), (2, 1), (2, 2), (3, 2), (2, 3), (3, 3), (4, 4)] {
                                            ui.selectable_value(&mut g, (c, r), format!("{c} x {r}"));
                                        }
                                    });
                                    if g != (self.draft.poster_cols, self.draft.poster_rows) { self.draft.poster_cols = g.0; self.draft.poster_rows = g.1; }
                                    ui.label(RichText::new(format!("= {} fogli per pagina", g.0 * g.1)).weak());
                                });
                                ui.horizontal(|ui| {
                                    ui.label("Sovrapposizione");
                                    ui.add(egui::DragValue::new(&mut self.draft.poster_ov).range(0.0..=30.0).speed(0.5).suffix(" mm"))
                                        .on_hover_text("Margine ripetuto sui lati interni dei fogli, per unirli senza tagliare il disegno");
                                });
                                ui.label(RichText::new("La pagina viene ingrandita sull'intero poster: ogni foglio ne stampa una parte, da unire. Con sovrapposizione maggiore di zero il foglio riporta i segni di taglio e la scritta R<riga>C<colonna>.").weak().small());
                            }
                            ui.add_enabled_ui(!self.draft.poster, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label("Pagine per foglio");
                                    egui::ComboBox::from_id_salt("nu").width(90.0).selected_text(format!("{} su 1", self.draft.nup)).show_ui(ui, |ui| {
                                        for k in NUPS { ui.selectable_value(&mut self.draft.nup, k, format!("{k} su 1")); }
                                    });
                                });
                                ui.add_enabled_ui(self.draft.nup > 1, |ui| ui.checkbox(&mut self.draft.border, "Bordo attorno alle pagine"));
                                ui.add_enabled_ui(self.draft.nup == 1, |ui| {
                                    ui.radio_value(&mut self.draft.scale, ScaleMode::Fit, "Adatta alla pagina");
                                    ui.radio_value(&mut self.draft.scale, ScaleMode::Actual, "Dimensioni reali (100%)");
                                    ui.horizontal(|ui| {
                                        ui.radio_value(&mut self.draft.scale, ScaleMode::Custom, "Proporzione personalizzata");
                                        ui.add_enabled(self.draft.scale == ScaleMode::Custom, egui::DragValue::new(&mut self.draft.pct).range(25..=200).suffix(" %"));
                                    });
                                });
                            });
                            ui.checkbox(&mut self.draft.rotate, "Ruota automaticamente");
                            ui.checkbox(&mut self.draft.center, "Centra automaticamente");
                        });
                        ui.group(|ui| {
                            ui.label(RichText::new("Colore e luminosità").strong().color(ACCENT));
                            ui.horizontal(|ui| {
                                                            ui.radio_value(&mut self.draft.color, ColorMode::Color, "Colori");
                            ui.radio_value(&mut self.draft.color, ColorMode::Auto, "Automatico");
                            ui.radio_value(&mut self.draft.color, ColorMode::Gray, "Bianco e nero");
                            });
                            ui.label(RichText::new("Automatico: decide la stampante. Bianco e nero: il foglio viene convertito in scala di grigi.").weak().small());
                            ui.horizontal(|ui| {
                                ui.label("Scuro");
                                ui.add(egui::Slider::new(&mut self.draft.bright, -100..=100).show_value(false));
                                ui.label("Chiaro");
                                if ui.add(Button::new("0").min_size(Vec2::new(36.0, 30.0))).on_hover_text("Azzera chiaro/scuro").clicked() { self.draft.bright = 0; }
                            });
                            ui.horizontal(|ui| {
                                ui.label("Contrasto");
                                ui.add(egui::Slider::new(&mut self.draft.contrast, -100..=100).show_value(false));
                                if ui.add(Button::new("0").min_size(Vec2::new(36.0, 30.0))).on_hover_text("Azzera il contrasto").clicked() { self.draft.contrast = 0; }
                                ui.label(RichText::new("Basso ─ Alto").weak().small());
                            });
                        });
                        ui.label(RichText::new("Le impostazioni vengono ricordate per «Stampa subito».").weak());
                    });
                });
                ui.vertical(|ui| {
                    let (pw, ph) = self.paper;
                    let mm = |p: f32| (p / 72.0 * 25.4).round() as i32;
                    ui.label(format!("Carta: {} × {} mm", mm(pw), mm(ph)));
                    let k = (300.0 / pw).min(424.0 / ph);
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(pw * k, ph * k), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 0.0, Color32::WHITE);
                    if let Some((_, t)) = &self.pv {
                        ui.painter().image(t.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                    }
                    ui.painter().rect_stroke(rect, 0.0, (1.0, Color32::GRAY));
                    ui.horizontal_wrapped(|ui| {
                        for p in 0..sheets.min(40) { if ui.selectable_label(prev == p, (p + 1).to_string()).clicked() { self.prev = p; } }
                    });
                    if self.draft.poster {
                        let (cols, rows) = self.draft.poster_grid();
                        let tile = prev % per;
                        ui.label(format!("Foglio {} di {} · poster {} x {} (riga {}, colonna {})", prev + 1, sheets.max(1), cols, rows, tile / cols + 1, tile % cols + 1));
                    } else {
                        ui.label(format!("Foglio {} di {} · {} pag. per foglio", prev + 1, sheets.max(1), nup));
                    }
                });
            });
            ui.separator();
            ui.horizontal(|ui| {
                if ui.add(Button::new(RichText::new(if self.pa_mode { "STAMPA TUTTO" } else { "Stampa" }).size(16.0).strong().color(Color32::WHITE)).fill(ACCENT).min_size(Vec2::new(if self.pa_mode { 176.0 } else { 136.0 }, 38.0))).clicked() { go = true; }
                if ui.add(Button::new(RichText::new("Annulla").size(16.0)).min_size(Vec2::new(110.0, 38.0))).clicked() { cancel = true; }
            });
        });
        #[cfg(windows)]
        if drv {
            let cur = if self.draft.drv_printer == self.draft.printer && !self.draft.drv_mode.is_empty() { Some(from_hex(&self.draft.drv_mode)) } else { None };
            if let Some(dm) = spool::driver_dialog(&self.draft.printer, cur.as_deref()) {
                self.draft.drv_printer = self.draft.printer.clone();
                self.draft.drv_mode = to_hex(&dm);
            }
        }
        #[cfg(not(windows))]
        let _ = drv; // su macOS/Linux le opzioni sono nella finestra (cups_options)
        if drv_reset { self.draft.drv_mode.clear(); self.drv_seen.clear(); }
        if go {
            let c = self.draft.copies;
            let testo = if self.pa_mode { format!("Sicuro di voler stampare {c} copie dei file aperti?") } else { format!("Sicuro di voler stampare {c} copie del file?

{}", self.docs[di].name) };
            if c > 1 && self.st.ask_copies && !confirm("Stampa", &testo) {
                // risposta "No": resta aperta la finestra di stampa
            } else {
                self.st = self.draft.clone(); self.st.save(); self.show_print = false;
                if self.pa_mode { self.pa_mode = false; let all: Vec<usize> = (0..self.docs.len()).collect(); self.print_docs(&all, true); }
                else { self.print_docs(&[di], false); }
            }
        }
        if cancel || !open { self.show_print = false; self.pa_mode = false; self.drv_seen.clear(); }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Ic { Preview, Quick, All, Min, Max, Close, Pan, Sel, Thumbs }

fn icon_btn(ui: &mut egui::Ui, ic: Ic, tip: &str, enabled: bool) -> bool { icon_btn_a(ui, ic, tip, enabled, false) }

/// Pulsante-icona disegnato a mano (nessun font di icone necessario). `active` = strumento attivo (sfondo evidenziato).
fn icon_btn_a(ui: &mut egui::Ui, ic: Ic, tip: &str, enabled: bool, active: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(30.0), if enabled { egui::Sense::click() } else { egui::Sense::hover() });
    let hov = enabled && resp.hovered();
    let p = ui.painter();
    if active { p.rect_filled(rect, 4.0, Color32::from_rgb(250, 218, 218)); }
    if hov { p.rect_filled(rect, 4.0, if ic == Ic::Close { Color32::from_rgb(232, 17, 35) } else { Color32::from_gray(205) }); }
    let col = if !enabled { Color32::from_gray(160) } else if ic == Ic::Close && hov { Color32::WHITE } else if ic == Ic::All || active { ACCENT } else { Color32::from_gray(35) };
    let st = egui::Stroke::new(1.6_f32, col);
    let c = rect.center();
    let v = |x: f32, y: f32| c + Vec2::new(x, y);
    match ic {
        Ic::Sel => { // cursore a I: seleziona testo
            p.line_segment([v(0.0, -9.0), v(0.0, 9.0)], st);
            p.line_segment([v(-4.0, -9.0), v(4.0, -9.0)], st);
            p.line_segment([v(-4.0, 9.0), v(4.0, 9.0)], st);
        }
        Ic::Pan => { // quattro frecce: sposta la pagina
            p.line_segment([v(-9.0, 0.0), v(9.0, 0.0)], st);
            p.line_segment([v(0.0, -9.0), v(0.0, 9.0)], st);
            for (dx, dy) in [(1.0_f32, 0.0_f32), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                let (tx, ty, bx, by, px, py) = (dx * 11.0, dy * 11.0, dx * 6.0, dy * 6.0, -dy * 3.5, dx * 3.5);
                p.add(egui::Shape::convex_polygon(vec![v(tx, ty), v(bx + px, by + py), v(bx - px, by - py)], col, egui::Stroke::NONE));
            }
        }
        Ic::Thumbs => { // colonna di mini pagine + pagina grande: le anteprime a sinistra
            for i in 0..3 {
                let y = -9.0 + i as f32 * 6.0;
                p.rect_stroke(egui::Rect::from_min_max(v(-9.0, y), v(-2.0, y + 4.5)), 0.0, st);
            }
            p.rect_stroke(egui::Rect::from_min_max(v(0.0, -9.0), v(9.0, 9.0)), 1.0, st);
        }
        Ic::Min => { p.line_segment([v(-5.0, 4.0), v(5.0, 4.0)], st); }
        Ic::Max => { p.rect_stroke(egui::Rect::from_center_size(c, Vec2::splat(10.0)), 0.0, st); }
        Ic::Close => { p.line_segment([v(-5.0, -5.0), v(5.0, 5.0)], st); p.line_segment([v(-5.0, 5.0), v(5.0, -5.0)], st); }
        _ => {
            if ic == Ic::All { p.rect_stroke(egui::Rect::from_min_max(v(-2.0, -12.0), v(8.0, -4.0)), 0.0, st); } // pagine impilate
            p.rect_stroke(egui::Rect::from_min_max(v(-5.0, -9.0), v(5.0, -1.0)), 0.0, st);   // foglio in alto
            p.rect_stroke(egui::Rect::from_min_max(v(-10.0, -1.0), v(10.0, 7.0)), 2.0, st);  // corpo stampante
            let out = egui::Rect::from_min_max(v(-5.0, 4.0), v(5.0, 11.0));
            p.rect_filled(out, 0.0, BAR);
            p.rect_stroke(out, 0.0, st);                                                     // foglio in uscita
            match ic {
                Ic::Preview => { p.circle_stroke(v(8.0, 8.0), 3.5, st); p.line_segment([v(10.5, 10.5), v(13.0, 13.0)], st); }
                Ic::Quick => { p.add(egui::Shape::convex_polygon(vec![v(7.0, 3.0), v(7.0, 13.0), v(14.0, 8.0)], col, egui::Stroke::NONE)); }
                _ => {}
            }
        }
    }
    resp.on_hover_text(tip).clicked()
}

/// Scheda con la "×" di chiusura integrata. Restituisce (risposta, chiusura_richiesta).
fn tab_widget(ui: &mut egui::Ui, text: &str, selected: bool, tip: &str, num: Option<usize>) -> (egui::Response, bool) {
    let col = if selected { Color32::from_gray(20) } else { Color32::from_gray(70) };
    let galley = ui.painter().layout_no_wrap(text.to_string(), egui::TextStyle::Button.resolve(ui.style()), col);
    let (pad, xw) = (10.0_f32, 18.0_f32);
    let extra = if num.is_some() { 13.0 } else { 0.0 };
    let (full, resp) = ui.allocate_exact_size(Vec2::new(pad + galley.size().x + 8.0 + xw + 4.0, 26.0 + extra), egui::Sense::click_and_drag());
    let rect = egui::Rect::from_min_size(full.min, Vec2::new(full.width(), 26.0));
    let xr = egui::Rect::from_center_size(egui::pos2(rect.right() - 4.0 - xw / 2.0, rect.center().y), Vec2::splat(xw));
    let hover_x = resp.hovered() && ui.input(|i| i.pointer.hover_pos()).map_or(false, |p| xr.contains(p));
    let close = resp.clicked() && resp.interact_pointer_pos().map_or(false, |p| xr.contains(p));
    let p = ui.painter();
    let bg = if selected { Color32::WHITE } else if resp.hovered() { Color32::from_gray(220) } else { Color32::from_gray(205) };
    p.rect_filled(rect, egui::Rounding { nw: 6.0, ne: 6.0, sw: 0.0, se: 0.0 }, bg);
    if selected { p.rect_filled(egui::Rect::from_min_size(rect.min, Vec2::new(rect.width(), 2.0)), 0.0, ACCENT); }
    p.galley(egui::pos2(rect.left() + pad, rect.center().y - galley.size().y / 2.0), galley.clone(), col);
    if hover_x { p.rect_filled(xr, 4.0, Color32::from_rgb(232, 17, 35)); }
    let st = egui::Stroke::new(1.4_f32, if hover_x { Color32::WHITE } else { Color32::from_gray(90) });
    let c = xr.center();
    p.line_segment([c + Vec2::new(-3.5, -3.5), c + Vec2::new(3.5, 3.5)], st);
    p.line_segment([c + Vec2::new(-3.5, 3.5), c + Vec2::new(3.5, -3.5)], st);
    if let Some(k) = num { p.text(egui::pos2(rect.center().x, rect.bottom() + 7.0), egui::Align2::CENTER_CENTER, k.to_string(), egui::FontId::proportional(10.0), Color32::from_gray(100)); }
    (resp.on_hover_text(tip), close)
}

/// Voce di menu: chiude il menu al clic.
fn mi(ui: &mut egui::Ui, text: &str, shortcut: &str, en: bool) -> bool {
    let c = ui.add_enabled(en, Button::new(text).shortcut_text(shortcut)).clicked();
    if c { ui.close_menu(); }
    c
}

/// Su Linux la finestra e' senza cornice di sistema: bordi e angoli diventano maniglie di
/// ridimensionamento, altrimenti la finestra resterebbe di dimensione fissa. Su macOS la
/// cornice e' quella nativa, quindi il ridimensionamento e' quello di sistema.
#[cfg(all(unix, not(target_os = "macos")))]
fn resize_grips(ctx: &egui::Context) {
    use egui::viewport::ResizeDirection as D;
    let r = ctx.screen_rect();
    let (t, c) = (6.0_f32, 14.0_f32); // spessore dei bordi, lato degli angoli
    let items: [(egui::Rect, D, egui::CursorIcon); 8] = [
        (egui::Rect::from_min_max(egui::pos2(r.left() + c, r.top()), egui::pos2(r.right() - c, r.top() + t)), D::North, egui::CursorIcon::ResizeVertical),
        (egui::Rect::from_min_max(egui::pos2(r.left() + c, r.bottom() - t), egui::pos2(r.right() - c, r.bottom())), D::South, egui::CursorIcon::ResizeVertical),
        (egui::Rect::from_min_max(egui::pos2(r.left(), r.top() + c), egui::pos2(r.left() + t, r.bottom() - c)), D::West, egui::CursorIcon::ResizeHorizontal),
        (egui::Rect::from_min_max(egui::pos2(r.right() - t, r.top() + c), egui::pos2(r.right(), r.bottom() - c)), D::East, egui::CursorIcon::ResizeHorizontal),
        (egui::Rect::from_min_max(egui::pos2(r.left(), r.top()), egui::pos2(r.left() + c, r.top() + c)), D::NorthWest, egui::CursorIcon::ResizeNwSe),
        (egui::Rect::from_min_max(egui::pos2(r.right() - c, r.top()), egui::pos2(r.right(), r.top() + c)), D::NorthEast, egui::CursorIcon::ResizeNeSw),
        (egui::Rect::from_min_max(egui::pos2(r.left(), r.bottom() - c), egui::pos2(r.left() + c, r.bottom())), D::SouthWest, egui::CursorIcon::ResizeNeSw),
        (egui::Rect::from_min_max(egui::pos2(r.right() - c, r.bottom() - c), egui::pos2(r.right(), r.bottom())), D::SouthEast, egui::CursorIcon::ResizeNwSe),
    ];
    for (i, (rect, dir, cursor)) in items.into_iter().enumerate() {
        egui::Area::new(egui::Id::new(("ridimensiona", i)))
            .order(egui::Order::Foreground)
            .fixed_pos(rect.min)
            .show(ctx, |ui| {
                let resp = ui.allocate_response(rect.size(), egui::Sense::drag());
                if resp.hovered() { ctx.set_cursor_icon(cursor); }
                if resp.drag_started() { ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(dir)); }
            });
    }
}

/// Opzioni della coda CUPS dentro la finestra di stampa (macOS/Linux): fronte/retro,
/// vassoio, qualita', colore... Si salvano in `drv_mode` per quella stampante.
#[cfg(not(windows))]
fn cups_options(ui: &mut egui::Ui, printer: &str, drv_printer: &mut String, drv_mode: &mut String) {
    if printer.is_empty() {
        ui.label(RichText::new("Scegli prima una stampante").weak());
        return;
    }
    if printer != drv_printer.as_str() { // stampante cambiata: si riparte dalle sue impostazioni
        drv_printer.clear();
        drv_mode.clear();
        *drv_printer = printer.to_string();
    }
    let opts = spool::options(printer);
    if opts.is_empty() {
        ui.label(RichText::new("La coda CUPS non riporta opzioni (driver senza PPD?)").weak());
        return;
    }
    let raw = from_hex(drv_mode);
    let mut saved = spool::saved(if raw.is_empty() { None } else { Some(&raw) });
    let mut changed = false;
    for o in &opts {
        let cur = saved.iter().find(|(k, _)| *k == o.key).map(|(_, v)| v.clone()).unwrap_or_else(|| o.current.clone());
        ui.horizontal(|ui| {
            ui.label(format!("{}:", o.label));
            egui::ComboBox::from_id_salt(("cups", &o.key)).width(260.0).selected_text(cur.clone()).show_ui(ui, |ui| {
                for v in &o.values {
                    if ui.selectable_label(cur == *v, v).clicked() {
                        saved.retain(|(k, _)| *k != o.key);
                        saved.push((o.key.clone(), v.clone()));
                        changed = true;
                    }
                }
            });
        });
    }
    if changed {
        let s = saved.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(";");
        *drv_mode = to_hex(s.as_bytes());
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _f: &mut eframe::Frame) {
        // PDF aperti dal Finder (doppio clic, Apri con) mentre il programma era gia' avviato
        #[cfg(target_os = "macos")]
        for p in mac_open::take() { self.open(p); }
        let before = self.st.clone();
        if !self.started {
            self.started = true;
            #[cfg(target_os = "macos")]
            mac_menu::install(); // menu dell'app in italiano (AppKit lo crea in inglese)
            if self.st.start_max && !self.st.background { ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true)); } // si apre sempre ingrandita
        }
        // PDF inviati da un secondo avvio del programma: si aprono qui, in nuove schede
        // controllo aggiornamenti: il risultato arriva dal thread che interroga GitHub
        if let Some(rx) = self.upd.take() {
            match rx.try_recv() {
                Ok((m, u)) => { self.upd_msg = m; self.upd_url = u; }
                Err(std::sync::mpsc::TryRecvError::Empty) => { self.upd = Some(rx); ctx.request_repaint_after(Duration::from_millis(250)); }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => { self.upd_msg = "Controllo non riuscito".into(); }
            }
        }
        let msgs: Vec<Ipc> = self.ipc.as_ref().map(|r| r.try_iter().collect::<Vec<Ipc>>()).unwrap_or_default();
        if !msgs.is_empty() && !self.st.background { // in secondo piano: i file si aprono senza portare la finestra davanti
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        for m in msgs { if let Ipc::Open(p) = m { self.open(p); } }
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        for p in dropped { self.open(p); }
        let (mut o, mut pd, mut q, mut all) = (false, false, false, false);
        ctx.input_mut(|i| {
            o |= i.consume_key(egui::Modifiers::COMMAND, egui::Key::O);
            q |= i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::P);
            pd |= i.consume_key(egui::Modifiers::COMMAND, egui::Key::P);
        });
        if ctx.input(|i| i.key_pressed(egui::Key::F11)) { self.fs = !self.fs; ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fs)); }
        if let Tab::Doc(i) = self.tab {
            if !self.show_print && !ctx.wants_keyboard_input() {
                let (nx, pv, z) = ctx.input(|i| (i.key_pressed(egui::Key::PageDown) || i.key_pressed(egui::Key::ArrowRight), i.key_pressed(egui::Key::PageUp) || i.key_pressed(egui::Key::ArrowLeft), i.zoom_delta()));
                if let Some(d) = self.docs.get_mut(i) {
                    if nx && d.cur + 1 < d.sizes.len() { d.cur += 1; }
                    if pv && d.cur > 0 { d.cur -= 1; }
                    if z != 1.0 { d.zoom = (d.zoom * z).clamp(0.3, 4.0); }
                }
            }
        }

        // ---- UNICA barra = barra del titolo: [icona][File][schede ............][trascina][stampa][- o x] ----
        let (mut ctrl_w, mut next_t, mut prev_t) = (false, false, false);
        ctx.input_mut(|i| {
            prev_t = i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::Tab);
            next_t = i.consume_key(egui::Modifiers::COMMAND, egui::Key::Tab);
            ctrl_w = i.consume_key(egui::Modifiers::COMMAND, egui::Key::W);
        });
        let typing = ctx.wants_keyboard_input();
        let mut sel_all = false;
        let mut no_text = false;
        let (mut search_next, mut search_prev) = (false, false);
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::F)) { self.search_focus = true; }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Comma)) { self.show_cfg = true; self.cfg = self.st.clone(); }
        if !typing {
            ctx.input_mut(|i| {
                if i.consume_key(egui::Modifiers::NONE, egui::Key::H) { self.pan = true; }
                if i.consume_key(egui::Modifiers::NONE, egui::Key::T) { self.pan = false; }
            });
        }
        let mut copy_req = !typing && ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Copy)));
        if !typing { ctx.input_mut(|i| { sel_all = i.consume_key(egui::Modifiers::COMMAND, egui::Key::A); }); }
        let n_docs = self.docs.len();
        if n_docs > 0 {
            let i = if let Tab::Doc(i) = self.tab { i } else { 0 };
            if next_t { self.tab = Tab::Doc((i + 1) % n_docs); self.scroll_tab = true; }
            if prev_t { self.tab = Tab::Doc((i + n_docs - 1) % n_docs); self.scroll_tab = true; }
        }
        let has_docs = n_docs > 0;
        let cur_doc = if let Tab::Doc(i) = self.tab { Some(i) } else { None };
        let maxed = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        let mut close: Option<usize> = None;
        let (mut tabs_w, mut tabs_avail) = (0.0_f32, 0.0_f32);
        let mut reorder: Option<(usize, usize)> = None;
        let (mut close_all, mut show_info, mut quit, mut fs_toggle) = (false, false, false, false);
        let want_scroll = std::mem::take(&mut self.scroll_tab);
        // su macOS la finestra ha la barra del titolo e i pulsanti nativi: la barra qui sotto
        // (File, stampa, strumenti e schede) puo' restare fissa oppure a scomparsa, e compare
        // portando il mouse in cima alla finestra. Sulle altre piattaforme resta sempre visibile
        // perche' contiene anche i pulsanti della finestra.
        let macos = cfg!(target_os = "macos");
        let bar_h = if self.tabs_overflow { 52.0 } else { 38.0 };
        // la barra non sparisce appena il mouse se ne va: resta 3 secondi, e se ci si clicca
        // sopra (File, stampa, schede) resta 2 secondi in piu' ed evidenziata
        let hot = ctx.pointer_latest_pos().map_or(false, |p| p.y <= ctx.screen_rect().top() + bar_h + 2.0);
        let now = Instant::now();
        let was_visible = !macos || self.st.tabs_pin || self.docs.is_empty() || self.tab_drag.is_some()
            || hot || now < self.bar_hide;
        // menu di File aperto: la barra non si ritira mentre lo si usa
        let popup = macos && was_visible && ctx.memory(|m| m.any_popup_open());
        if macos && !self.st.tabs_pin && (hot || self.tab_drag.is_some() || popup) {
            self.bar_hide = now + Duration::from_secs(3);
        }
        if macos && !self.st.tabs_pin && hot && ctx.input(|i| i.pointer.any_click()) {
            self.bar_hide = self.bar_hide.max(now + Duration::from_secs(2));
            self.bar_lit = now + Duration::from_secs(2);
        }
        let bar_lit = macos && now < self.bar_lit;
        let bar_visible = was_visible;
        if bar_visible {
        egui::TopBottomPanel::top("top")
            .exact_height(if self.tabs_overflow { 52.0 } else { 38.0 })
            .frame(egui::Frame::none().fill(if bar_lit { BAR_LIT } else { BAR }).inner_margin(egui::Margin::symmetric(6.0, 3.0)))
            .show(ctx, |ui| {
                // tutta la barra e' una maniglia: trascinando negli spazi liberi si sposta la finestra; doppio clic = ingrandisci/ripristina
                let bar = ui.interact(ui.max_rect(), egui::Id::new("titlebar_drag"), egui::Sense::click_and_drag());
                if bar.drag_started() { ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag); }
                if bar.double_clicked() { ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maxed)); }
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if !macos { // su macOS ci sono i pulsanti nativi della barra del titolo
                            if icon_btn(ui, Ic::Close, "Chiudi", true) { quit = true; }
                            if icon_btn(ui, Ic::Max, if maxed { "Ripristina" } else { "Massimizza" }, true) { ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maxed)); }
                            if icon_btn(ui, Ic::Min, "Riduci a icona", true) { ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)); }
                            ui.separator();
                        }
                        all |= icon_btn(ui, Ic::All, "STAMPA TUTTO / A GRUPPO: scegli le pagine di ogni scheda e stampa tutto insieme", has_docs);
                        q |= icon_btn(ui, Ic::Quick, "Stampa subito: scheda corrente, ultime impostazioni (Ctrl+Maiusc+P)", cur_doc.is_some());
                        pd |= icon_btn(ui, Ic::Preview, "Stampa… con anteprima (Ctrl+P)", cur_doc.is_some());
                        ui.separator();
                        if icon_btn_a(ui, Ic::Pan, "Sposta la pagina con il mouse (H)", cur_doc.is_some(), self.pan) { self.pan = true; }
                        if icon_btn_a(ui, Ic::Sel, "Seleziona testo (T)", cur_doc.is_some(), !self.pan) { self.pan = false; }
                        if icon_btn_a(ui, Ic::Thumbs, "Mini anteprime delle pagine a sinistra", cur_doc.is_some(), self.st.thumbs) { self.st.thumbs = !self.st.thumbs; }
                        let (_, drag) = ui.allocate_exact_size(Vec2::new(56.0, 28.0), egui::Sense::click_and_drag());
                        if drag.drag_started() { ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag); }
                        if drag.double_clicked() { ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maxed)); }
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.image(egui::load::SizedTexture::new(self.logo.id(), Vec2::splat(24.0)));
                            ui.menu_button(RichText::new("File").strong().color(ACCENT), |ui| {
                                ui.set_min_width(260.0);
                                if mi(ui, "Apri…", "Ctrl+O", true) { o = true; }
                                if mi(ui, "Chiudi scheda", "Ctrl+W", cur_doc.is_some()) { ctrl_w = true; }
                                if mi(ui, "Chiudi tutte le schede", "", has_docs) { close_all = true; }
                                ui.separator();
                                if mi(ui, "Copia testo selezionato", "Ctrl+C", cur_doc.is_some()) { copy_req = true; }
                                if mi(ui, "Seleziona tutto il testo", "Ctrl+A", cur_doc.is_some()) { sel_all = true; }
                                ui.separator();
                                if mi(ui, "Stampa…", "Ctrl+P", cur_doc.is_some()) { pd = true; }
                                if mi(ui, "Stampa subito", "Ctrl+Maiusc+P", cur_doc.is_some()) { q = true; }
                                if mi(ui, "STAMPA TUTTO / A GRUPPO…", "", has_docs) { all = true; }
                                ui.menu_button("Stampante", |ui| {
                                    for p in &self.printers {
                                        if ui.selectable_label(self.st.printer == *p, p.as_str()).clicked() { self.st.printer = p.clone(); ui.close_menu(); }
                                    }
                                });
                                ui.menu_button("Pagine per foglio", |ui| {
                                    for k in NUPS {
                                        if ui.selectable_label(self.st.nup == k, format!("{k} su 1")).clicked() { self.st.nup = k; ui.close_menu(); }
                                    }
                                });
                                ui.separator();
                                if mi(ui, "Schermo intero", "F11", true) { fs_toggle = true; }
                                ui.checkbox(&mut self.st.thumbs, "Mini anteprime delle pagine")
                                    .on_hover_text("Mostra a sinistra le miniature di tutte le pagine: un clic porta a quella pagina");
                                #[cfg(target_os = "macos")]
                                ui.checkbox(&mut self.st.tabs_pin, "Fissa la barra delle schede")
                                    .on_hover_text("Se non e' fissata, questa barra compare solo portando il mouse in cima alla finestra");
                                ui.checkbox(&mut self.st.background, "Apri sempre in secondo piano")
                                    .on_hover_text("Aprendo altri PDF la finestra del programma non passa davanti: non perdi il focus su quello che stai usando");
                                ui.separator();
                                if mi(ui, "Impostazioni…", "Ctrl+,", true) { self.show_cfg = true; self.cfg = self.st.clone(); }
                                if mi(ui, "Info", "", true) { show_info = true; }
                                if mi(ui, "Esci", "", true) { quit = true; }
                            });
                            { // barra di scorrimento sotto le schede: compare solo quando servono, occupa 6 px e non le copre
                                let mut sc = egui::style::ScrollStyle::solid();
                                sc.bar_width = 5.0; sc.bar_inner_margin = 1.0; sc.bar_outer_margin = 0.0;
                                ui.spacing_mut().scroll = sc;
                                tabs_avail = ui.available_width();
                            }
                            egui::ScrollArea::horizontal().id_salt("tabs").drag_to_scroll(false).auto_shrink([false, false]).show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 2.0;
                                    let mut rects: Vec<egui::Rect> = vec![];
                                    for i in 0..self.docs.len() {
                                        let sel = self.tab == Tab::Doc(i);
                                        let (r, x) = tab_widget(ui, &short(&self.docs[i].name, 24), sel, &self.docs[i].name, if self.tabs_overflow { Some(i + 1) } else { None });
                                        if x || r.middle_clicked() { close = Some(i); } else if r.clicked() { self.tab = Tab::Doc(i); }
                                        rects.push(r.rect);
                                        if r.drag_started() { self.tab_drag = Some(i); }
                                        if sel && want_scroll { r.scroll_to_me(Some(egui::Align::Center)); }
                                    }
                                    // trascinamento di una scheda: segno di inserimento e spostamento al rilascio
                                    if let Some(from) = self.tab_drag {
                                        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
                                        if let (Some(pp), false) = (ctx.input(|i| i.pointer.hover_pos().or(i.pointer.interact_pos())), rects.is_empty()) {
                                            let ins = rects.iter().filter(|r| r.center().x < pp.x).count();
                                            let x = if ins < rects.len() { rects[ins].left() - 1.0 } else { rects[rects.len() - 1].right() + 1.0 };
                                            let top = rects[0].top();
                                            ui.painter().line_segment([egui::pos2(x, top), egui::pos2(x, top + 26.0)], egui::Stroke::new(2.5, ACCENT));
                                            if !ctx.input(|i| i.pointer.primary_down()) {
                                                let to = if ins > from { ins - 1 } else { ins };
                                                if to != from { reorder = Some((from, to)); }
                                                self.tab_drag = None;
                                            }
                                        } else { self.tab_drag = None; }
                                    }
                                    tabs_w = ui.min_rect().width();
                                });
                            });
                        });
                    });
                });
            });
        }
        // barra a scomparsa: una piccola maniglia in cima segnala dove ripresentarla
        if !bar_visible {
            let r = ctx.screen_rect();
            egui::Area::new(egui::Id::new("bar_hint")).fixed_pos(egui::pos2(r.center().x - 20.0, r.top() + 3.0)).show(ctx, |ui| {
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(40.0, 4.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, 2.0, Color32::from_gray(150));
                resp.on_hover_text("Porta il mouse qui per la barra con File, stampa e schede");
            });
        }
        if quit { ctx.send_viewport_cmd(egui::ViewportCommand::Close); }
        if fs_toggle { self.fs = !self.fs; ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fs)); }
        if show_info { self.show_info = true; }
        if ctrl_w { close = cur_doc; }
        if close_all { self.docs.clear(); self.tab = Tab::Home; self.show_print = false; self.pv = None; self.pv_raw = None; }
        if let Some((from, to)) = reorder { self.move_tab(from, to); }
        if tabs_w > 0.0 { // schede troppe per la barra: compaiono i numeri sotto le schede
            let ov = tabs_w > tabs_avail + 1.0;
            if ov != self.tabs_overflow { self.tabs_overflow = ov; ctx.request_repaint(); }
        }
        if let Some(i) = close.filter(|&i| i < self.docs.len()) {
            self.docs.remove(i);
            self.tab = match self.tab { Tab::Doc(j) if j == i => Tab::Home, Tab::Doc(j) if j > i => Tab::Doc(j - 1), t => t };
            self.show_print = false; self.pv = None; self.pv_raw = None;
        }

        // ---- testo selezionabile: carica i caratteri della pagina e gestisce "seleziona tutto" ----
        if close_all || close.is_some() { self.sel = None; }
        if let Tab::Doc(i) = self.tab {
            if i < self.docs.len() {
                let cur = self.docs[i].cur;
                if self.sel.as_ref().map_or(false, |s| s.doc != i || s.page != cur) { self.sel = None; }
                let searching = !self.search.is_empty();
                if sel_all || searching || ctx.input(|i| i.pointer.primary_pressed()) { self.ensure_chars(i, cur); } // il testo si legge solo quando serve
                if searching { self.ensure_ptext(i, cur); }
                if sel_all {
                    let boxed: Vec<usize> = self.docs[i].text[&cur].iter().enumerate().filter(|(_, c)| c.r[2] - c.r[0] > 0.01 && c.r[3] - c.r[1] > 0.01).map(|(k, _)| k).collect();
                    self.sel = match (boxed.first(), boxed.last()) { (Some(&a), Some(&b)) => Some(Sel { doc: i, page: cur, a, b }), _ => { no_text = true; None } };
                }
            }
        } else { self.sel = None; }

        // ---- mini anteprime: colonna a sinistra con le pagine del documento ----
        if self.st.thumbs {
            if let Tab::Doc(i) = self.tab {
                if i < self.docs.len() {
                    let cur = self.docs[i].cur;
                    let mut go = None;
                    egui::SidePanel::left("mini_pagine").exact_width(148.0).resizable(false)
                        .frame(egui::Frame::none().fill(Color32::from_rgb(236, 237, 240)).inner_margin(egui::Margin::symmetric(6.0, 6.0)))
                        .show(ctx, |ui| {
                            let n = self.docs[i].sizes.len();
                            ui.label(RichText::new(format!("Pagine · {n}")).strong().size(13.0));
                            ui.add_space(4.0);
                            egui::ScrollArea::vertical().id_salt("mini_scroll").auto_shrink([false, false]).show_rows(ui, 166.0, n, |ui, range| {
                                for p in range {
                                    let ratio = { let (pw, ph) = self.docs[i].sizes[p]; (ph / pw).max(0.1) };
                                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 166.0), egui::Sense::click());
                                    let ctx2 = ui.ctx().clone();
                                    if let Some(t) = self.thumb(&ctx2, i, p) {
                                        let w = (rect.width() - 20.0).min(104.0);
                                        let h = (w * ratio).min(142.0);
                                        let img = egui::Rect::from_center_size(egui::pos2(rect.center().x, rect.top() + 4.0 + h / 2.0), Vec2::new(w, h));
                                        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                                        ui.painter().image(t.id(), img, uv, Color32::WHITE);
                                        let col = if p == cur { ACCENT } else { Color32::from_gray(160) };
                                        ui.painter().rect_stroke(img, 2.0, egui::Stroke::new(if p == cur { 2.0 } else { 1.0 }, col));
                                        ui.painter().text(egui::pos2(rect.center().x, img.bottom() + 9.0), egui::Align2::CENTER_CENTER,
                                            format!("{}", p + 1), egui::FontId::proportional(11.0), if p == cur { ACCENT } else { Color32::from_gray(90) });
                                    }
                                    if resp.clicked() { go = Some(p); }
                                    resp.on_hover_text(format!("Vai a pagina {}", p + 1));
                                }
                            });
                        });
                    if let Some(p) = go {
                        self.docs[i].cur = p;
                        self.sel = None;
                        self.scroll_reset = Some(false);
                    }
                }
            }
        }

        // ---- area documento: occupa tutto lo spazio rimasto ----
        egui::CentralPanel::default().frame(egui::Frame::none().fill(Color32::from_rgb(107, 114, 128))).show(ctx, |ui| match self.tab {
            Tab::Home => {
                egui::Frame::none().fill(Color32::WHITE).show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    ui.vertical_centered(|ui| {
                        ui.add_space(80.0);
                        ui.image(egui::load::SizedTexture::new(self.logo.id(), Vec2::splat(96.0)));
                        ui.heading("PDF Reader CP");
                        ui.label("Trascina qui un PDF oppure premi Apri.");
                        if ui.button("Apri PDF…").clicked() { o = true; }
                    });
                });
            }
            Tab::Doc(i) => {
                if i >= self.docs.len() { return; }
                let (cur, zoom, fp) = (self.docs[i].cur, self.docs[i].zoom, self.docs[i].fitpage);
                let (pw, ph) = self.docs[i].sizes[cur];
                let av = ui.available_size();
                let base = if fp { (av.x - 16.0).min((av.y - 8.0) * pw / ph) } else { av.x - 16.0 };
                let w = (base * zoom).max(50.0);
                let h = w * ph / pw;
                let sz = (w as i32, h as i32);
                if sz != self.last_size { self.last_size = sz; self.last_change = Instant::now(); }
                let bucket = (((w * ctx.pixels_per_point()) as i32).clamp(800, 3200) + 399) / 400 * 400;
                if let Some(t) = self.get_tex(ctx, i, cur, bucket) {
                    let empty: Vec<TCh> = Vec::new();
                    let loaded = self.docs[i].text.contains_key(&cur);
                    let chars = self.docs[i].text.get(&cur).unwrap_or(&empty);
                    let ptext = self.docs[i].ptext.get(&cur);
                    let sel = &mut self.sel;
                    let pan = self.pan;
                    let shit = self.search_hit;
                    let q: Vec<char> = self.search.chars().map(|c| c.to_lowercase().next().unwrap_or(c)).collect();
                    // salto al risultato della ricerca: lo porta in vista
                    let target_y = if self.jump { shit.filter(|(p, _)| *p == cur).and_then(|(_, st)| chars.get(st)).filter(|c| c.r[3] > c.r[1]).map(|c| ((ph - c.r[3]) / ph * h - 100.0).max(0.0)) } else { None };
                    let mut sa = egui::ScrollArea::both().id_salt((i, cur)).auto_shrink([false; 2]).drag_to_scroll(pan);
                    if let Some(y) = target_y { sa = sa.vertical_scroll_offset(y); }
                    else if let Some(bottom) = self.scroll_reset.take() { sa = sa.vertical_scroll_offset(if bottom { 1.0e9 } else { 0.0 }); }
                    let out = sa.show(ui, |ui| {
                        ui.add_space(8.0);
                        let img = egui::Image::new(egui::load::SizedTexture::new(t.id(), Vec2::new(w, h))).sense(if pan { egui::Sense::click() } else { egui::Sense::click_and_drag() });
                        let resp = ui.vertical_centered(|ui| ui.add(img)).inner;
                        let to_pt = |p: egui::Pos2| ((p.x - resp.rect.min.x) / w * pw, ph - (p.y - resp.rect.min.y) / h * ph);
                        let rect_of = |c: &TCh| egui::Rect::from_min_max(
                            egui::pos2(resp.rect.min.x + c.r[0] / pw * w, resp.rect.min.y + (ph - c.r[3]) / ph * h),
                            egui::pos2(resp.rect.min.x + c.r[2] / pw * w, resp.rect.min.y + (ph - c.r[1]) / ph * h));
                        if resp.hovered() {
                            if pan { ctx.set_cursor_icon(if ui.input(|i| i.pointer.primary_down()) { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab }); }
                            else if !loaded || !chars.is_empty() { ctx.set_cursor_icon(egui::CursorIcon::Text); }
                        }
                        if !pan {
                            if resp.drag_started() {
                                if loaded && chars.is_empty() { no_text = true; }
                                *sel = resp.interact_pointer_pos().and_then(|p| { let (x, y) = to_pt(p); hit(chars, x, y) }).map(|k| Sel { doc: i, page: cur, a: k, b: k });
                            } else if resp.dragged() {
                                if let (Some(p), Some(s)) = (resp.interact_pointer_pos(), sel.as_mut()) {
                                    let (x, y) = to_pt(p);
                                    if let Some(k) = hit(chars, x, y) { s.b = k; }
                                }
                            } else if resp.clicked() { *sel = None; }
                        }
                        // risultati della ricerca (giallo; quello corrente arancione)
                        if let (false, Some(tx)) = (q.is_empty(), ptext) {
                            let cur_hit = shit.filter(|(p, _)| *p == cur).map(|(_, st)| st);
                            for st in find_all(tx, &q) {
                                let col = if Some(st) == cur_hit { Color32::from_rgba_unmultiplied(255, 140, 0, 150) } else { Color32::from_rgba_unmultiplied(255, 214, 0, 110) };
                                for c in chars.iter().skip(st).take(q.len()) { if c.r[2] - c.r[0] >= 0.01 { ui.painter().rect_filled(rect_of(c), 0.0, col); } }
                            }
                        }
                        // testo selezionato
                        if let Some(s) = sel.as_ref() {
                            let (lo, hi) = (s.a.min(s.b), s.a.max(s.b));
                            for c in chars.iter().skip(lo).take(hi + 1 - lo) { if c.r[2] - c.r[0] >= 0.01 { ui.painter().rect_filled(rect_of(c), 0.0, Color32::from_rgba_unmultiplied(51, 144, 255, 90)); } }
                        }
                        resp.context_menu(|ui| {
                            if ui.button("Copia").clicked() { copy_req = true; ui.close_menu(); }
                            if ui.button("Seleziona tutto").clicked() {
                                let mut it = chars.iter().enumerate().filter(|(_, c)| c.r[2] - c.r[0] > 0.01 && c.r[3] - c.r[1] > 0.01).map(|(k, _)| k);
                                if let Some(a) = it.next() { let b = it.last().unwrap_or(a); *sel = Some(Sel { doc: i, page: cur, a, b }); }
                                ui.close_menu();
                            }
                        });
                    });
                    if target_y.is_some() { self.jump = false; }
                    // scorrendo oltre il fondo (o l'inizio) della pagina con rotella, touchpad o mano si passa alla pagina successiva (o precedente)
                    let max_off = (out.content_size.y - out.inner_rect.height()).max(0.0);
                    let dy = ctx.input(|i| i.raw_scroll_delta.y + if pan && i.pointer.primary_down() { i.pointer.delta().y } else { 0.0 });
                    if dy != 0.0 {
                        if self.over_at.elapsed() > Duration::from_millis(450) { self.over_acc = 0.0; }
                        self.over_at = Instant::now();
                        if dy < 0.0 && self.edge.1 { self.over_acc += -dy; }
                        else if dy > 0.0 && self.edge.0 { self.over_acc -= dy; }
                        else { self.over_acc = 0.0; }
                    }
                    self.edge = (out.state.offset.y <= 0.5, out.state.offset.y >= max_off - 0.5);
                    let n_pages = self.docs[i].sizes.len();
                    if self.over_acc >= 140.0 && cur + 1 < n_pages {
                        self.docs[i].cur = cur + 1; self.over_acc = 0.0; self.edge = (true, false); ctx.request_repaint();
                    } else if self.over_acc <= -140.0 && cur > 0 {
                        self.docs[i].cur = cur - 1; self.over_acc = 0.0; self.edge = (false, true); self.scroll_reset = Some(true); ctx.request_repaint();
                    } else if self.over_acc.abs() >= 140.0 { self.over_acc = 0.0; }
                }
            }
        });

        if no_text { self.toast("Questa pagina non contiene testo selezionabile (immagine scansionata?)".into()); }
        if copy_req {
            match self.selected_text() {
                Some(t) => { ctx.copy_text(t); self.toast("Testo copiato negli appunti".into()); }
                None => self.toast("Seleziona prima del testo: trascina il mouse sulla pagina".into()),
            }
        }

        // ---- comandi pagina/zoom: barra fluttuante in basso (non toglie spazio al documento) ----
        if let Tab::Doc(i) = self.tab {
            if let Some(d) = self.docs.get_mut(i) {
                egui::Area::new(egui::Id::new("nav")).anchor(egui::Align2::CENTER_BOTTOM, [0.0, -14.0]).show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if ui.button("<").clicked() && d.cur > 0 { d.cur -= 1; }
                            ui.label(format!("{} / {}", d.cur + 1, d.sizes.len()));
                            if ui.button(">").clicked() && d.cur + 1 < d.sizes.len() { d.cur += 1; }
                            ui.separator();
                            if ui.button("-").clicked() { d.zoom = (d.zoom - 0.1).max(0.3); }
                            ui.label(format!("{:.0}%", d.zoom * 100.0));
                            if ui.button("+").clicked() { d.zoom = (d.zoom + 0.1).min(4.0); }
                            if ui.button(if d.fitpage { "Pagina" } else { "Larghezza" }).on_hover_text("Adatta a larghezza / pagina intera").clicked() { d.fitpage = !d.fitpage; d.zoom = 1.0; }
                            ui.separator();
                            let r = ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Cerca nel PDF…").desired_width(170.0));
                            if self.search_focus { r.request_focus(); self.search_focus = false; }
                            if r.changed() { self.search_hit = None; }
                            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                if ui.input(|i| i.modifiers.shift) { search_prev = true; } else { search_next = true; }
                                r.request_focus();
                            }
                            if ui.button("Prec.").clicked() { search_prev = true; }
                            if ui.button("Succ.").clicked() { search_next = true; }
                            if !self.search.is_empty() {
                                let q: Vec<char> = self.search.chars().map(|c| c.to_lowercase().next().unwrap_or(c)).collect();
                                match d.ptext.get(&d.cur).map(|t| find_all(t, &q).len()) {
                                    Some(0) => { ui.label("nessun risultato in pagina"); }
                                    Some(k) => { ui.label(format!("{k} in pagina")); }
                                    None => {}
                                }
                            }
                        });
                    });
                });
            }
        }
        if search_next || search_prev {
            if let Tab::Doc(i) = self.tab { if i < self.docs.len() { self.search_step(i, search_next); ctx.request_repaint(); } }
        }
        if let Some(t) = self.msg_at {
            if t.elapsed().as_secs_f32() < 4.0 {
                egui::Area::new(egui::Id::new("toast")).anchor(egui::Align2::LEFT_BOTTOM, [14.0, -14.0]).show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| { ui.label(&self.msg); });
                });
                ctx.request_repaint_after(Duration::from_millis(500));
            } else { self.msg_at = None; }
        }

        // ---- azioni ----
        if o { self.pick(); }
        if let Some(i) = cur_doc.filter(|&i| i < self.docs.len()) {
            if pd { self.show_print = true; self.draft = self.st.clone(); self.drv_seen.clear(); self.prev = 0; self.pv = None; self.pv_raw = None; }
            if q { self.print_one(i); }
        }
        if all { self.print_all(); }
        // ---- stampa in corso: cerchio di caricamento finche' il lavoro non e' in coda nella stampante ----
        let mut cancel_print = false;
        if let Some(t) = &self.task {
            let testo = format!("Invio alla stampante…  {}/{} fogli · scheda {}/{}", t.done.min(t.total), t.total, (t.di + 1).min(t.ids.len()), t.ids.len());
            egui::Area::new(egui::Id::new("printing")).order(egui::Order::Foreground).anchor(egui::Align2::RIGHT_BOTTOM, [-16.0, -16.0]).show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().size(20.0));
                        ui.label(RichText::new(testo).strong());
                        if ui.small_button("Annulla").clicked() { cancel_print = true; }
                    });
                });
            });
        }
        if cancel_print { if let Some(t) = self.task.as_mut() { t.cancel = true; } }
        if self.task.is_some() { self.print_step(ctx); }
        // ---- stampa a gruppo: stesso cerchio di caricamento, un solo lavoro ----
        let mut cancel_group = false;
        if let Some(g) = &self.gjob {
            let testo = format!("Gruppo alla stampante…  {}/{} fogli · {} pagine", g.done.min(g.total), g.total, g.sheets.len() * g.st.nup.max(1) as usize);
            egui::Area::new(egui::Id::new("printing_group")).order(egui::Order::Foreground).anchor(egui::Align2::RIGHT_BOTTOM, [-16.0, -56.0]).show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().size(20.0));
                        ui.label(RichText::new(testo).strong());
                        if ui.small_button("Annulla").clicked() { cancel_group = true; }
                    });
                });
            });
        }
        if cancel_group { if let Some(g) = self.gjob.as_mut() { g.cancel = true; } }
        if self.gjob.is_some() { self.group_step(ctx); }
        if self.show_pall { self.pall_window(ctx); }
        if self.show_cfg { self.settings_window(ctx); }
        if !self.show_print { self.pa_mode = false; }
        if self.show_print { if let Tab::Doc(i) = self.tab { self.print_dialog(ctx, i); } else { self.show_print = false; } }
        if self.pwd.is_some() { self.pwd_window(ctx); }
        if self.show_info {
            let mut open = true;
            egui::Window::new("Informazioni").open(&mut open).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.image(egui::load::SizedTexture::new(self.logo.id(), Vec2::splat(72.0)));
                    ui.heading("PDF Reader CP");
                    ui.label("Programma opensource progettato da Peppe CP con l'aiuto della IA.");
                    ui.label(RichText::new(format!("Versione {}", app_version())).weak());
                    ui.add_space(8.0);
                    ui.separator();
                    ui.scope(|ui| {
                        ui.set_max_width(430.0);
                        ui.label("Se vuoi contribuire al mio lavoro ti lascio in basso il pulsante per contribuire anche con una piccola donazione simbolica visto il tempo che ti farò risparmiare per stampare grossi quantitativi di pdf ;-)");
                    });
                    ui.add_space(6.0);
                    if ui.add(Button::new(RichText::new("Dona con PayPal").strong().color(Color32::WHITE)).fill(Color32::from_rgb(0, 112, 186)).min_size(Vec2::new(220.0, 38.0)))
                        .on_hover_text(DONATE_URL).clicked() {
                        ctx.output_mut(|o| o.open_url = Some(egui::OpenUrl::new_tab(DONATE_URL)));
                    }
                    ui.add_space(4.0);
                    ui.hyperlink_to("github.com/gsepp86/Pdf-Reader-CP", GITHUB_URL);
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(4.0);
                    if ui.add_enabled(self.upd.is_none(), Button::new("Cerca la versione piu' recente"))
                        .on_hover_text("Controlla su GitHub se e' stata pubblicata una versione nuova").clicked() {
                        self.check_update(ctx);
                    }
                    if self.upd.is_some() { ui.spinner(); }
                    if !self.upd_msg.is_empty() { ui.label(RichText::new(&self.upd_msg).weak()); }
                    if !self.upd_url.is_empty() { ui.hyperlink_to("Apri la pagina delle release", self.upd_url.clone()); }
                });
            });
            if !open { self.show_info = false; }
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        resize_grips(ctx); // senza cornice di sistema: bordi e angoli ridimensionano la finestra
        // su macOS la finestra ha la barra del titolo nativa, quindi il ridimensionamento e' quello di sistema
        if macos && !self.st.tabs_pin && !self.docs.is_empty() {
            // senza input la barra deve ritirarsi da sola: sveglia l'interfaccia allo scadere del tempo
            let till = self.bar_hide.max(self.bar_lit).saturating_duration_since(Instant::now());
            if till > Duration::ZERO { ctx.request_repaint_after(till.min(Duration::from_millis(200))); }
        }
        // contorno nero attorno alla finestra (la finestra non ha la cornice di Windows)
        #[cfg(not(target_os = "macos"))]
        ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("window_outline")))
            .rect_stroke(ctx.screen_rect().shrink(0.75), 0.0, egui::Stroke::new(1.5, Color32::from_gray(75)));
        if self.st != before { self.st.save(); } // stampante / "N su 1" scelti nella barra restano per "Stampa subito"
    }
}

// ---------- istanza unica: un secondo avvio passa i PDF al programma gia' aperto ----------
const IPC_ADDR: &str = "127.0.0.1:47653"; // solo localhost
const IPC_HELLO: &str = "PDFReaderCP/1";
enum Ipc { Open(PathBuf), Focus }

/// Ok(Some(listener)) = siamo la prima istanza. Err(()) = c'e' gia' un'istanza e le abbiamo passato i file: uscire.
fn single_instance(args: &[PathBuf]) -> Result<Option<std::net::TcpListener>, ()> {
    match std::net::TcpListener::bind(IPC_ADDR) {
        Ok(l) => Ok(Some(l)),
        Err(_) => if forward(args) { Err(()) } else { Ok(None) }, // porta occupata da altro: si parte comunque
    }
}

fn forward(args: &[PathBuf]) -> bool {
    let Ok(addr) = IPC_ADDR.parse::<std::net::SocketAddr>() else { return false };
    let Ok(mut s) = std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(800)) else { return false };
    let _ = s.set_read_timeout(Some(Duration::from_millis(2000)));
    #[cfg(windows)]
    unsafe { let _ = windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(u32::MAX); } // permette all'altra finestra di portarsi in primo piano
    let mut msg = format!("{IPC_HELLO}\n");
    if args.is_empty() { msg.push_str("FOCUS\n"); }
    for a in args { msg.push_str(&format!("OPEN\t{}\n", a.to_string_lossy())); }
    msg.push_str("END\n");
    if s.write_all(msg.as_bytes()).is_err() { return false; }
    let mut r = String::new();
    BufReader::new(s).read_line(&mut r).is_ok() && r.trim() == IPC_HELLO
}

fn serve(lst: std::net::TcpListener, tx: std::sync::mpsc::Sender<Ipc>, ctx: egui::Context) {
    std::thread::spawn(move || {
        for s in lst.incoming().flatten() {
            let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
            let mut rd = BufReader::new(&s);
            let mut line = String::new();
            if rd.read_line(&mut line).is_err() || line.trim() != IPC_HELLO { continue; }
            loop {
                line.clear();
                if rd.read_line(&mut line).unwrap_or(0) == 0 { break; }
                let l = line.trim_end_matches(['\r', '\n']);
                if l == "END" { break; }
                if l == "FOCUS" { let _ = tx.send(Ipc::Focus); }
                else if let Some(p) = l.strip_prefix("OPEN\t") {
                    let p = PathBuf::from(p);
                    if p.is_file() { let _ = tx.send(Ipc::Open(p)); }
                }
            }
            let _ = (&s).write_all(format!("{IPC_HELLO}\n").as_bytes());
            ctx.request_repaint();
        }
    });
}

fn main() -> eframe::Result<()> {
    // su macOS il Finder manda i file con un Apple Event (vedi `mac_open`) e sulla riga di
    // comando aggiunge solo -psn_0_...: si tengono gli argomenti che sono davvero file
    let args: Vec<PathBuf> = std::env::args().skip(1)
        .filter(|a| !a.starts_with('-'))
        .map(|a| { let p = PathBuf::from(a); std::path::absolute(&p).unwrap_or(p) })
        .filter(|p| p.is_file())
        .collect();
    #[cfg(target_os = "macos")]
    mac_open::install(); // prima di run_native: l'evento di avvio non deve andare perso
    // se il programma e' gia' aperto, i PDF si aprono li' (in nuove schede) e questo processo termina
    let listener = match single_instance(&args) { Ok(l) => l, Err(()) => return Ok(()) };
    let cfg0 = Settings::load();
    let icon = eframe::icon_data::from_png_bytes(ICON).expect("icona");
    #[allow(unused_mut)] // `vp` si riassegna solo su macOS/Linux (app_id)
    let mut vp = egui::ViewportBuilder::default().with_inner_size([1200.0, 760.0]).with_min_inner_size([800.0, 500.0]).with_maximized(cfg0.start_max).with_active(!cfg0.background)
        // su macOS si usano la barra del titolo e i pulsanti nativi della finestra (traffic lights:
        // chiudi, riduci a icona, ingrandisci/ridimensiona); sulle altre piattaforme la cornice
        // e' disegnata dal programma
        .with_decorations(cfg!(target_os = "macos"))
        .with_icon(icon).with_title("PDF Reader CP");
    #[cfg(unix)]
    {
        vp = vp.with_app_id("pdf-reader-cp"); // nome usato dal dock/launcher di Linux
    }
    let opts = eframe::NativeOptions { viewport: vp, ..Default::default() };
    eframe::run_native("PDF Reader CP", opts, Box::new(move |cc| Ok(Box::new(App::new(cc, listener, args)))))
}
