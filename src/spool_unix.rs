// Stampa su macOS e Linux tramite CUPS.
//
// Il foglio viene composto dal programma a 300 dpi (come su Windows) e salvato in PNG;
// alla chiusura del lavoro i fogli vengono inviati a CUPS con UN comando `lp`, nello
// stesso ordine calcolato da `PrintTask::advance` (copie e "fascicola" compresi).
// Se il lavoro viene annullato prima della chiusura non parte nulla.
//
// L'interfaccia di stampa del programma mostra le opzioni della coda CUPS (fronte/retro,
// vassoio, qualita'...) lette da `lpoptions`: si scelgono nella finestra, senza comandi
// da terminale. Le scelte valgono per il singolo lavoro (`lp -o chiave=valore`) e sono
// ricordate per stampante, come le impostazioni del driver su Windows.
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Job {
    dest: String, // nome della coda CUPS (non il nome "amichevole" mostrato nell'interfaccia)
    name: String,
    dir: PathBuf,
    paper: (f32, f32),
    opts: Vec<(String, String)>,
    sheets: RefCell<Vec<PathBuf>>,
    same: RefCell<HashMap<u64, PathBuf>>, // copie dello stesso foglio: stesso PNG
    next: Cell<usize>,
    aborted: bool,
}

/// Una opzione della coda CUPS con i valori ammessi e quello attivo.
#[derive(Clone)]
pub struct Opt {
    pub key: String,
    pub label: String,
    pub values: Vec<String>,
    pub current: String,
}

thread_local! {
    static CACHE: RefCell<(String, Vec<Opt>)> = RefCell::new((String::new(), Vec::new()));
    static DEST: RefCell<Option<Vec<(String, String)>>> = RefCell::new(None); // (nome mostrato, nome coda)
}

/// Nome della coda CUPS. L'interfaccia mostra il nome "amichevole" del driver
/// (es. «SHARP MX-3060N (6504934500)»), ma `lp`, `lpstat` e `lpoptions` accettano solo il
/// nome della coda («SHARP_MX-3060N»): senza questa conversione lpstat risponde
/// «Invalid destination name in list» e la stampa non parte.
fn dest(printer: &str) -> String {
    let p = printer.trim();
    if p.is_empty() { return String::new(); }
    DEST.with(|c| {
        if c.borrow().is_none() {
            *c.borrow_mut() = Some(printers::get_printers().into_iter().map(|x| (x.name, x.system_name)).collect());
        }
        c.borrow().as_ref()
            .and_then(|l| l.iter().find(|(n, _)| n == p).map(|(_, s)| s.clone()))
            .unwrap_or_else(|| p.to_string())
    })
}

/// Opzioni della coda (lette una volta per stampante: `lpoptions -d stampante -l`).
pub fn options(printer: &str) -> Vec<Opt> {
    CACHE.with(|c| {
        {
            let b = c.borrow();
            if b.0 == printer {
                return b.1.clone();
            }
        }
        let list = read_options(printer);
        *c.borrow_mut() = (printer.to_string(), list.clone());
        list
    })
}

fn read_options(printer: &str) -> Vec<Opt> {
    let dest = dest(printer);
    let Ok(out) = Command::new("lpoptions").args(["-d", dest.as_str(), "-l"]).output() else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    let defaults = queue_defaults(printer);
    let mut list = Vec::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let Some((head, rest)) = line.trim().split_once(':') else { continue };
        let (key, label) = match head.split_once('/') {
            Some((k, l)) => (k.trim(), l.trim()),
            None => (head.trim(), head.trim()),
        };
        let (mut values, mut def) = (Vec::new(), String::new());
        for tok in rest.split_whitespace() {
            match tok.strip_prefix('*') {
                Some(d) => {
                    values.push(d.to_string());
                    def = d.to_string();
                }
                None => values.push(tok.to_string()),
            }
        }
        if key.is_empty() || values.is_empty() {
            continue;
        }
        let current = defaults.get(key).cloned().unwrap_or(def);
        list.push(Opt { key: key.to_string(), label: label.to_string(), values, current });
    }
    list
}

/// Preferenze attuali del driver per la stampa (colore, copie, pagine per foglio):
/// servono ad allineare le scelte della finestra di stampa a come e' impostata la stampante.
#[derive(Default)]
pub struct Defaults {
    /// Some(true) = stampa in bianco e nero/scala di grigi, Some(false) = a colori.
    pub color: Option<bool>,
    pub copies: Option<u32>,
    pub nup: Option<u32>,
}

pub fn defaults(printer: &str) -> Defaults {
    let m = queue_defaults(printer);
    let get = |k: &str| m.iter().find(|(a, _)| a.eq_ignore_ascii_case(k)).map(|(_, v)| v.to_ascii_lowercase());
    let mut d = Defaults::default();
    if let Some(v) = get("print-color-mode") {
        if v.contains("mono") || v.contains("gray") { d.color = Some(true); }
        else if v.contains("color") { d.color = Some(false); }
    }
    if d.color.is_none() {
        if let Some(v) = get("ColorModel") {
            d.color = Some(matches!(v.as_str(), "gray" | "g" | "k" | "mono" | "monochrome"));
        }
    }
    if let Some(v) = get("copies") { d.copies = v.parse::<u32>().ok().filter(|c| *c > 0); }
    if let Some(v) = get("number-up") { d.nup = v.parse::<u32>().ok(); }
    d
}

/// Valori attivi della coda (`lpoptions -d stampante`).
fn queue_defaults(printer: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    let dest = dest(printer);
    if let Ok(out) = Command::new("lpoptions").args(["-d", dest.as_str()]).output() {
        for w in String::from_utf8_lossy(&out.stdout).split_whitespace() {
            if let Some((k, v)) = w.split_once('=') {
                m.insert(k.to_string(), v.to_string());
            }
        }
    }
    m
}

/// Opzioni scelte dall'utente, salvate in `drv_mode` come testo UTF-8 "chiave=valore;...".
pub fn saved(devmode: Option<&[u8]>) -> Vec<(String, String)> {
    let Some(d) = devmode else { return Vec::new() };
    let Ok(s) = std::str::from_utf8(d) else { return Vec::new() };
    s.split(';')
        .filter_map(|p| p.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .filter(|(k, v)| !k.is_empty() && !v.is_empty())
        .collect()
}

/// Dimensione della carta (punti): prima la scelta fatta nella finestra di stampa,
/// poi `PageSize` della coda, altrimenti A4.
pub fn paper_pts(printer: &str, devmode: Option<&[u8]>) -> Option<(f32, f32)> {
    if let Some((_, v)) = saved(devmode).into_iter().find(|(k, _)| k == "PageSize") {
        return Some(media_pts(&v));
    }
    let dest = dest(printer);
    let out = Command::new("lpoptions").args(["-d", dest.as_str()]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let txt = String::from_utf8_lossy(&out.stdout).to_string();
    let page = txt
        .split_whitespace()
        .find_map(|w| w.strip_prefix("PageSize="))
        .map(|v| v.split('/').next().unwrap_or(v).to_string());
    Some(media_pts(page.as_deref().unwrap_or("")))
}

fn media_pts(name: &str) -> (f32, f32) {
    match name.trim().to_ascii_lowercase().as_str() {
        "a3" => (842.0, 1191.0),
        "a5" => (420.0, 595.0),
        "a6" => (298.0, 420.0),
        "letter" | "na_letter" => (612.0, 792.0),
        "legal" | "na_legal" => (612.0, 1008.0),
        "tabloid" | "ledger" | "na_ledger" => (792.0, 1224.0),
        _ => (595.0, 842.0), // A4
    }
}

fn unique() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// Firma veloce del foglio: due copie identiche usano lo stesso file.
fn digest(d: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in d {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn write_png(path: &Path, rgba: &[u8], w: u32, h: u32) -> Result<(), String> {
    let mut rgb = Vec::with_capacity(w as usize * h as usize * 3);
    for px in rgba.chunks_exact(4) {
        rgb.extend_from_slice(&px[..3]);
    }
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut wr = enc.write_header().map_err(|e| e.to_string())?;
    wr.write_image_data(&rgb).map_err(|e| e.to_string())?;
    wr.finish().map_err(|e| e.to_string())?;
    Ok(())
}

impl Job {
    pub fn new(printer: &str, name: &str, devmode: Option<&[u8]>) -> Result<Job, String> {
        if printer.trim().is_empty() {
            return Err("Nessuna stampante selezionata".into());
        }
        // CUPS presente? La stampante esiste ed e' abilitata?
        let dest = dest(printer);
        let st = Command::new("lpstat").args(["-p", dest.as_str()]).output().map_err(|_| {
            "Comandi di stampa non trovati: installa CUPS (Linux: pacchetto cups-client)".to_string()
        })?;
        if !st.status.success() {
            let e = String::from_utf8_lossy(&st.stderr).trim().to_string();
            return Err(if e.is_empty() {
                format!("La stampante «{printer}» non e' disponibile")
            } else {
                format!("La stampante «{printer}» non e' disponibile: {e}")
            });
        }
        let dir = std::env::temp_dir().join(format!("pdfreadercp-{}-{}", std::process::id(), unique()));
        std::fs::create_dir_all(&dir).map_err(|e| format!("Cartella temporanea non creabile: {e}"))?;
        Ok(Job {
            dest,
            name: name.to_string(),
            dir,
            paper: paper_pts(printer, devmode).unwrap_or((595.0, 842.0)),
            opts: saved(devmode),
            sheets: RefCell::new(Vec::new()),
            same: RefCell::new(HashMap::new()),
            next: Cell::new(0),
            aborted: false,
        })
    }

    /// Carta in punti e risoluzione con cui comporre i fogli.
    pub fn area(&self) -> (f32, f32, f32) {
        (self.paper.0, self.paper.1, 300.0)
    }

    /// Annulla il lavoro: non viene mandato alla stampante.
    pub fn abort(&mut self) {
        self.aborted = true;
    }

    /// Un foglio (RGBA, gia' composto dal programma) diventa un PNG da inviare a CUPS.
    pub fn page(&self, rgba: &[u8], w: i32, h: i32, _x: i32, _y: i32, _dw: i32, _dh: i32) {
        if w <= 0 || h <= 0 || rgba.len() < w as usize * h as usize * 4 {
            return;
        }
        let key = digest(rgba);
        if let Some(p) = self.same.borrow().get(&key) {
            self.sheets.borrow_mut().push(p.clone());
            return;
        }
        let idx = self.next.get();
        self.next.set(idx + 1);
        let path = self.dir.join(format!("foglio-{idx:05}.png"));
        match write_png(&path, rgba, w as u32, h as u32) {
            Ok(()) => {
                self.same.borrow_mut().insert(key, path.clone());
                self.sheets.borrow_mut().push(path);
            }
            Err(e) => eprintln!("PDF Reader CP: foglio non salvato: {e}"),
        }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        let sheets: Vec<PathBuf> = self.sheets.borrow().clone();
        if !self.aborted && !sheets.is_empty() {
            let name = if self.name.is_empty() { "PDF Reader CP".to_string() } else { self.name.clone() };
            let mut cmd = Command::new("lp");
            cmd.arg("-d").arg(&self.dest).arg("-t").arg(name);
            cmd.arg("-o").arg("fit-to-page"); // il foglio (gia' impaginato) riempie la carta
            cmd.arg("-o").arg("copies=1"); // le copie sono gia' nell'ordine dei fogli
            for (k, v) in &self.opts {
                if k != "copies" {
                    cmd.arg("-o").arg(format!("{k}={v}"));
                }
            }
            match cmd.args(&sheets).output() {
                Ok(o) if o.status.success() => {}
                Ok(o) => eprintln!("PDF Reader CP: stampa non riuscita: {}", String::from_utf8_lossy(&o.stderr).trim()),
                Err(e) => eprintln!("PDF Reader CP: comando `lp` non eseguibile: {e}"),
            }
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
