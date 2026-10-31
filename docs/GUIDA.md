# Guida all'uso di Aurora

Questa guida spiega come usare gli editor di Aurora con mouse, tastiera e menu.

## Piano roll

- Clic su uno spazio vuoto: aggiunge una nota (la durata si sceglie nella barra degli strumenti).
- Clic su una nota: la seleziona. Ctrl o Maiusc + clic aggiunge o toglie note dalla selezione.
- Trascinamento su uno spazio vuoto: selezione a riquadro, comprese le note sovrapposte.
- Trascinamento di una nota: la sposta. Trascinando il bordo destro ne cambi la durata.
- Canc o Backspace: elimina le note selezionate. Doppio clic o clic destro elimina una sola nota.
- Corsia delle velocity: trascina per impostare la velocity.

### Quantize, Humanize e Snap to key

I tre pulsanti della barra degli strumenti lavorano sulle note selezionate.
Se non c'è nessuna selezione, lavorano su tutte le note della regione.

- **Quantize** allinea le note alla griglia di snap scelta nella barra.
- **Humanize** sposta un poco i tempi e cambia un poco le velocity, per un suono meno meccanico.
- **Snap to key** porta ogni nota alla nota più vicina della tonalità del progetto.

Dopo il comando le stesse note restano selezionate, anche se sono cambiate di posto. Se vuoi
correggere solo due battute, selezionale e premi il pulsante: il resto non si muove.
Ogni operazione si annulla con Ctrl+Z.

La selezione resta sulle stesse note anche dopo Annulla e Ripeti (Ctrl+Z, Ctrl+Y) e quando
l'assistente o un client MCP modificano la regione: le note che non esistono più escono dalla
selezione. Se selezioni un'altra regione, la selezione delle note si svuota.

Anche l'assistente AI vede quali note sono selezionate. Puoi dirgli, per esempio, "quantizza
queste note" e lui agirà solo sulla selezione.

## Corsia marker

La corsia marker sta sotto il righello dell'arrangiamento e contiene le sezioni (marker con una
lunghezza, in blu) e i marker semplici (un punto, in marrone).

- Clic su un marker: sposta il playhead all'inizio del marker.
- Clic destro su una sezione: **Set cycle to section** imposta il ciclo sulla sezione,
  **Repeat section after itself** ripete la sezione subito dopo, **Delete marker** elimina il marker.
- Trascinamento di un marker: lo sposta seguendo lo snap della barra degli strumenti. Il marker non
  va prima dell'inizio della canzone.
- Trascinamento del bordo destro di una sezione: ne cambia la lunghezza. La fine si aggancia alla
  griglia e la sezione dura almeno un battito. I marker semplici non hanno bordo da trascinare.
- Doppio clic su un marker: apre un campo di testo per rinominarlo. Invio o un clic fuori
  confermano, Esc annulla. Mentre scrivi, le scorciatoie da tastiera sono spente.
- Doppio clic sulla corsia vuota: crea una sezione di 4 battute.
- Ogni spostamento, ridimensionamento o rinomina si annulla con un solo Ctrl+Z.

Spostare un marker non sposta le regioni che ci stanno sotto. Per copiare o ripetere il materiale
di una sezione usa il menu del clic destro.

## Filmato

Puoi collegare un filmato al progetto per scrivere la musica a tempo con l'immagine. Il filmato deve
essere un file su un disco locale: percorsi di rete (`\\server\cartella`), indirizzi web e percorsi
che cominciano con `-` vengono rifiutati. Se il file sta su una condivisione di rete, copialo in
locale o collegala a una lettera di unità.

- Se un progetto aperto contiene un percorso del filmato non valido, la finestra Movie mostra una
  riga rossa al posto dell'immagine. Ricarica il filmato da un file locale.
- **Export Movie** scrive un file con estensione `.mp4`, `.mov`, `.mkv` o `.m4v`. Con un'altra
  estensione (per esempio `.avi`) o senza estensione, la barra di stato mostra un errore e non viene
  scritto nulla.
- I fotogrammi e l'export richiedono ffmpeg.

## Bounce

- **File > Bounce to WAV…** (Ctrl+B) scrive il mix in un file WAV: tutta la canzone, oppure solo
  l'intervallo del ciclo se il ciclo è attivo.
- Dopo la fine, Aurora continua finché le code di riverbero e delay non si spengono (sotto circa
  −90 dBFS): almeno 2 secondi, al massimo 20. Così il finale suona come in riproduzione. Vale anche
  per Bounce in Place, Bounce Surround e Bounce Binaural.
- Un suono che non si spegne mai, per esempio una nota tenuta oltre la fine o un plug-in che fa rumore,
  allunga il file fino al massimo di 20 secondi.
- Per esportare un loop di lunghezza esatta, senza coda, chiedi all'assistente o usa il tool MCP
  `bounce` con `tail_sec: 0`.

## Annulla e ripeti

- Ctrl+Z annulla l'ultima modifica, Ctrl+Y la ripete.
- Dopo **Nuovo** (Ctrl+N) o **Apri** (Ctrl+O) la cronologia riparte da zero: Ctrl+Z non riporta
  indietro il progetto precedente.
- Un progetto appena aperto non risulta modificato: Aurora non chiede di salvare finché non
  cambi qualcosa (vedi *Modifiche non salvate*).

## Modifiche non salvate

Quando il progetto ha modifiche non salvate, nel titolo della finestra compare un pallino "•".
Se in quel momento chiudi la finestra, crei un progetto nuovo (Ctrl+N), ne apri un altro (Ctrl+O)
o scegli un file recente, Aurora ti chiede prima se vuoi salvare.

- **Save** salva il progetto e poi continua. Se il progetto non è mai stato salvato, si apre la
  finestra per scegliere il file; se la chiudi senza scegliere, non succede nulla.
- **Don't Save** continua senza salvare: le modifiche si perdono.
- **Cancel** (o Esc) chiude la domanda e lascia tutto com'era.

Invio equivale a Save. Se stai registrando, la registrazione si ferma prima della domanda e la
take resta nel progetto.

## Scorciatoie principali

| Tasto | Azione |
|---|---|
| Spazio | riproduci / ferma |
| R | registra |
| C | attiva o disattiva il ciclo |
| K | metronomo |
| Home | torna all'inizio |
| Ctrl+Z / Ctrl+Y | annulla / ripeti |
| Ctrl+S / O / N | salva / apri / nuovo |
| Ctrl+D | duplica la regione |
| Ctrl+B | bounce |
| Ctrl+rotella | zoom |
| Maiusc+rotella | scorrimento orizzontale |
