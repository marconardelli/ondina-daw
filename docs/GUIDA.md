# Guida all'uso di Ondina

Questa guida spiega come usare gli editor di Ondina con mouse, tastiera e menu.

## Piano roll

- Clic su uno spazio vuoto: aggiunge una nota (la durata si sceglie nella barra degli strumenti).
- Clic su una nota: la seleziona. Ctrl o Maiusc + clic aggiunge o toglie note dalla selezione.
- Trascinamento su uno spazio vuoto: selezione a riquadro, comprese le note sovrapposte.
- Trascinamento di una nota: la sposta e le note spostate restano selezionate. Con Alt premuto al rilascio la
  copia: gli originali restano dove erano e le copie diventano la selezione (un solo Ctrl+Z). Mentre tieni Alt il
  puntatore mostra la copia. Trascinando il bordo destro ne cambi la durata.
- Canc o Backspace: elimina le note selezionate. Doppio clic o clic destro elimina una sola nota.
- Ctrl+C copia le note selezionate, Ctrl+X le taglia, Ctrl+V le incolla. Se il playhead è dentro la regione
  aperta, la prima nota va sul playhead (arrotondato alla griglia dello snap); altrimenti le note tornano alla
  posizione da cui le hai copiate, anche in un'altra regione. Le note che cadrebbero oltre la fine della regione
  si scartano. Le note incollate restano selezionate. Ogni operazione si annulla con un solo Ctrl+Z.
- Le note copiate vanno anche negli appunti di Windows come testo (JSON), che puoi incollare nella chat
  dell'assistente. Se dopo aver copiato in Ondina copi un'immagine in un altro programma, Ctrl+V nel piano roll
  non fa nulla finché non copi di nuovo le note.
- Ctrl+A seleziona tutte le note.
- Frecce ←/→: spostano le note selezionate di un passo di snap (1/16 con lo snap spento, 1/4 con Bar);
  ↑/↓ di un semitono, Maiusc+↑/↓ di un'ottava. Lo spostamento si ferma all'inizio della regione e ai limiti
  delle altezze MIDI senza cambiare la forma degli accordi.
- Alt+←/→: accorciano o allungano le note selezionate di un passo di snap (1/16 con lo snap spento), mai sotto un
  passo. Le note restano selezionate.
- Queste scorciatoie agiscono quando il pannello Editor mostra il piano roll e non stai scrivendo in un campo
  di testo; le frecce sono spente con Musical Typing attivo.
- Corsia delle velocity: trascina per impostare la velocity; ogni nota che il tratto attraversa prende il valore
  lungo il tratto, anche con un gesto veloce. Un tratto che non cambia nulla non crea un passo di annullamento.
- Quando apri una regione la vista si porta sulle sue note (su C4 se è vuota). Doppio clic sulla regione
  nell'arrangiamento: la inquadra di nuovo.
- Rotella: scorre le altezze, alla stessa velocità in su e in giù. Maiusc+rotella: scorre il tempo.
- Ctrl+rotella: zoom orizzontale attorno al punto sotto il puntatore. Ctrl+Maiusc+rotella: zoom verticale
  (altezza dei tasti) attorno alla nota sotto il puntatore.
- In riproduzione la linea del playhead segue le ripetizioni di una regione in loop e, con **Follow playhead**
  attivo, la vista la segue.
- Nello step sequencer, durante la riproduzione, un bordo bianco segna lo step che suona, anche nelle
  ripetizioni di una regione in loop.

### Corsia dei controller

Sotto la corsia delle velocity c'è la corsia dei controller: mostra un controller MIDI della regione alla volta.

- Il selettore a sinistra sceglie il controller: Sustain (64), Mod wheel (1), Expression (11), Volume (7),
  Pan (10), Pitch bend e ogni altro controller già presente nella regione. Accanto al nome vedi quanti eventi ha.
- Il pedale del sustain si vede come barre dove è premuto. Gli altri controller sono una linea a gradini con un
  punto per ogni evento; Pan e Pitch bend hanno una linea al centro.
- Trascina con il tasto sinistro per disegnare: scrive un evento per ogni passo di snap (1/32 con lo snap
  spento). In alto il valore è 127, in basso 0; per il sustain la metà alta è pedale giù, la metà bassa pedale su.
  Mentre trascini vedi il risultato e il valore in alto a destra.
- Trascina con il tasto destro per cancellare: il tratto si colora di rosso e al rilascio gli eventi del
  controller in quel tratto spariscono.
- Un clic disegna un solo passo, un clic destro lo cancella.
- Dopo il tratto il controller torna al valore che aveva, quindi il resto della curva non cambia.
- Ogni tratto si annulla con un solo Ctrl+Z e si rifà con Ctrl+Y.
- Il pulsante **CC** chiude la corsia e lascia più spazio alle note; un altro clic la riapre.
- Gli strumenti interni rispondono solo al sustain, e solo Analog Synth e FM Synth. Gli strumenti VST3 ricevono
  tutti i controller. Se lo strumento della traccia ignora il controller scelto, la corsia lo scrive in grigio.
- L'assistente e i client MCP cambiano gli stessi eventi con `replace_cc` (sostituisce un controller in un
  intervallo di beat) e li aggiungono con `add_cc`.

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
- Trascinamento dell'intestazione **col tasto sinistro** in alto o in basso: sposta la traccia. Una
  linea mostra dove andrà. Se arrivi al bordo alto o basso dell'area delle tracce, la vista scorre.
  Rilascia per spostarla, Esc annulla. Se la rilasci dov'era non cambia niente. Il mixer segue lo stesso ordine.
- Il trascinamento parte anche da M, S e R (un clic senza trascinare li attiva come sempre), non dallo
  slider del volume, che resta per il volume.
- Il clic destro apre solo il menu: un clic destro tenuto a lungo o trascinato, o un trascinamento col
  tasto centrale, non sposta la traccia.
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

## Nome e colore delle regioni

Una regione nuova, o una take registrata, nasce senza un colore proprio e segue quello della traccia;
il nome di partenza viene dalla traccia o dal file importato.

- Clic destro sulla regione, **Rename…**: sopra la regione compare un campo di testo con il nome
  selezionato. Invio o un clic fuori confermano, Esc annulla. Un nome vuoto o uguale a prima non cambia
  niente. Mentre scrivi, le scorciatoie da tastiera sono spente. Se la regione esce dalla vista (rotella,
  zoom, playhead che scorre), il campo si chiude senza cambiare il nome.
- Clic destro sulla regione, **Color**: dodici colori; il bordo bianco segna quello attuale. **Track
  color** toglie il colore proprio della regione: torna al colore della traccia e lo segue anche se lo
  cambi dopo. Una regione in mute resta grigia.
- Inspector, sezione **Region**: il nome (si conferma uscendo dal campo o con Invio, Esc annulla), gli
  stessi colori con **Track color** e il selettore **Custom** per un colore qualsiasi. La sezione resta
  aperta quando rinomini la regione.
- Ogni rinomina o cambio di colore si annulla con un solo Ctrl+Z; anche un trascinamento nel selettore
  **Custom** è un passo solo.
- Il doppio clic sulla regione apre l'editor, come prima.
- L'assistente e i tool MCP usano `set_region_properties`: `color` vuoto o `"none"` toglie il colore
  della regione, e `get_region` riporta `color` solo se la regione ne ha uno.

## Dissolvenze delle regioni audio

Sulle regioni audio la curva del fade-in e del fade-out è disegnata sopra la forma d'onda, con un velo scuro
sulla parte che la dissolvenza abbassa. In una regione in loop la curva c'è su ogni ripetizione, perché il fade
si sente su ognuna.

- Passando col puntatore su una regione audio (o quando è selezionata) compaiono due quadratini sul bordo alto
  della prima ripetizione: a sinistra il fade-in, a destra il fade-out. Stanno dove finisce il fade-in e dove
  inizia il fade-out; con il fade a zero sono negli angoli.
- Trascina un quadratino in orizzontale per allungare o accorciare quel fade: la curva segue il puntatore e un
  riquadro mostra il valore in battiti. Il valore segue lo **Snap** (al massimo un battito, anche con Bar); tieni
  premuto Alt mentre trascini per ignorarlo. Il fade non va sotto zero e i due fade insieme non superano la
  regione (una ripetizione, se è in loop).
- Doppio clic su un quadratino toglie quel fade.
- Ogni trascinamento o doppio clic si annulla con un solo Ctrl+Z.
- Nelle regioni più strette di circa 24 punti i quadratini non ci sono: allarga la regione con lo zoom. Il bordo
  destro sotto i quadratini cambia ancora la lunghezza della regione, e il resto della regione si sposta come
  prima.
- Gli stessi valori si cambiano dagli slider **Fade in** e **Fade out** dell'editor audio; l'assistente e i tool
  MCP usano `set_region_properties` con `fade_in` e `fade_out` in battiti.

## Righello e ciclo

Il righello in cima all'arrangiamento ha due parti: la striscia alta, più scura, è quella del ciclo;
la parte bassa, con i numeri delle battute, è quella del playhead.

- Clic o trascinamento nella parte bassa: sposta il playhead.
- Trascinamento del ciclo nella striscia alta: lo sposta e ne tiene la lunghezza. L'inizio si
  aggancia allo snap della barra degli strumenti e il ciclo non va prima dell'inizio della canzone.
- Trascinamento di un bordo del ciclo (il puntatore diventa una doppia freccia): sposta solo quel
  bordo. Il bordo si aggancia alla griglia, il ciclo dura almeno un sedicesimo e i bordi non si
  incrociano.
- Clic sul ciclo: lo accende o lo spegne, come il tasto C. Spento, il ciclo è grigio e si può
  comunque spostare o allungare.
- Trascinamento sulla striscia fuori dal ciclo: disegna un ciclo nuovo e lo accende. Un clic nello
  stesso punto sposta il playhead.
- Maiusc+trascinamento in qualsiasi punto del righello: disegna un ciclo nuovo, come prima.
- Ogni gesto si annulla con un solo Ctrl+Z.

Durante la riproduzione il ciclo nuovo vale subito. Se lo sposti tutto prima del playhead, la
riproduzione prosegue: riporta il playhead dentro il ciclo con un clic nella parte bassa per
farlo ripetere.

## Corsia marker

La corsia marker sta sotto il righello dell'arrangiamento e contiene le sezioni (marker con una
lunghezza, in blu) e i marker semplici (un punto, in marrone).

- Clic su un marker: sposta il playhead all'inizio del marker.
- Maiusc+, e Maiusc+.: portano il playhead al marker precedente o successivo.
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

## Corsia accordi

La corsia accordi sta sotto la corsia marker, sopra le tracce. Mostra gli accordi della canzone e,
nell'etichetta a sinistra, la tonalità del progetto.

- Doppio clic sulla corsia vuota: apre un campo di testo sul posto. L'accordo comincia sulla linea
  di griglia a sinistra del puntatore (snap della barra degli strumenti; con lo snap spento, nel
  punto esatto) e dura una battuta, meno se l'accordo seguente comincia prima: un contorno grigio
  mostra la lunghezza che avrà. Scrivi un simbolo (`Am7`, `F#m`, `G/B`) o un grado nella tonalità
  del progetto (`I`, `vi`, `IV`, `V7`) e premi Invio. Un grado diventa il simbolo dell'accordo
  nella tonalità attuale: `IV` in Do maggiore è `F`. Esc o un testo vuoto annullano.
- Testo non valido: con Invio il campo resta aperto con un contorno rosso e il testo selezionato,
  così lo riscrivi subito, e la barra di stato mostra il messaggio. Un clic altrove chiude il campo
  senza creare nulla.
- Doppio clic su un accordo, oppure clic destro e **Edit chord**: apre lo stesso campo con il
  simbolo attuale. Invio o un clic fuori confermano, Esc annulla. Lo stesso accordo scritto come
  grado (`V` su un `G` in Do maggiore) non cambia nulla.
- Clic su un accordo: sposta il playhead all'inizio dell'accordo.
- Trascinamento di un accordo: lo sposta seguendo lo snap. L'accordo non va prima dell'inizio della
  canzone e gli altri accordi restano dove sono: un accordo lasciato sopra un altro si sovrappone.
- Trascinamento del bordo destro: cambia la lunghezza dell'accordo. La fine si aggancia alla
  griglia, l'accordo dura almeno un sedicesimo e non va oltre l'inizio dell'accordo seguente.
- Clic destro e **Remove chord**: elimina l'accordo.
- Mentre scrivi, le scorciatoie da tastiera sono spente.
- Ogni creazione, modifica, spostamento o allungamento si annulla con un solo Ctrl+Z.

I session player (Bassist, Keys, Arpeggio, Melody) seguono la corsia accordi quando non ricevono
una progressione propria. Un grado salvato sulla corsia da una versione precedente viene letto
nella tonalità attuale del progetto.

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

## Carico DSP e sovraccarichi

Nella barra di trasporto, accanto a **DSP nn%** (quanto del tempo a disposizione ha usato l'ultimo blocco
audio), c'è il numero di **overload**: i blocchi che dall'ultimo Play hanno impiegato più tempo di quello
concesso dalla scheda audio. Ognuno si può sentire come un clic o un buco nel suono.

- A 0 il numero è grigio; appena c'è un sovraccarico diventa rosso.
- Play lo azzera. Stop no: dopo Stop leggi quanti sovraccarichi ha avuto la riproduzione appena finita.
- Se cresce, aumenta la dimensione del buffer nelle impostazioni della scheda audio, chiudi altri programmi
  pesanti o togli qualche plug-in.
- Il tool MCP `get_transport` riporta gli stessi valori in `dsp_load` e `dsp_overloads`.

Quando una nota o una coda di riverbero si spegne, i filtri scendono verso numeri piccolissimi (sotto 1e-38) che
il processore calcola molto più lentamente. Ondina li azzera mentre suona e durante i bounce: il carico non sale
a fine nota o dopo Stop, e il suono non cambia (sono valori centinaia di dB sotto l'udibile).

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

## Plug-in VST3

Ondina carica i plug-in VST3 solo da un bundle `.vst3` su un disco locale, per esempio
`C:\Program Files\Common Files\VST3`. Il controllo vale per **Add from file…**, per l'assistente, per i
client MCP e per i progetti che apri.

I client MCP e l'assistente caricano plug-in solo dalle cartelle VST3 standard
(`C:\Program Files\Common Files\VST3` e `%LOCALAPPDATA%\Programs\Common\VST3`) e da quelle che aggiungi tu.
Per aggiungerne una: pannello **Library** → **Third-party VST3** → **Add folder…**. Ondina la salva nelle
impostazioni e rifà la scansione; la ✕ accanto alla cartella la toglie. Nessun tool MCP può cambiare questo
elenco.

- Se un agente chiede un plug-in fuori da queste cartelle riceve un errore che inizia con *plug-in path
  refused*: aggiungi la cartella e riprova. Lo stesso vale per le cartelle extra della scansione, che vengono
  saltate.
- **Add from file…** non ha questo limite: il plug-in lo scegli tu.
- Un plug-in su una cartella di rete (`\\server\cartella`) o un file che non è `.vst3` viene rifiutato
  con un messaggio.
- Se apri un progetto che usa un plug-in rifiutato, il progetto si apre lo stesso: il plug-in resta al
  suo posto ma non suona, e la barra di stato dice quanti plug-in non sono stati caricati. Nella stessa
  riga compare anche il numero dei file audio mancanti.
- Anche le cartelle extra della scansione devono stare su un disco locale: quelle su una condivisione di rete
  vengono saltate.
- Se tieni i plug-in su un NAS, copiali in una cartella locale e aggiungili di nuovo.

## Assistente con modello locale

Senza chiave Anthropic l'assistente usa un modello che gira sul computer, attraverso un server
compatibile OpenAI all'indirizzo scelto in **Settings ⚙** (di default `http://127.0.0.1:8080/v1`).

- Se nessun server risponde, Ondina avvia il server llama.cpp incluso in `tools\llm` e lo chiude
  quando chiudi Ondina. Un server già acceso prima (Ollama, LM Studio, un `llama-server` avviato a
  mano) viene usato così com'è e resta acceso.
- Il modello locale riceve 43 tool, scelti perché stiano nel suo contesto. Bastano per i pulsanti ✨
  del piano roll (*continue / improve*), dello step sequencer (*write a groove*), degli editor dei
  plug-in (*design a sound*), di Live Loops (*fill the grid*), del report del mix e dei menu della
  finestra di arrangiamento. Se nel piano roll hai selezionato delle note, il modello modifica solo
  quelle. Claude riceve tutti i tool.
- La conversazione tiene le richieste più recenti entro il contesto del modello. Quando si allunga,
  le richieste più vecchie vengono dimenticate; **New chat** ricomincia da capo.
- **⏹ Stop** interrompe subito l'attesa. Le modifiche già fatte restano e si annullano tutte insieme
  con Ctrl+Z.
  Il modello può finire di scrivere la risposta interrotta in sottofondo: la richiesta successiva
  può partire con qualche secondo di ritardo.
- Se Ondina si chiude in modo anomalo, `llama-server.exe` può restare acceso. Al riavvio Ondina lo
  riusa ma non lo chiude: chiudilo dal Task Manager.

## Annulla e ripeti

- Ctrl+Z annulla l'ultima modifica, Ctrl+Y la ripete.
- Esc mentre trascini con il mouse (regioni, dissolvenze, marker, accordi, ciclo, intestazioni delle tracce, note,
  bordi delle note, rettangolo di selezione, velocity, tratti nella corsia dei controller) annulla il gesto: tutto
  torna com'era e non serve Ctrl+Z. Il ciclo, che si aggiorna mentre lo trascini, torna al suo posto; nella
  cronologia può restare un passo "set_cycle" che non cambia nulla. Finché tieni premuto il pulsante non parte un
  altro trascinamento: rilascia e ricomincia.
- Tutto quello che l'assistente fa per una richiesta è un solo passo: Ctrl+Z, o **↶ Undo AI edit**
  nel pannello dell'assistente, riporta il progetto a com'era prima della richiesta. Il menu Edit
  mostra l'inizio della richiesta, per esempio *Undo assistant: Add drums*.
- **↶ Undo AI edit** è attivo solo quando l'ultima modifica è dell'assistente e l'assistente ha
  finito: se dopo la risposta sposti una nota o un fader, il pulsante si spegne e Ctrl+Z annulla
  prima la tua modifica.
- Se mentre l'assistente lavora modifichi il progetto, o lo modifica un client MCP, la richiesta si
  divide in due passi con la modifica in mezzo. Le modifiche fatte da un client MCP restano un passo
  per ogni tool.
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

L'assistente o un client MCP non possono aprire o creare un progetto mentre registri: ricevono l'errore
"recording in progress" e la registrazione continua. Dopo **Nuovo** il trasporto è fermo e il playhead
torna all'inizio, come dopo **Apri**.

Un campo di testo aperto (rinomina di marker, tracce o regioni, nome della regione nell'inspector,
accordo) si chiude senza applicare nulla quando apri o crei un altro progetto, anche se lo fanno
l'assistente o un client MCP.

## Scorciatoie principali

| Tasto | Azione |
|---|---|
| Spazio | riproduci / ferma |
| R | registra |
| C | attiva o disattiva il ciclo |
| K | metronomo |
| Home | torna all'inizio |
| , / . | playhead all'inizio della battuta precedente / successiva |
| Maiusc+, / Maiusc+. | playhead al marker precedente / successivo (o all'inizio / alla fine della canzone) |
| Ctrl+Z / Ctrl+Y | annulla / ripeti |
| Ctrl+S / O / N | salva / apri / nuovo |
| Ctrl+D | duplica la regione |
| Ctrl+T | divide la regione selezionata al playhead |
| Ctrl+B | bounce |
| Ctrl+rotella | zoom orizzontale attorno al puntatore |
| Maiusc+rotella | scorrimento orizzontale |
| Ctrl+Maiusc+rotella | altezza delle tracce (nel piano roll: altezza dei tasti) |
| Ctrl+C / Ctrl+X / Ctrl+V | copia / taglia / incolla le note (piano roll) |
| Ctrl+A | seleziona tutte le note (piano roll) |
| ← / → | sposta le note selezionate di un passo di snap |
| ↑ / ↓, Maiusc+↑ / ↓ | trasporta le note selezionate di un semitono / un'ottava |
| Alt+← / → | accorcia / allunga le note selezionate di un passo di snap |
| Alt+trascinamento | copia le note trascinate (piano roll) |
| Esc (mentre trascini) | annulla il trascinamento |

`,` e `.` da metà battuta portano all'inizio della battuta; durante la riproduzione, a meno di mezzo beat da una
battuta o da un marker, `,` va a quello prima. Queste scorciatoie e Ctrl+T sono spente mentre scrivi in un campo di
testo e con Musical Typing attivo.
