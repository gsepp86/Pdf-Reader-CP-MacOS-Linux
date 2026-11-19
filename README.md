# PDF Reader CP 3.0
Programma opensource progettato da Peppe CP con l'aiuto della IA.

Lettore PDF a schede in Rust (egui + PDFium): una sola barra in alto con schede e comandi di stampa, documento a tutto schermo.

## Creare l'installer per Windows
**Metodo 1 - sul tuo PC (Windows 10/11 x64):** installa Rust (https://rustup.rs), poi doppio clic su `build.bat`.
Scarica da solo pdfium.dll e Inno Setup e produce `dist\PDFReaderCP_Setup_3.0.exe`.

**Metodo 2 - senza installare nulla:** carica la cartella su GitHub, poi Actions > "Crea installer" > Run workflow.
L'installer si scarica dagli "Artifacts" a fine lavoro.

## Uso
- Barra del titolo (una sola riga): **File** | schede (tutto lo spazio disponibile) | icone **Stampa…**, **Stampa subito**, **STAMPA TUTTO** | riduci / massimizza / chiudi.
- Menu **File**: Apri, Chiudi scheda / tutte, Stampa…, Stampa subito, STAMPA TUTTO, Stampante, Pagine per foglio, Schermo intero, **Info**, Esci.
- **Info**: mostra la versione (3.0) e il pulsante **Cerca la versione piu' recente**, che controlla le release su GitHub senza bloccare il programma e, se ce n'e' una nuova, offre il link alla pagina delle release.
- **Stampa…** (Ctrl+P): anteprima dettagliata (1, 2, 4, 6, 9, 16 pagine per foglio), copie, fascicola, intervallo, scala, rotazione, centratura. La finestra si sposta trascinando la barra del titolo.
- **Stampa subito** (Ctrl+Maiusc+P): scheda corrente con le ultime impostazioni, senza anteprima.
- **STAMPA TUTTO / A GRUPPO**: finestra grande con l'elenco delle schede aperte; accanto a ogni file si scrivono le pagine da stampare (vuoto = tutte). In alto i totali: pagine totali da stampare, fogli per copia, copie e totale fogli. Due pulsanti: **STAMPA A GRUPPO** (un solo lavoro, con le pagine scelte impaginate insieme: la pagina 1 del file 1 e la pagina 3 del file 2 possono stare sullo stesso foglio, per esempio 2 su 1) e **STAMPA TUTTO** (ogni file resta un lavoro separato con tutte le sue pagine, come nella stampa standard, utile per la fascicolazione del driver). Il tasto si raggiunge dall'icona con le pagine impilate o da File > STAMPA TUTTO / A GRUPPO...
- **Preferenze del driver**: scegliendo una stampante nella finestra Stampa, colore (per esempio bianco e nero se la stampante e' impostata cosi'), copie, pagine per foglio e carta vengono presi dalle preferenze attuali del driver/CUPS; le modifiche fatte a mano restano valide fino al cambio di stampante.
- Molti PDF: Ctrl+Tab / Ctrl+Maiusc+Tab cambiano scheda, Ctrl+W la chiude, clic con la rotellina sulla scheda la chiude, rotellina del mouse sulla barra scorre le schede.
- F11 schermo intero · frecce / PagSu / PagGiu cambiano pagina · Ctrl+rotellina zoom · trascina i PDF nella finestra.
- **PDF protetti da password**: si aprono come gli altri; la finestra di dialogo della password compare **solo** quando il file e' davvero protetto, senza bloccare il programma. Se la password e' errata lo dice e si puo' correggere (Invio conferma, Esc annulla). I file aperti mentre la richiesta e' in corso vengono accodati.
- **Istanza unica**: se il programma e' gia' aperto, un altro PDF (doppio clic, "Apri con", riga di comando) si apre in una nuova scheda della stessa finestra, che torna in primo piano. Usa la porta locale 127.0.0.1:47653.
- La finestra si apre sempre ingrandita. Ogni scheda ha la sua "x" di chiusura integrata (clic con la rotellina = chiudi).
- Stampa...: scala **Adatta alla pagina**, **Dimensioni reali (100%)** oppure **Proporzione personalizzata** (25-200 %); con piu' pagine per foglio la scala e' automatica.
- **Testo selezionabile**: trascina il mouse sul testo della pagina, poi Ctrl+C (o tasto destro > Copia). Ctrl+A seleziona tutto il testo della pagina. I PDF scansionati (solo immagini) non hanno testo da selezionare.
- Con il testo selezionato non si trascina piu' la pagina col mouse: usa rotellina e barre di scorrimento.
- **Stampa...**: pulsante **Impostazioni driver stampante...** (proprieta' del driver: fronte/retro, vassoio, qualita', colore, finitura...). Le scelte del driver vengono usate nella stampa e ricordate per quella stampante; la carta dell'anteprima segue quella del driver.
- **Colore e luminosita'**: Colori / Scala di grigi / Bianco e nero e cursore Scuro-Chiaro (valido per anteprima e stampa).
- **Copie**: pulsanti - e + accanto al numero. Con piu' di una copia il programma chiede conferma ("Sicuro di voler stampare N copie del file?", oppure "...dei file aperti?" con STAMPA TUTTO).
- **Fluidita'**: i PDF restano aperti in memoria; durante ridimensionamento e zoom la pagina non viene ridisegnata a ogni passo (si affina appena ti fermi); nell'anteprima di stampa il cursore chiaro/scuro e la scelta del colore non ricompongono il foglio.
- **Strumenti** (accanto alle icone di stampa): **Seleziona testo** (T) e **Sposta la pagina con il mouse** (H, trascinando la pagina).
- **Ricerca nel PDF**: campo "Cerca nel PDF" nella barra in basso (Ctrl+F). Invio = risultato successivo, Maiusc+Invio = precedente; i risultati sono evidenziati in giallo (quello corrente in arancione) e la ricerca passa da una pagina all'altra.
- **STAMPA TUTTO / A GRUPPO**: se sono impostate piu' copie avvisa in stampatello; i pulsanti **1 COPIA** (corregge e stampa), **CAMBIA IMPOSTAZIONI** (colore, N su 1, stampante, driver, poster) e **ANNULLA** sono nella stessa finestra.
- **Finestra**: si sposta trascinando qualsiasi spazio libero della barra in alto (anche tra le schede e accanto all'icona); doppio clic sulla barra = ingrandisci/ripristina. Le stampanti sono elencate in ordine alfabetico.
- **Impostazioni** (File > Impostazioni..., Ctrl+,): finestra con due sezioni, Generale (apri sempre in secondo piano, avvia ingrandita, strumento e vista iniziali) e Stampa (stampante, copie, pagine per foglio, colore, conferma con piu' copie). Si salvano con "Salva" nel file settings.json.
- **In secondo piano**: con l'opzione attiva (anche da File > Apri sempre in secondo piano) la finestra parte senza prendere il focus e i PDF aperti da un secondo avvio si aggiungono come schede senza portarla davanti.
- **Stampa poster**: nella finestra Stampa... l'opzione *Stampa poster (una pagina su piu' fogli)* ingrandisce una sola pagina e la divide su piu' fogli (griglie 1x2, 2x1, 2x2, 3x2, 2x3, 3x3, 4x4) con sovrapposizione regolabile in millimetri sui lati interni, per unire i fogli senza tagliare il disegno. L'anteprima mostra il singolo foglio con riga e colonna nella griglia; con il poster sono disattivate le pagine per foglio e la scala (la pagina e' adattata all'intero poster). Nella striscia di sovrapposizione ogni foglio riporta i **segni di taglio** (due tacche per lato interno) e la scritta **R<riga>C<colonna>** per riconoscere e montare i fogli; con sovrapposizione 0 mm i segni non vengono stampati.
- **Intervallo personalizzato**: si possono ripetere le pagine, per esempio `1,1` (pagina 1 due volte, utile con 2 su 1), `2x3` (pagina 2 tre volte) o `1-3x2` (ogni pagina due volte).
- **Schede**: la barra di scorrimento compare sotto le schede solo quando sono troppe.

## Se vuoi contribuire al mio lavoro ti lascio in basso il pulsante per contribuire anche con una piccola donazione simbolica visto il tempo che ti farò risparmiare per stampare grossi quantitativi di pdf ;-)

[![Donate via PayPal](https://img.shields.io/badge/Donate-PayPal-blue.svg?logo=paypal)](https://paypal.me/PeppeCP)

https://github.com/gsepp86/Pdf-Reader-CP
- **STAMPA TUTTO / A GRUPPO**: riepilogo sempre visibile (schede, pagine in totale, pagine scelte per ogni file, fogli per copia, copie, totale fogli, stampante, colore) e pagine modificabili file per file nella stessa finestra.
- **Cambio pagina scorrendo**: con la rotella, il touchpad o lo strumento mano, arrivati in fondo alla pagina si continua sulla successiva (e in alto sulla precedente), senza cliccare i pulsanti.
- **Schede tante**: quando non entrano nella barra compaiono piccoli numeri 1, 2, 3... sotto ogni scheda.
- **Finestra di stampa**: la colonna delle impostazioni (compreso il cursore Scuro-Chiaro in "Colore e luminosita'") usa piu' altezza e ha una barra di scorrimento visibile se serve. La finestra **Stampa** (e **STAMPA TUTTO**) si puo' spostare trascinandola per la barra del titolo, cosi' si vede il PDF che sta sotto.
- **Stampa in background**: durante l'invio di molti file compare in basso a destra un cerchio di caricamento con l'avanzamento (fogli e schede) e il pulsante Annulla; sparisce quando il lavoro e' in coda nella stampante.
- **Schede spostabili**: trascina una scheda per cambiarne la posizione; STAMPA TUTTO stampa le schede nell'ordine in cui sono disposte.
- **Contorno grigio scuro** attorno alla finestra.
- **Impostazioni di stampa all'avvio** (Impostazioni > Stampa): si sceglie tra *ultime impostazioni utilizzate* e *le mie impostazioni predefinite* (stampante, copie, colore e funzioni del driver come il fronte/retro). All'avvio il programma ripristina sempre la stampa standard (1 pagina per foglio, tutte le pagine, scala adatta, 1 copia) e usa le impostazioni predefinite del driver della stampante, per esempio il fronte/retro se e' impostato come predefinito nel driver (si possono disattivare entrambe le opzioni).
- Il contorno della finestra e' grigio scuro.
