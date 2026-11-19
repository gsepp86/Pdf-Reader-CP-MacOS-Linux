# PDF Reader CP per macOS e Linux

Port della versione Windows 3.0: stesso programma (egui + PDFium), stesso look, con stampa
funzionante su macOS e Linux.

## Cosa cambia rispetto a Windows

Il codice Windows non e' stato toccato: le differenze sono tutte dietro
`#[cfg(windows)]` / `#[cfg(not(windows))]`, e il workflow di build Windows resta com'era.

| | Windows | macOS / Linux |
|---|---|---|
| Stampa | spooler GDI | CUPS (comando `lp`) |
| Interfaccia di stampa | anteprima + finestra del driver | anteprima + opzioni della coda CUPS nella finestra **Stampa** |
| `pdfium.dll` | accanto a `PDFReaderCP.exe` | `libpdfium.dylib` / `libpdfium.so` accanto all'eseguibile |
| Pacchetto | installer Inno Setup | `.app` (macOS), `.deb` + `.tar.gz` (Linux) |

## Interfaccia di stampa su Linux

La finestra **Stampa…** (Ctrl+P) e' la stessa di Windows: stampante, copie, fascicola,
intervallo pagine, pagine per foglio, scala, rotazione, centratura, colore e chiaro/scuro,
con anteprima del foglio. Si puo' spostare trascinandola per la barra del titolo e comprende
anche la **stampa poster**: una pagina ingrandita e divisa su piu' fogli (fino a 4x4) con
sovrapposizione in millimetri per unirli; nella striscia di sovrapposizione vengono stampati
i segni di taglio e la scritta R<riga>C<colonna> di ogni foglio.

La finestra **Info** mostra la versione e il pulsante **Cerca la versione piu' recente**
(release su GitHub), con il link alla pagina quando ce n'e' una nuova.

Al posto della finestra "Proprieta' driver" c'e' la sezione **Opzioni stampante (CUPS)**:
le opzioni della coda (fronte/retro, vassoio, qualita', colore, finitura...) sono lette da
`lpoptions` e si scelgono da menu a tendina direttamente nel programma. Le scelte valgono
per il lavoro inviato e vengono ricordate per quella stampante. Nessun comando da digitare.

CUPS resta il servizio di stampa del sistema (e' cosi' su Linux e su macOS): il programma
non lo evita, ma non richiede piu' di usare il terminale. Se CUPS non e' installato, la
finestra lo dice all'inizio del lavoro invece di fallire in silenzio.

## Requisiti

- **macOS 10.13+** (binario universale arm64 + x86_64)
- **Linux x86_64 / arm64**, glibc 2.35+ (Ubuntu 22.04 o successiva), CUPS (`cups-client`),
  OpenGL e librerie X11/Wayland di sistema

## Compilare

```bash
./build-unix.sh              # per il sistema corrente -> dist/
./build-unix.sh --universal  # solo macOS: binario universale
```

Lo script scarica PDFium da `bblanchon/pdfium-binaries`, compila in release e crea:

- Linux: `dist/PDFReaderCP_3.0.0_amd64.deb` e `dist/PDFReaderCP-3.0.0-linux-x64.tar.gz`
- macOS: `dist/PDFReaderCP-3.0.0-macos-universal.dmg` (installer), `dist/PDFReaderCP-3.0.0-macos-universal.pkg` (installer) e `dist/PDFReaderCP-3.0.0-macos-universal.zip` (app da scompattare)

Dipendenze per compilare su Linux:

```bash
sudo apt install libxkbcommon-dev libwayland-dev libx11-dev libxcursor-dev \
  libxrandr-dev libxi-dev libgl1-mesa-dev libegl1-mesa-dev libcups2-dev
```

`libcups2-dev` serve al link: il crate `printers` (lo stesso di Windows, in versione
Unix) collega la libreria CUPS del sistema. A runtime basta `libcups2`, gia' presente
sui sistemi con CUPS.

## Installare

**macOS (.dmg)** — apri `PDFReaderCP-3.0.0-macos-universal.dmg` e trascina `PDFReaderCP`
nel collegamento *Applications* (Applicazioni); poi espelli l'immagine.

**macOS (.pkg)** — doppio clic su `PDFReaderCP-3.0.0-macos-universal.pkg` e segui
l'installazione: l'app finisce in *Applicazioni*.

**macOS (.zip)** — in alternativa, scompatta lo zip e sposta `PDFReaderCP.app` in *Applicazioni*.

Gli installer sono firmati solo ad-hoc (nessun certificato Apple): al primo avvio fai clic
destro sull'app > *Apri*, oppure esegui
`xattr -dr com.apple.quarantine /Applications/PDFReaderCP.app`.

**Linux (.deb)**

```bash
sudo dpkg -i PDFReaderCP_3.0.0_amd64.deb
sudo apt -f install   # solo se mancano dipendenze
```

**Linux (tar.gz)** — estrai dove preferisci e avvia `./PDFReaderCP` tenendo `libpdfium.so`
nella stessa cartella.

## Percorsi: differenze tra Windows, macOS e Linux

Sono state evitate le assunzioni tipiche di Windows, che su Linux si rompono:

- **Nessuna unita' o lettera di disco**: tutto passa da `PathBuf`/`Path::join`, quindi
  `/home/utente/file.pdf` e `/Users/utente/file.pdf` funzionano senza modifiche.
- **Maiuscole/minuscole**: Linux distingue `Documento.PDF` da `documento.pdf`. Il filtro
  "Apri PDF" accetta sia `.pdf` sia `.PDF`; due file con nome che differisce solo per le
  maiuscole restano due schede distinte (su Windows sarebbero lo stesso file).
- **Separatori**: nessun `\` nel codice o negli script; i pacchetti usano percorsi `/`
  (`Contents/MacOS`, `usr/lib/pdfreadercp`, `share/applications`).
- **Cartella dei dati**: `dirs` sceglie da solo `~/.config/PDFReaderCP` su Linux e
  `~/Library/Application Support/PDFReaderCP` su macOS.
- **File temporanei**: `std::env::temp_dir()` rispetta `TMPDIR`.
- **Libreria PDFium**: cercata accanto all'eseguibile (dentro `PDFReaderCP.app/Contents/MacOS`
  su macOS), con ricaduta sulla libreria di sistema.

## Firma e notarizzazione (opzionale)

Per distribuire su macOS senza il passaggio *clic destro > Apri* servono un certificato
*Developer ID Application*, `codesign --options runtime` e `notarytool`. In quel caso
sostituisci la firma ad-hoc in `build-unix.sh` e aggiungi i segreti al workflow.

## Release automatica

`.github/workflows/release-unix.yml` (separato dal workflow Windows `blank.yml`):

- trigger: tag `v*` oppure avvio manuale (*Run workflow*);
- job **Linux x86_64** su `ubuntu-22.04` (glibc compatibile con le distro recenti);
- job **macOS universale** su `macos-14` (arm64 + x86_64 con `lipo`);
- artefatti in *Artifacts*; con un tag finiscono anche nella Release.

## Note

- Istanza unica sulla porta locale `127.0.0.1:47653`, come su Windows.
- I PDF protetti da password chiedono la password in una finestra di dialogo solo quando il
  file e' davvero protetto (stesso comportamento di Windows).
- Cmd/Ctrl sono gestiti da egui (`Modifiers::COMMAND`): su macOS valgono Cmd+O, Cmd+P, ecc.
- La finestra e' senza cornice di sistema: su macOS/Linux bordi e angoli sono trascinabili
  per ridimensionarla; su Windows il comportamento resta quello originale.
