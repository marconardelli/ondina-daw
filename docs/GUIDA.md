# Guida all'uso di Ondina

Questa guida spiega come usare gli editor di Ondina con mouse, tastiera e menu.

## Piano roll

- Clic su uno spazio vuoto: aggiunge una nota (la durata si sceglie nella barra degli strumenti) e la seleziona,
  così le frecce, Alt+←/→, Canc e Ctrl+C/X agiscono subito sulla nota nuova. Con Ctrl o Maiusc la nota nuova si
  aggiunge alla selezione. Un clic oltre la fine della regione toglie la selezione.
- Clic su una nota: la seleziona. Ctrl o Maiusc + clic aggiunge o toglie note dalla selezione.
- Trascinamento su uno spazio vuoto: selezione a riquadro, comprese le note sovrapposte.
- Trascinamento di una nota: la sposta e le note spostate restano selezionate. Con Alt premuto al rilascio la
  copia: gli originali restano dove erano e le copie diventano la selezione (un solo Ctrl+Z). Mentre tieni Alt il
  puntatore mostra la copia. Trascinando il bordo destro ne cambi la durata.
- Canc o Backspace: elimina le note selezionate. Doppio clic o clic destro elimina una sola nota.
- Ctrl+C copia le note selezionate, Ctrl+X le taglia, Ctrl+V le incolla. Se il playhead è dentro la regione
  aperta, la prima nota va sul playhead (arrotondato alla griglia dello snap); altrimenti le note tornano alla
  posizione da cui le hai copiate, anche in un'altra regione. Le note che cadrebbero oltre la fine della regione
  si scartano. Le note incollate restano selezionate. Ogni operazione si annulla con un solo Ctrl+Z. Senza note
  selezionate, Ctrl+C e Ctrl+X copiano la regione intera (vedi «Copiare, tagliare e incollare regioni»).
- Le note copiate vanno anche negli appunti di Windows come testo (JSON), che puoi incollare nella chat
  dell'assistente. Se dopo aver copiato in Ondina copi un'immagine in un altro programma, Ctrl+V nel piano roll
  non fa nulla finché non copi di nuovo le note.
- Ctrl+A seleziona tutte le note.
- Ctrl+D ripete le note selezionate subito dopo se stesse: le copie partono dalla fine della selezione (dall'inizio
  della prima nota alla fine dell'ultima), arrotondata in su alla griglia dello snap (al massimo un beat; con lo snap
  spento la durata esatta). Le copie che cadrebbero oltre la fine della regione si scartano. Le copie diventano la
  selezione, quindi premendo ancora Ctrl+D la frase si ripete di nuovo; ogni ripetizione si annulla con un solo
  Ctrl+Z. Una pausa dopo l'ultima nota non conta: per ripetere una battuta che finisce con una pausa usa lo snap 1/4
  (l'ultima nota si arrotonda al beat) oppure copia e incolla col playhead. Senza note selezionate Ctrl+D duplica la
  regione. Anche dal menu Edit ▸ Repeat Selected Notes.
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

### Armonia (Harmony)

Il menu **Harmony** della barra del piano roll aggiunge una seconda voce alle note selezionate (a tutte le note
della regione se non c'è nessuna selezione).

- Scegli l'intervallo: *3rd above*, *3rd below*, *5th above*, *6th above*, *6th below*, *Octave above* o
  *Octave below*.
- Terze, quinte e seste contano i gradi della tonalità del progetto, quindi la voce resta nella tonalità: in Do
  maggiore la terza sopra di Do è Mi, quella di Mi è Sol, quella di Si è Re. Una nota fuori tonalità parte dalla
  nota della scala più vicina. Con una scala pentatonica, blues o cromatica la voce va alla nota della scala più
  vicina all'intervallo maggiore.
- L'ottava è sempre di 12 semitoni, anche per una nota fuori tonalità.
- Le note nuove hanno lo stesso inizio e la stessa durata, e una velocity un poco più bassa (85%).
- Una nota che c'è già (stessa altezza e stesso inizio) non si raddoppia, e una nota che uscirebbe dall'estensione
  MIDI non si aggiunge. Se non si aggiunge niente, la barra di stato lo dice.
- Le note aggiunte diventano la selezione: puoi spostarle con le frecce o toglierle con Canc. Un Ctrl+Z annulla
  tutta l'armonia.
- L'assistente e i client MCP fanno lo stesso con il tool `add_harmony` (per esempio "aggiungi una seconda voce a
  terze sopra le note selezionate").

### Lunghezza e velocity (Length, Velocity)

I menu **Length** e **Velocity** della barra del piano roll cambiano le note selezionate (tutte le note della
regione se non c'è nessuna selezione). Gli inizi e le altezze non si muovono e la selezione resta sulle stesse
note. Ogni voce si annulla con un Ctrl+Z.

- **Legato**: ogni nota dura fino all'inizio della nota seguente. Le note che partono insieme (anche un accordo
  suonato un poco sfasato) finiscono insieme; le ultime restano come sono. Con una selezione conta solo la nota
  seguente tra quelle selezionate: puoi legare il basso senza guardare gli accordi.
- **Remove overlaps**: una nota si accorcia dove ricomincia la stessa altezza. Utile con un basso o un lead
  monofonico suonato dal vivo.
- **Fixed**: tutte le note prendono la durata scelta in **Note length** (il menu mostra quale, per esempio
  *Fixed 1/16*).
- **Halve (×½)** e **Double (×2)** dimezzano o raddoppiano le durate.
- Nessuna nota diventa più corta di 1/64 di battito, e una nota allungata si ferma alla fine della regione. Se non
  cambia niente, la barra di stato lo dice.
- **Velocity**: *Set 64*, *80*, *100*, *127* danno a tutte la stessa velocity; *Scale 90%* e *110%* le
  abbassano o le alzano in proporzione; nel campo numerico scrivi un valore da 1 a 127 e premi **Set**.
- L'assistente (con Claude) e i client MCP fanno lo stesso con `set_note_lengths` (per esempio "rendi legato il
  basso") e `set_note_velocities`.

## Step sequencer

Una regione pattern (per esempio quella creata su una traccia Pop Drum Kit) si apre nello step sequencer: una riga
per suono di batteria, una cella per step.

- **Clic** su una cella: la accende (velocity 100) o la spegne. **Clic destro**: accento (120) → normale (90) →
  ghost (45) → spenta.
- **Trascinamento**: tieni premuto il tasto sinistro su una cella e passa sulle altre. Se la prima cella era
  spenta il tratto le accende tutte, se era accesa le spegne. Il tratto può attraversare più righe e un movimento
  veloce non salta celle. Le celle cambiano colore subito, il progetto cambia al rilascio: un Ctrl+Z annulla
  l'intero tratto. Le celle non toccate tengono la loro velocity; un tratto che non cambia nulla non lascia passi
  di undo. **Esc** col tasto ancora premuto annulla il tratto.
- **Clic sul nome** della riga: suona il suono. **Clic destro sul nome**: menu con il nome e la nota della riga in
  testa, poi:
  - **Preview** suona il suono;
  - **Mute** / **Unmute** silenzia o riattiva la riga (come il pulsante **M**);
  - **Rename…** apre un campo sopra il nome: **Invio** (o un clic altrove) conferma, **Esc** annulla;
  - **Sound** elenca i 25 suoni del Drum Kit e cambia nota e nome insieme; il mute e gli step restano. Su una
    traccia che non è un Drum Kit cambia solo la nota suonata;
  - cinque riempimenti che riscrivono la riga intera a velocity normale: **Clear row**, **Every step**, **Every
    2nd step**, **Every 4th step** e **Offbeats** (il "e" di ogni battito: con step da 1/16 gli step 3, 7, 11, 15);
  - **Delete row** toglie la riga con i suoi step; le righe sotto salgono di uno. Tolta l'ultima, il pattern resta
    vuoto e si riempie di nuovo con **+ Row**.

  Ogni voce si annulla con un Ctrl+Z.
- **M** accanto al nome muta la riga: resta visibile, con il nome in grigio e le celle più scure, ma non suona e
  non compare nella miniatura della regione né nell'export MIDI. Gli step restano e si possono modificare. Un
  altro clic la riattiva.
- **+ Row**, sotto la griglia: elenco dei 25 suoni del Drum Kit con la loro altezza General MIDI (per esempio
  *Ride · 51*). La riga nuova compare in fondo, vuota. Una riga si toglie con **Delete row** nel menu del nome.
- L'assistente e i client MCP fanno lo stesso con `set_pattern_step`, `set_pattern_row`, `add_pattern_row`,
  `update_pattern_row` (nome, nota, mute) e `remove_pattern_row`.

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

Dalla tastiera:

- ↑ e ↓: selezionano la traccia sopra o sotto e la portano in vista. Sulla prima e sull'ultima traccia la
  selezione resta ferma; se nessuna traccia è selezionata si parte dalla prima.
- M: muta la traccia selezionata, S la mette in solo. Premuti di nuovo tolgono mute e solo. Ognuno si
  annulla con un Ctrl+Z.
- Con note selezionate nel piano roll le frecce spostano le note, non la selezione delle tracce. Con il
  piano roll aperto e nessuna nota selezionata le frecce cambiano traccia, ma l'editor continua a mostrare
  la regione di prima: un clic su una regione della nuova traccia la apre.
- Le stesse azioni sono nel menu **Track**.

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

### Trascinare file da Esplora risorse

- Un file audio (wav, aiff, flac, mp3, ogg, m4a, caf) rilasciato sulla corsia di una traccia audio si
  posa su quella traccia, al punto sotto il puntatore, agganciato alla linea della griglia più vicina.
- Sotto l'ultima traccia, o su una traccia instrument o bus, il file va su una traccia audio nuova in fondo.
- Mentre trascini, la riga di destinazione si colora, una linea mostra il punto d'inizio e un'etichetta
  accanto al puntatore dice il nome del file, la traccia e la posizione.
- Più file insieme si mettono uno dopo l'altro, in ordine di nome, al massimo 8 per volta; la barra di
  stato dice quanti ne sono rimasti fuori.
- Un solo Ctrl+Z toglie tutto quello che il rilascio ha aggiunto, tracce nuove comprese.
- Un file MIDI (.mid) si importa al beat sotto il puntatore, su tracce nuove.
- Un progetto (.ondina) si apre: se ci sono modifiche non salvate, prima compare la domanda *Save*,
  *Don't Save* o *Cancel* (una registrazione in corso si ferma prima della domanda). Un progetto va
  rilasciato da solo: insieme ad altri file non si apre e non si importa niente.
- I file di altro tipo si saltano e la barra di stato lo dice.
- Rilasciati fuori dall'arrangiamento (editor, mixer, assistente), i file vanno al cursore di riproduzione.
- Con file lunghi la finestra si ferma un attimo mentre li legge, come con File > Import Audio.

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

## Copiare, tagliare e incollare regioni

- Seleziona una regione con un clic: **Ctrl+C** la copia, **Ctrl+X** la taglia (la toglie dall'arrangiamento). Le
  stesse azioni sono nel menu **Edit**: *Copy Region*, *Cut Region* e *Paste Region*.
- **Ctrl+V** incolla una copia al playhead, portato indietro alla linea della griglia dello snap (con lo snap spento
  esattamente al playhead), sulla traccia selezionata. Per incollare su un'altra traccia fai clic su uno spazio vuoto
  della sua corsia o sulla sua intestazione, poi Ctrl+V. Senza una traccia selezionata la copia va sulla traccia da
  cui viene.
- Le regioni MIDI e i pattern vanno su tracce instrument, le regioni audio su tracce audio. Su una traccia di altro
  tipo la barra di stato lo dice e non cambia niente.
- La copia porta con sé tutto: note, controller, testi, pattern, impostazioni audio (gain, dissolvenze, reverse),
  nome, colore, ripetizioni, transpose e velocity offset. Diventa la regione selezionata e non fa parte di un gruppo
  di take.
- Puoi incollare più volte; due incolla nello stesso punto sovrappongono due copie.
- Ogni taglio e ogni incolla si annulla con un solo Ctrl+Z.
- Con note selezionate nel piano roll Ctrl+C e Ctrl+X copiano le note, come prima. Per copiare la regione intera
  togli la selezione delle note (un clic oltre la fine della regione) o usa il menu Edit. Ctrl+V incolla l'ultima
  cosa copiata, note o regione.
- Gli appunti restano finché Ondina è aperto e non si salvano nel progetto. Una regione MIDI o un pattern si può
  incollare anche dopo aver aperto un altro progetto; una regione audio no, perché il suo file appartiene al
  progetto da cui l'hai copiata.
- La regione copiata va anche negli appunti di Windows come testo (JSON), lo stesso oggetto che accetta il tool MCP
  `paste_region`. Se dopo aver copiato in Ondina copi un'immagine in un altro programma, Ctrl+V non fa nulla finché
  non copi di nuovo.
- Ctrl+C, Ctrl+X e Ctrl+V funzionano anche con Musical Typing acceso; sono spenti mentre scrivi in un campo di testo
  e durante un trascinamento.

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

## Automazione nell'arrangiamento

Premi **A** (o **View > Show Automation**) per vedere l'automazione sopra le regioni. Ogni traccia mostra una
sua corsia: il volume, il pan, una mandata o un'altra corsia che ha già (surround, parametri dei plug-in). Premi
di nuovo A per tornare all'arrangiamento di sempre.

- La curva è una linea arancione con un pallino per ogni punto; tra due punti il valore cambia in linea retta,
  come lo suona Ondina. Prima del primo punto e dopo l'ultimo il valore resta quello del punto.
- Una corsia senza punti è una linea tratteggiata all'altezza del valore del canale (per esempio il volume del
  mixer). In basso a sinistra c'è il nome della corsia, con il valore se è vuota.
- Al posto dello slider del volume, nell'intestazione della traccia, c'è il selettore della corsia: le voci con
  punti hanno accanto il loro numero. Lo stesso elenco è nel menu del clic destro sull'intestazione, **Automation
  lane**, che accende anche la vista; c'è anche sulle tracce basse, dove il selettore non entra.
- Clic sotto la barra del nome di una regione aggiunge un punto all'altezza del clic, sulla griglia dello
  **Snap** (al massimo un battito). Con Alt il punto va dove hai cliccato.
- Trascina un punto per spostarlo: resta tra i punti vicini e dentro l'intervallo del parametro, e un riquadro
  mostra il valore. Se premi sul vuoto e trascini, il punto nuovo nasce lì e lo sposti nello stesso gesto.
- Doppio clic o clic destro su un punto lo elimina. Esc mentre trascini riporta il punto dov'era.
- La striscia alta di ogni traccia (la barra del nome delle regioni e i quadratini dei fade) resta alle regioni:
  da lì le selezioni, le sposti, le ridimensioni e tiri i fade anche con la vista accesa.
- Ogni clic, trascinamento o eliminazione si annulla con un solo Ctrl+Z.
- Valori: volume da -60 a +6 dB, pan da sinistra a destra (-64..+64 come nel mixer), mandate in dB, parametri dei
  plug-in nel loro intervallo (i selettori, come la forma d'onda, a scatti).
- Il master non ha automazione e non compare nell'arrangiamento.
- L'assistente e i tool MCP vedono subito le corsie che disegni (`get_track`) e scrivono le stesse corsie con
  `set_automation`: quello che scrivono si vede nella vista e si corregge col mouse.

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
- Maiusc+M: aggiunge un marker semplice al punto esatto del playhead, anche durante la riproduzione, con il
  nome Marker N. Se lì c'è già un marker non ne aggiunge un altro. La stessa azione è nel menu Edit,
  **Add Marker at Playhead**.
- Ogni spostamento, ridimensionamento o rinomina si annulla con un solo Ctrl+Z.

Spostare un marker non sposta le regioni che ci stanno sotto. Per copiare o ripetere il materiale
di una sezione usa il menu del clic destro.

## Tonalità del progetto

La tonalità del progetto si sceglie dal menu a tendina della barra di trasporto, che mostra quella attuale (per
esempio "C Major"). La usano Snap to key, Harmony, il Transposer con "Lock to Project Key", il Chord Trigger con
"Diatonic (Key)", i session player e i gradi della corsia accordi.

- **Detect from MIDI**, la prima voce del menu, stima la tonalità dalle note delle parti MIDI delle tracce strumento
  e la imposta come tonalità del progetto. Non contano le tracce Drum Kit, i pattern dello step sequencer, le
  regioni mute e le celle di Live Loops. Le note lunghe e forti pesano più di quelle brevi e piano, e conta la
  trasposizione della regione.
- La barra di stato mostra la tonalità trovata, la confidenza (da 0 a 1) e le tonalità vicine. Le tonalità relative
  (Do maggiore e La minore) hanno le stesse note e spesso escono vicine: se la parte suona minore, scegli la
  relativa dal menu. Con poche note (meno di 8) o una confidenza bassa la barra lo dice.
- Propone solo tonalità maggiori e minori: le scale modali (Dorian, Mixolydian) si scelgono a mano.
- Una batteria suonata da un Sampler o da un plug-in VST3 non è riconosciuta come Drum Kit e le sue note entrano
  nella stima.
- Un Ctrl+Z riporta la tonalità di prima.

Agenti e assistente usano il tool `detect_key`: `region_id` analizza una sola regione MIDI, `apply: true` imposta
la tonalità trovata. Senza `apply` il tool non cambia nulla e restituisce la stima con le alternative.

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

Bassist, Keys e Melody seguono anche il metro del progetto: i ritmi di una battuta ricominciano a
ogni battuta. Il 3/4 e il 6/8 hanno ritmi propri, e in 6/8 la melodia accenta il primo e il quarto
ottavo. Negli altri metri, per esempio 5/4 o 7/8, il ritmo del 4/4 si ripete ogni quattro movimenti
e si ferma alla stanghetta. In 4/4, con lo stesso seed, le parti sono identiche a quelle delle
versioni precedenti. Arpeggio e gli stili che seguono il movimento (Root, Root Fifth, Eighths,
Octaves, Walking, Sustained, Offbeat, Ballad) non dipendono dalla battuta.

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

## De-Esser

Il De-Esser abbassa le S e le T taglienti di una voce, solo nei momenti in cui ci sono: il resto
della voce passa com'è.

- Dove si trova: nel mixer, menu **+** di uno slot insert, oppure nell'inspector con **Add effect**,
  poi **Dynamics** e **De-Esser**. Ci sono due preset: **Vocal De-Ess** e **Soft De-Ess**.
- **Frequency**: da dove cominciano le sibilanti. Da questa frequenza in su la riduzione è piena e
  sfuma circa un'ottava sotto: con 6000 Hz e 8 dB di riduzione, a 3 kHz la voce scende di circa 2 dB.
- **Threshold**: quando la banda alta supera questo livello viene abbassata, fino a riportarla alla
  soglia.
- **Range**: la riduzione massima, in dB. Con 0 il De-Esser non cambia nulla.
- **Mode**: **Split** abbassa solo le frequenze alte, **Wideband** abbassa tutto il segnale mentre
  dura la "s".
- **Listen**: fa sentire solo la banda che il De-Esser ascolta. Serve a trovare la Frequency giusta;
  ricordati di spegnerlo.
- Sotto la soglia l'audio esce identico a com'è entrato, e il De-Esser non aggiunge latenza.

In pratica: mettilo dopo l'EQ e prima del compressore, poi abbassa Threshold finché le "s" si
ammorbidiscono senza che la voce diventi sorda.

Da MCP e dall'assistente: il tool `add_effect` con `kind: "de_esser"` lo aggiunge a una traccia e
`set_plugin_params` ne cambia i parametri (`freq`, `threshold`, `range`, `mode`, `listen`). Puoi anche
chiedere all'assistente "de-ess the vocal". La patch **Lead Vocal** non lo contiene: va aggiunto a
mano.

## Bounce

- **File > Bounce to WAV…** (Ctrl+B) scrive il mix in un file WAV: tutta la canzone, oppure solo
  l'intervallo del ciclo se il ciclo è attivo.
- **File > Bounce loudness** sceglie il livello del file: Off (il mix com'è), Peak -1 dB (il picco a
  -1 dBFS), -14 LUFS (streaming), -16 LUFS, -23 LUFS (broadcast). Con un target LUFS Ondina misura la
  loudness integrata del mix e applica un solo guadagno a tutto il brano, senza limiter, tenendo i
  picchi sotto -1 dBTP. La scelta vale fino alla chiusura di Ondina, come la bit depth.
- Se il mix è troppo dinamico, il guadagno si ferma al tetto dei picchi e la barra di stato dice che
  il target non è stato raggiunto: per arrivarci serve un limiter (**Mix > Mastering Assistant**). Da
  MCP e dall'assistente: `bounce` con `normalize_lufs` (da -30 a -6) e, se serve,
  `true_peak_ceiling_db` (da -9 a 0, default -1); il risultato riporta `loudness` con `gain_db`,
  `lufs_after`, `true_peak_after` e `target_reached`. Il picco è la stima di `analyze_mix`; a 16 e
  24 bit il dither può aggiungere circa un LSB.
- Dopo la fine, Ondina continua finché le code di riverbero e delay non si spengono (sotto circa
  −90 dBFS): almeno 2 secondi, al massimo 20. Così il finale suona come in riproduzione. Vale anche
  per Bounce in Place, Bounce Surround e Bounce Binaural.
- Un suono che non si spegne mai, per esempio una nota tenuta oltre la fine o un plug-in che fa rumore,
  allunga il file fino al massimo di 20 secondi.
- Per esportare un loop di lunghezza esatta, senza coda, chiedi all'assistente o usa il tool MCP
  `bounce` con `tail_sec: 0`.

## Metronomo

**K** o un clic sul pulsante del metronomo nella barra di trasporto accendono e spengono il clic. Un clic destro
sul pulsante apre un menu:

- **Click level**: uno slider da −60 a 0 dB (−9,1 dB di partenza). Il volume cambia subito, anche durante la
  riproduzione, e un intero trascinamento si annulla con un solo Ctrl+Z.
- **Reset level**: torna a −9,1 dB.
- **Only while recording**: il clic suona solo mentre registri, non nella riproduzione normale. Il count-in prima
  della registrazione suona sempre, allo stesso livello.

Il pulsante è blu quando il clic suona, blu scuro quando il metronomo è acceso ma aspetta una registrazione (modo
**Only while recording** e niente registrazione in corso), del colore della corsia quando è spento.

Il tool MCP `set_metronome` accetta gli stessi valori con i parametri facoltativi `level_db` e
`only_while_recording`; `get_transport` e `get_project` li riportano come `metronome_level_db` e
`metronome_only_while_recording`. I progetti salvati prima di questa funzione suonano il clic al livello di sempre.

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
- Se un client MCP o l'assistente apre un progetto, i plug-in VST3 fuori da queste cartelle non vengono caricati:
  restano nel progetto e nel file, ma la traccia non suona, i bounce li saltano, la finestra del plug-in non si
  apre e l'agente li trova in `blocked_plugins`. Per sentirli salva il progetto e aprilo tu da **File → Open**,
  oppure aggiungi la loro cartella con **Add folder…** prima che l'agente apra il progetto.
- Un plug-in su una cartella di rete (`\\server\cartella`) o un file che non è `.vst3` viene rifiutato
  con un messaggio.
- Se apri un progetto che usa un plug-in rifiutato, il progetto si apre lo stesso: il plug-in resta al
  suo posto ma non suona, e la barra di stato dice quanti plug-in non sono stati caricati. Nella stessa
  riga compare anche il numero dei file audio mancanti.
- Anche le cartelle extra della scansione devono stare su un disco locale: quelle su una condivisione di rete
  vengono saltate.
- Se tieni i plug-in su un NAS, copiali in una cartella locale e aggiungili di nuovo.
- Nel mixer e nell'inspector ogni VST3 si vede col suo nome: quello che il plug-in ha dato quando l'hai
  aggiunto, oppure il nome del bundle. Se il nome è troppo lungo per lo slot, passandoci sopra col mouse lo leggi
  intero.
- Nel mixer un doppio clic su un insert VST3 o sullo strumento VST3 apre la finestra del plug-in. Sull'insert
  puoi anche usare il tasto destro → **Open plug-in window**. Il clic singolo seleziona il plug-in come prima.
- Un plug-in che non viene caricato è barrato e in grigio, nel mixer e nell'inspector: passandoci sopra col
  mouse leggi il motivo. La sua finestra non si apre.

## Assistente con modello locale

Senza chiave Anthropic l'assistente usa un modello che gira sul computer, attraverso un server
compatibile OpenAI all'indirizzo scelto in **Settings ⚙** (di default `http://127.0.0.1:8080/v1`).

- Se nessun server risponde, Ondina avvia il server llama.cpp incluso in `tools\llm` e lo chiude
  quando chiudi Ondina. Un server già acceso prima (Ollama, LM Studio, un `llama-server` avviato a
  mano) viene usato così com'è e resta acceso.
- Il modello locale riceve 46 tool, scelti perché stiano nel suo contesto. Bastano per i pulsanti ✨
  del piano roll (*continue / improve*), dello step sequencer (*write a groove*), degli editor dei
  plug-in (*design a sound*), di Live Loops (*fill the grid*), del report del mix e dei menu della
  finestra di arrangiamento. Se nel piano roll hai selezionato delle note, il modello modifica solo
  quelle: può quantizzarle, umanizzarle, trasporle, portarle nella tonalità del progetto o aggiungere
  una voce in armonia (per esempio "umanizza le note selezionate"). Claude riceve tutti i tool.
  Le lunghezze delle note (`set_note_lengths`) non sono tra i tool del modello locale: usa il menu
  **Length** o Claude.
- Sotto il titolo del pannello compaiono il motore e quanti tool riceve: col modello locale
  `local model (offline) · 46 tools`.
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

## Salvataggio sicuro e copia precedente

Quando salvi, Ondina scrive prima il progetto in un file temporaneo nuovo accanto (per esempio
`Brano.ondina.4120-0.tmp`), mai uno che esiste già, si assicura che sia tutto sul disco e solo allora lo mette al posto del vecchio. Se il disco è pieno, se
Ondina si chiude o salta la corrente durante il salvataggio, il progetto che avevi resta com'era. La riga
di stato dice perché il salvataggio non è riuscito (per esempio `not enough disk space`) e il pallino "•"
resta nel titolo.

Prima di sostituire un progetto che esiste già, Ondina tiene la versione precedente accanto, con `.bak`
in fondo al nome: `Brano.ondina.bak`. Ce n'è una sola: a ogni salvataggio prende il posto della
precedente. Vale anche quando salvano l'assistente o un client MCP.

Se accanto c'è già un file `Brano.ondina.bak` che non è un progetto (o una cartella con quel nome), Ondina
non lo tocca: salva il brano e la riga di stato dice che la copia non è stata scritta. Vale anche quando
salvano l'assistente o un client MCP.

Un file temporaneo rimasto da un salvataggio interrotto si cancella da solo al salvataggio successivo, dopo
dieci minuti; un vecchio `Brano.ondina.tmp` delle versioni precedenti si può cancellare a mano.

Per tornare alla versione precedente: in Esplora file copia `Brano.ondina.bak`, rinomina la copia in, per
esempio, `Brano (precedente).ondina` (Windows chiede conferma per il cambio di estensione) e aprila con
File > Open.

La copia si spegne in Settings, sezione **Autosave**, con **Keep a backup of the previous save** (acceso
all'inizio), poi **Save settings**.

Le impostazioni (`%APPDATA%\OndinaDAW\settings.json`) si scrivono nello stesso modo. Se all'avvio il file
non si legge (per esempio è rimasto troncato), Ondina lo rinomina in `settings.json.broken`, parte con le
impostazioni di default e lo dice nella riga di stato: chiave API, cartelle VST3 e file recenti si possono
recuperare da quel file. Se invece Windows non lascia leggere il file all'avvio (un antivirus o la
sincronizzazione del cloud lo tengono occupato), Ondina riprova per mezzo secondo; se non ci riesce parte con
le impostazioni di default, lascia il file com'è e non lo sovrascrive finché non premi **Save settings**
nella finestra Settings, che lo segnala in cima.

## Salvataggio automatico e recupero

Mentre lavori, Ondina scrive ogni 3 minuti una copia del progetto non salvato, in background: non
compare nessuna finestra e il lavoro non si ferma, nemmeno durante la riproduzione o la registrazione.
La copia si scrive solo se il progetto ha modifiche non salvate (il pallino "•" nel titolo) e solo se è
cambiato qualcosa dalla copia precedente. La prima copia arriva qualche minuto dopo la prima modifica.

Le copie finiscono nella cartella `%APPDATA%\OndinaDAW\Autosave`. Sono normali file `.ondina`: se
serve, si aprono anche con File > Open.

La copia sparisce da sola quando salvi il progetto, quando crei un progetto nuovo o ne apri un altro, e
quando chiudi Ondina normalmente.

### Dopo un crash

Se Ondina si è chiuso male (un plug-in o il driver audio lo hanno fatto cadere, oppure lo hai terminato
dal Task Manager), al riavvio compare la finestra **Recover Unsaved Work** con l'elenco delle copie
trovate: il nome del progetto, il file da cui veniva (o "never saved" se non era mai stato salvato) e da
quanto tempo è stata scritta la copia.

- **Recover** apre la copia come lavoro non salvato. Il titolo mostra il pallino "•" e la riga di stato
  ti ricorda di salvare. Il primo Save chiede sempre dove salvare e propone la cartella e il nome del
  file originale: la copia nella cartella Autosave non viene mai sovrascritta. La cronologia di Annulla
  riparte vuota, come dopo ogni Apri.
- **Discard** cancella quella copia.
- **Discard All** cancella tutte le copie dell'elenco.
- **Decide Later** (o Esc) chiude la finestra senza toccare niente: le copie restano e la finestra
  ricompare al prossimo avvio.

Se recuperi una copia, le altre restano sul disco e ricompaiono al prossimo avvio. Se il progetto aperto
ha modifiche non salvate, prima del recupero compare la solita domanda Save / Don't Save / Cancel.

### Impostazione

In Settings, nella sezione **Autosave**, scegli ogni quanti minuti scrivere la copia (da 0 a 60).
Con 0 il salvataggio automatico è spento: non si scrive nessuna copia e quella che c'era viene
cancellata. La finestra di recupero compare comunque se all'avvio trova copie rimaste da prima.

### Cosa non è coperto

- I file audio non vengono copiati: la copia li cita dove erano, come il progetto.
- Una registrazione ancora in corso al momento del crash non è nella copia: una take entra nel
  progetto solo dopo Stop.
- Se il crash arriva prima della prima copia (pochi minuti dopo la prima modifica), non c'è niente da
  recuperare.
- Se tieni aperte più finestre di Ondina, ognuna scrive le sue copie e non propone quelle di un'altra
  finestra ancora aperta.

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
| Ctrl+D | duplica la regione (con note selezionate nel piano roll: ripete le note) |
| Ctrl+T | divide la regione selezionata al playhead |
| Ctrl+B | bounce |
| Ctrl+rotella | zoom orizzontale attorno al puntatore |
| Maiusc+rotella | scorrimento orizzontale |
| Ctrl+Maiusc+rotella | altezza delle tracce (nel piano roll: altezza dei tasti) |
| Ctrl+C / Ctrl+X / Ctrl+V | copia / taglia / incolla le note selezionate nel piano roll, altrimenti la regione selezionata (incolla al playhead) |
| Ctrl+A | seleziona tutte le note (piano roll) |
| ← / → | sposta le note selezionate di un passo di snap |
| ↑ / ↓, Maiusc+↑ / ↓ | trasporta le note selezionate di un semitono / un'ottava |
| ↑ / ↓ | seleziona la traccia precedente / successiva (con note selezionate nel piano roll: trasporta le note) |
| M / S | mute / solo della traccia selezionata |
| Maiusc+M | marker al playhead |
| Alt+← / → | accorcia / allunga le note selezionate di un passo di snap |
| Alt+trascinamento | copia le note trascinate (piano roll) |
| Esc (mentre trascini) | annulla il trascinamento |
| A | mostra / nasconde l'automazione nell'arrangiamento |

`,` e `.` da metà battuta portano all'inizio della battuta; durante la riproduzione, a meno di mezzo beat da una
battuta o da un marker, `,` va a quello prima. Queste scorciatoie, Ctrl+T, ↑ / ↓ per le tracce, A, M, S e
Maiusc+M sono spente mentre scrivi in un campo di testo, durante un trascinamento e con Musical Typing attivo (lì
A, S e M suonano note).
