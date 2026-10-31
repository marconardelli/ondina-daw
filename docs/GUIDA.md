# Guida all'uso di Ondina

Questa guida spiega come usare gli editor di Ondina con mouse, tastiera e menu.

## Piano roll

- Clic su uno spazio vuoto: aggiunge una nota (la durata si sceglie nella barra degli strumenti).
- Clic su una nota: la seleziona. Ctrl o Maiusc + clic aggiunge o toglie note dalla selezione.
- Trascinamento su uno spazio vuoto: selezione a riquadro, comprese le note sovrapposte.
- Trascinamento di una nota: la sposta. Trascinando il bordo destro ne cambi la durata.
- Canc o Backspace: elimina le note selezionate. Doppio clic o clic destro elimina una sola nota.
- Corsia delle velocity: trascina per impostare la velocity.
- Quando apri una regione la vista si porta sulle sue note (su C4 se è vuota). Doppio clic sulla regione
  nell'arrangiamento: la inquadra di nuovo.
- Rotella: scorre le altezze, alla stessa velocità in su e in giù. Maiusc+rotella: scorre il tempo.
- Ctrl+rotella: zoom orizzontale attorno al punto sotto il puntatore. Ctrl+Maiusc+rotella: zoom verticale
  (altezza dei tasti) attorno alla nota sotto il puntatore.
- In riproduzione la linea del playhead segue le ripetizioni di una regione in loop e, con **Follow playhead**
  attivo, la vista la segue.

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

Lo stesso vale per un agente esterno collegato via MCP (per esempio Claude Desktop): con il tool
`get_selection` legge la traccia, la regione e le note che hai selezionato.

## Tracce nell'arrangiamento

A sinistra di ogni traccia c'è la sua intestazione, con il nome, i pulsanti M (mute), S (solo) e
R (registrazione), lo slider del volume e il meter.

- Clic sull'intestazione: seleziona la traccia. Clic destro: **Duplicate track**, **Bounce in place**,
  **Delete track**.
- Trascinamento dell'intestazione in alto o in basso: sposta la traccia. Una linea mostra dove andrà.
  Se arrivi al bordo alto o basso dell'area delle tracce, la vista scorre. Rilascia per spostarla,
  Esc annulla. Se la rilasci dov'era non cambia niente. Il mixer segue lo stesso ordine.
- Il trascinamento parte anche da M, S e R (un clic senza trascinare li attiva come sempre), non dallo
  slider del volume, che resta per il volume.
- Doppio clic sul nome: apre un campo di testo per rinominare la traccia. Invio o un clic fuori
  confermano, Esc annulla. Un nome vuoto o uguale a prima non cambia niente. Mentre scrivi, le
  scorciatoie da tastiera sono spente.
- Ogni spostamento o rinomina si annulla con un solo Ctrl+Z.
- Cancellando una traccia, una mandata o un plug-in, Ondina toglie anche ciò che puntava a loro: le
  automazioni delle mandate e dei parametri del plug-in, le celle di Live Loops della traccia e le route
  MIDI verso di lei. Le automazioni delle mandate rimaste restano sulle mandate giuste. Un solo Ctrl+Z
  rimette tutto a posto (una cella che stava suonando va rilanciata).
- **Duplicate track** copia anche l'automazione: volume, pan, mandate e parametri dei plug-in. Nella copia
  i parametri automatizzati muovono i plug-in della copia, non quelli della traccia originale.
- Se cambi lo strumento o applichi una patch dall'inspector, Ondina toglie le automazioni dei parametri dei
  plug-in sostituiti. Volume, pan e mandate restano. Un solo Ctrl+Z rimette tutto com'era.

### Zoom, scorrimento e altezza delle tracce

- Rotella: scorre le tracce su e giù.
- Maiusc+rotella, o la rotella orizzontale: scorre il tempo.
- Ctrl+rotella: zoom orizzontale attorno al punto sotto il puntatore.
- Ctrl+Maiusc+rotella: tracce più alte o più basse (da 32 a 160 punti) attorno alla traccia sotto il
  puntatore.
- Menu View: **Zoom In** e **Zoom Out** ingrandiscono o rimpiccioliscono attorno al cursore di
  riproduzione (o al centro, se il cursore non si vede); **Zoom to Fit** mostra tutto il brano nella
  larghezza visibile; **Taller Tracks** e **Shorter Tracks** cambiano l'altezza delle tracce. Una voce è
  grigia quando il suo limite è raggiunto.
- Con tracce basse i pulsanti M, S e R stanno accanto al nome e il volume si regola dal mixer.
- Zoom e altezza delle tracce non si salvano nel progetto e non entrano in Annulla.

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
- Dopo la fine, Ondina continua finché le code di riverbero e delay non si spengono (sotto circa
  −90 dBFS): almeno 2 secondi, al massimo 20. Così il finale suona come in riproduzione. Vale anche
  per Bounce in Place, Bounce Surround e Bounce Binaural.
- Un suono che non si spegne mai, per esempio una nota tenuta oltre la fine o un plug-in che fa rumore,
  allunga il file fino al massimo di 20 secondi.
- Per esportare un loop di lunghezza esatta, senza coda, chiedi all'assistente o usa il tool MCP
  `bounce` con `tail_sec: 0`.

## File scritti dall'assistente e dai tool MCP

Quando l'assistente o un client MCP salva il progetto, fa un bounce o esporta un file, Ondina accetta
solo un file su un disco locale con l'estensione del suo tipo: `.ondina` per il progetto (o il vecchio `.aurora`), `.wav` per
i bounce e l'export ADM, `.mid` o `.midi` per il MIDI, `.musicxml` o `.xml` per la partitura. Se il
percorso non ha estensione, Ondina la aggiunge.

- I percorsi di rete (`\\server\cartella`) e le altre estensioni vengono rifiutati con un messaggio, e
  non viene scritto nulla.
- L'assistente non sovrascrive un file esistente che non sia un progetto Ondina.
- Dai menu e dalle finestre di salvataggio dell'app puoi continuare a scegliere qualsiasi cartella,
  anche su un NAS. Un progetto aperto da una cartella di rete si risalva con Ctrl+S, non dall'assistente.

## Assistente con modello locale

Senza chiave Anthropic l'assistente usa un modello che gira sul computer, attraverso un server
compatibile OpenAI all'indirizzo scelto in **Settings ⚙** (di default `http://127.0.0.1:8080/v1`).

- Se nessun server risponde, Ondina avvia il server llama.cpp incluso in `tools\llm` e lo chiude
  quando chiudi Ondina. Un server già acceso prima (Ollama, LM Studio, un `llama-server` avviato a
  mano) viene usato così com'è e resta acceso.
- La conversazione tiene le richieste più recenti entro il contesto del modello. Quando si allunga,
  le richieste più vecchie vengono dimenticate; **New chat** ricomincia da capo.
- **⏹ Stop** interrompe subito l'attesa. Le modifiche già fatte restano e si annullano con Ctrl+Z.
  Il modello può finire di scrivere la risposta interrotta in sottofondo: la richiesta successiva
  può partire con qualche secondo di ritardo.
- Se Ondina si chiude in modo anomalo, `llama-server.exe` può restare acceso. Al riavvio Ondina lo
  riusa ma non lo chiude: chiudilo dal Task Manager.

## Annulla e ripeti

- Ctrl+Z annulla l'ultima modifica, Ctrl+Y la ripete.
- Dopo **Nuovo** (Ctrl+N) o **Apri** (Ctrl+O) la cronologia riparte da zero: Ctrl+Z non riporta
  indietro il progetto precedente.
- Un progetto appena aperto non risulta modificato: Ondina non chiede di salvare finché non
  cambi qualcosa (vedi *Modifiche non salvate*).

## Modifiche non salvate

Quando il progetto ha modifiche non salvate, nel titolo della finestra compare un pallino "•".
Se in quel momento chiudi la finestra, crei un progetto nuovo (Ctrl+N), ne apri un altro (Ctrl+O)
o scegli un file recente, Ondina ti chiede prima se vuoi salvare.

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
| Ctrl+rotella | zoom orizzontale attorno al puntatore |
| Maiusc+rotella | scorrimento orizzontale |
| Ctrl+Maiusc+rotella | altezza delle tracce (nel piano roll: altezza dei tasti) |
