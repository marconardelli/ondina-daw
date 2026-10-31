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

Anche l'assistente AI vede quali note sono selezionate. Puoi dirgli, per esempio, "quantizza
queste note" e lui agirà solo sulla selezione.

## Annulla e ripeti

- Ctrl+Z annulla l'ultima modifica, Ctrl+Y la ripete.
- Dopo **Nuovo** (Ctrl+N) o **Apri** (Ctrl+O) la cronologia riparte da zero: Ctrl+Z non riporta
  indietro il progetto precedente.
- Un progetto appena aperto non risulta modificato: Aurora non chiede di salvare finché non
  cambi qualcosa.

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
