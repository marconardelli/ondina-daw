# Plugin VST professionali più diffusi

Elenco dei plugin (VST3, spesso anche AU/AAX/CLAP) più usati negli studi professionali e nelle produzioni
home-studio di alto livello, divisi per categoria. Serve come riferimento per capire cosa gli utenti si aspettano
di poter caricare in una DAW e come banco di prova per la compatibilità dell'host.

Aggiornato a novembre 2026. La lista è basata su diffusione e reputazione nel settore, non su test fatti qui.

Legenda colonna "Formati": V3 = VST3, AU = Audio Unit (solo macOS), AAX = Pro Tools, CLAP.

---

## PRIORITÀ: ogni plug-in si apre nella sua finestra grafica

È il requisito più importante del documento. Oggi i plug-in si regolano dentro l'inspector, di lato, con una lista
di barre orizzontali con il nome del parametro: è scomodo e brutto. In ogni DAW professionale, quando clicchi su
uno strumento o un effetto della traccia si apre la **finestra del plug-in**, con la sua grafica, e lo regoli lì.
Ondina deve fare lo stesso.

**Finché la finestra grafica non esiste ed è usata dai plug-in che ci sono già, è il lavoro con la precedenza su
tutte le altre voci**: ogni ciclo dedica almeno un ramo a costruirla o a portarci i plug-in esistenti, e una
proposta che aggiunge un nuovo plug-in con la vecchia lista di barre non è finita.

1. **Si apre con un clic.** Clic (o doppio clic) sullo slot del plug-in nel mixer, nel channel strip o
   nell'inspector apre la finestra del plug-in. Un secondo clic la porta in primo piano. Si possono tenere aperte
   più finestre insieme; si chiudono con la X o con Esc.
2. **Una finestra vera.** È una finestra separata (multi-viewport di egui), che si sposta anche su un secondo
   monitor, si ridimensiona e ricorda posizione e dimensione per plug-in. Il titolo dice plug-in e traccia.
3. **Intestazione comune a tutti i plug-in:** nome, traccia, bypass, menu dei preset (carica, salva), confronto
   A/B, la barra AI del plug-in (vedi i requisiti sotto), e il pulsante per tornare ai valori di default.
4. **Controlli grafici, non liste.** Manopole e controlli disegnati (trascini su e giù, Shift per la regolazione
   fine, doppio clic per il default, valore e unità mentre trascini e al passaggio del mouse, clic destro per
   scrivere il valore). Niente più colonne di slider orizzontali con il nome a fianco.
5. **La grafica fa capire cosa succede.** Ogni tipo di plug-in mostra il suo suono:
   - EQ: analizzatore di spettro in tempo reale e curva con i punti delle bande da trascinare (frequenza e
     guadagno col drag, Q con la rotella);
   - compressore, limiter, de-esser, gate: curva di trasferimento, misuratore di riduzione del guadagno,
     misuratori di ingresso e uscita;
   - riverbero e delay: rappresentazione del decadimento e dei tempi, sincronizzati al tempo del progetto;
   - saturazione, nastro, amplificatore: curva di trasferimento o pannello in stile hardware (grafica originale di
     Ondina);
   - stereo imager: vettorscopio e correlazione;
   - sintetizzatori: forma d'onda o wavetable dell'oscillatore, inviluppi ADSR da trascinare, curva del filtro,
     tastiera cliccabile per provare il suono;
   - campionatori e batteria: forma d'onda del campione, pad cliccabili.
6. **Stile di Ondina.** Grafica originale, coerente con il tema dell'app (colori e tipografia di `theme.rs`),
   leggibile sul tema scuro. Nessuna copia di interfacce di prodotti esistenti.
7. **Stesse regole del prodotto.** Ogni modifica passa da un comando con undo (un trascinamento è un solo passo di
   undo). I misuratori leggono dal motore senza lock che blocchino l'audio. La finestra non rallenta la GUI.
8. **L'inspector diventa un riassunto.** Nell'inspector resta solo una riga compatta per plug-in (nome, bypass,
   pulsante per aprire la finestra), non più l'elenco dei parametri.
9. **VST3 di terzi.** Lo stesso clic apre la loro finestra nativa, se ce l'hanno; se non ce l'hanno, una finestra
   generica di Ondina con manopole al posto delle barre.
10. **Tutti i plug-in esistenti passano alla finestra**, non solo i nuovi. La prima fetta costruisce la finestra,
    l'intestazione comune e le manopole e ci porta i plug-in interni con la vista generica; le fette successive
    aggiungono le grafiche specifiche (EQ, dinamica, synth...) una famiglia per volta.

## Requisiti per ogni plug-in (valgono per tutte le voci)

Ogni strumento o effetto costruito in Ondina a partire da questo documento ha l'AI integrata. Non è una voce a sé:
è un requisito di ogni voce, e una voce senza AI non è finita.

1. **Richiesta AI dentro il plug-in.** Nel pannello del plug-in c'è una casella di testo con il bottone di invio
   (lo stesso bottone AI dell'app, con gli stessi stati: disattivato senza modello o mentre l'assistente lavora).
   L'utente scrive cosa vuole ("più aria sulla voce", "togli il rimbombo sotto i 200 Hz", "un pad caldo che si
   apre lentamente") e l'AI regola quel plug-in.
2. **L'AI sa su cosa sta lavorando.** La richiesta parte con il contesto del plug-in: id, tipo, traccia su cui si
   trova e suo ruolo, valori attuali e intervalli di tutti i parametri. L'assistente può leggere l'audio della
   traccia con gli strumenti di analisi che ci sono già quando serve.
3. **Azioni rapide specifiche.** Oltre alla casella, due o tre richieste pronte adatte al plug-in (un EQ: "pulisci
   le frequenze che impastano"; un limiter: "porta a -14 LUFS"; un synth: "dammi un basso per questo pezzo").
   Il bottone generico "AI: design a sound" di oggi diventa una di queste.
4. **Si annulla con undo.** L'AI cambia i parametri solo con i comandi della sessione (`SetPluginParam` e
   simili): ogni modifica dell'AI si annulla con un undo e si vede subito nei controlli del pannello.
5. **Spiega cosa ha fatto.** La risposta dice in poche righe quali parametri ha cambiato e perché, nella chat
   dell'assistente.
6. **Funziona con entrambi i modelli.** Con Claude e con il modello locale, senza bloccare la GUI e
   interrompibile con Stop.
7. **Anche via MCP.** Un agente esterno può fare la stessa cosa: leggere i parametri del plug-in con il loro
   significato e regolarli.
8. **Un solo componente.** La barra AI del plug-in è un componente comune riusato da tutti i plug-in, compresi
   quelli interni che ci sono già e l'editor generico dei VST3 di terzi. La prima voce che tocca i plug-in lo
   costruisce, le altre lo riusano.

---

## 1. Suite e bundle di riferimento

| Produttore | Prodotto | Note | Formati |
|---|---|---|---|
| FabFilter | Total Bundle (Pro-Q 4, Pro-C 2, Pro-L 2, Pro-R 2, Saturn 2, Timeless 3, Pro-MB, Pro-DS, Pro-G, Volcano 3, Twin 3) | Standard di fatto per EQ, dinamica e limiting; GUI con resize e analizzatore | V3, AU, AAX, CLAP |
| Waves | Mercury / Waves Creative Access (SSL, CLA, API, Abbey Road, L2, Renaissance, H-Delay) | La collezione più diffusa al mondo, presente in quasi ogni studio | V3, AU, AAX |
| Universal Audio | UAD Native (1176, LA-2A, Pultec, Neve 1073, Studer A800, Ampex ATR-102, Lexicon 224) | Emulazioni hardware di riferimento, ora anche native senza DSP | V3, AU, AAX |
| iZotope | Ozone 11 (mastering), Neutron 4 (mix), RX 11 (restauro audio), Nectar 4 (voce) | Assistenti con analisi automatica; RX è lo standard per la pulizia audio | V3, AU, AAX |
| Soundtoys | Soundtoys 5 (Decapitator, EchoBoy, Little AlterBoy, Crystallizer, PrimalTap, MicroShift) | Saturazione e effetti creativi, molto usati in pop/rock | V3, AU, AAX |
| Plugin Alliance / Brainworx | bx_console, Black Box HG-2, SPL, Maag EQ4, Shadow Hills, Lindell | Emulazioni console e mastering; tecnologia TMT | V3, AU, AAX |
| Slate Digital | All Access (VMR, FG-X, Virtual Mix Rack, Fresh Air, Infinity EQ) | Molto diffusi in home studio | V3, AU, AAX |
| Native Instruments | Komplete 15 (Kontakt, Massive X, Battery, Guitar Rig, Reaktor, librerie) | Ecosistema strumenti più grande; Kontakt è il player di librerie standard | V3, AU, AAX |
| Arturia | V Collection 10 + FX Collection 5 | Emulazioni sintetizzatori ed effetti vintage | V3, AU, AAX |

## 2. Equalizzatori

| Plugin | Produttore | Perché è usato |
|---|---|---|
| Pro-Q 4 | FabFilter | EQ dinamico/lineare, mid-side, spectral dynamics; il più usato in assoluto |
| Pultec EQP-1A | UAD / Waves / Softube | Carattere sulle basse, classico sul master e sulla cassa |
| Neve 1073 / 1084 | UAD / Waves | Colore console su voci e strumenti |
| SSL E/G Channel | Waves / SSL Native / UAD | Channel strip di riferimento |
| Kirchhoff-EQ | Three-Body Technology | Alternativa moderna a Pro-Q con molti modelli analogici |
| TDR Nova | Tokyo Dawn Records | EQ dinamico, versione gratuita molto diffusa |
| Gullfoss | Soundtheory | EQ "intelligente" che bilancia automaticamente lo spettro |
| SPL Passeq / Maag EQ4 | Plugin Alliance | Mastering e "air" |

## 3. Compressori e dinamica

| Plugin | Produttore | Perché è usato |
|---|---|---|
| Pro-C 2 / Pro-MB | FabFilter | Compressore e multibanda trasparenti e versatili |
| 1176 / LA-2A | UAD / Waves / Softube | FET e ottico classici, onnipresenti su voci e batteria |
| SSL G-Bus Compressor | Waves / SSL / Cytomic The Glue | Compressione di bus e "colla" del mix |
| Fairchild 670 | UAD / Waves | Valvolare vintage per bus e master |
| Distressor | UAD (Empirical Labs) | Versatile su batteria e voci |
| Shadow Hills Mastering Compressor | Plugin Alliance | Mastering |
| Oxford Dynamics / Inflator | Sonnox | Dinamica pulita, Inflator per loudness |
| DeEsser: Pro-DS, Waves DeEsser, Soothe 2 | FabFilter / Waves / oeksound | Soothe 2 è lo standard per risonanze e sibilanti |

## 4. Limiter, mastering e loudness

| Plugin | Produttore | Perché è usato |
|---|---|---|
| Pro-L 2 | FabFilter | Limiter di riferimento, true peak e metering LUFS |
| Ozone 11 | iZotope | Suite di mastering completa con assistente |
| L2 / L3 Ultramaximizer | Waves | Storico, ancora molto diffuso |
| Weiss MM-1 / DS1-MK3 | Softube | Qualità da mastering studio |
| Elevate | Newfangled Audio (Eventide) | Limiter multibanda moderno |
| Youlean Loudness Meter 2 | Youlean | Metering LUFS, standard (versione gratuita) |
| Insight 2 | iZotope | Metering completo per broadcast |
| SPAN | Voxengo | Analizzatore di spettro gratuito molto usato |

## 5. Riverberi

| Plugin | Produttore | Perché è usato |
|---|---|---|
| Valhalla VintageVerb / Room / Supermassive | Valhalla DSP | Ottimo rapporto qualità/prezzo, ovunque; Supermassive gratuito |
| Pro-R 2 | FabFilter | Riverbero algoritmico trasparente |
| Lexicon 224 / 480L | UAD / Relab LX480 | Suono dei dischi anni '80-'90 |
| Altiverb 8 | Audio Ease | Convoluzione di riferimento per post-produzione e cinema |
| Seventh Heaven | LiquidSonics | Emulazione Bricasti M7 |
| Blackhole / H3000 / SP2016 | Eventide | Riverberi creativi e classici |
| AMS RMX16 | UAD | Classico sui rullanti |
| Raum / Replika | Native Instruments | Gratuiti con Komplete Start |

## 6. Delay ed effetti di modulazione

| Plugin | Produttore | Perché è usato |
|---|---|---|
| EchoBoy | Soundtoys | Delay più usato in assoluto, emulazioni nastro/analogiche |
| Timeless 3 | FabFilter | Delay modulabile |
| H-Delay | Waves | Semplice e diffuso |
| ValhallaDelay | Valhalla DSP | Molti modi (tape, BBD, digital) |
| Galactic Delay / Replika XT | Native Instruments | Creativi |
| Space Echo RE-201 | Roland / UAD | Tape echo classico |
| MicroShift / H910 Harmonizer | Soundtoys / Eventide | Allargamento stereo delle voci |
| Chorus/Phaser: Arturia FX Collection, Soundtoys PhaseMistress | Arturia / Soundtoys | Modulazioni vintage |

## 7. Saturazione, distorsione e colore

| Plugin | Produttore | Perché è usato |
|---|---|---|
| Decapitator | Soundtoys | Saturazione analogica più usata |
| Saturn 2 | FabFilter | Multibanda, molto flessibile |
| Black Box HG-2 | Plugin Alliance | Saturazione valvolare per master e bus |
| Studer A800 / Ampex ATR-102 | UAD | Emulazioni nastro di riferimento |
| Kramer Master Tape / J37 | Waves | Nastro |
| Trash 2 | iZotope | Distorsione creativa |
| Softube Tape | Softube | Nastro semplice |
| CHOW Tape Model | Chowdhury DSP | Gratuito e open source |

## 8. Voce e correzione d'intonazione

| Plugin | Produttore | Perché è usato |
|---|---|---|
| Auto-Tune Pro X | Antares | Standard per l'effetto e la correzione in tempo reale |
| Melodyne 5 | Celemony | Editing di pitch e timing di riferimento (ARA) |
| Waves Tune Real-Time | Waves | Alternativa economica |
| Nectar 4 | iZotope | Catena vocale completa |
| VocalSynth 2 | iZotope | Effetti vocali creativi |
| Little AlterBoy | Soundtoys | Formanti e pitch |
| Revoice Pro / VocAlign | Synchro Arts | Allineamento delle voci doppie |

## 9. Restauro e pulizia audio

| Plugin | Produttore | Perché è usato |
|---|---|---|
| RX 11 | iZotope | Standard per rumore, click, hum, de-reverb, separazione |
| Accentize dxRevive / VoiceGate | Accentize | Ricostruzione dialoghi con IA |
| Waves Clarity Vx | Waves | Riduzione rumore voce con IA |
| Acon Digital Restoration Suite | Acon Digital | Alternativa a RX |
| Cedar Studio | Cedar | Broadcast e forense |

## 10. Sintetizzatori

| Plugin | Produttore | Perché è usato |
|---|---|---|
| Serum 2 | Xfer Records | Wavetable più usato nella musica elettronica |
| Vital | Matt Tytel | Wavetable gratuito, alternativa a Serum |
| Omnisphere 2 | Spectrasonics | Enorme libreria, standard in colonne sonore e pop |
| Diva / Zebra 3 / Repro / Hive 2 | u-he | Emulazioni analogiche di altissima qualità |
| Massive X / Massive | Native Instruments | Bassi e lead elettronici |
| Pigments 5 | Arturia | Sintetizzatore ibrido moderno |
| Sylenth1 | LennarDigital | Classico per lead trance/EDM |
| Phase Plant | Kilohearts | Sintesi modulare |
| Spire | Reveal Sound | EDM |
| Arturia V Collection (Mini V, Jup-8 V, CS-80 V, Prophet-5 V) | Arturia | Emulazioni vintage |
| TAL-U-NO-LX / TAL-Sampler | TAL Software | Juno-60 emulato, economico |
| OB-Xd / Surge XT | discoDSP / Surge Synth Team | Open source, molto diffusi |

## 11. Campionatori, pianoforti e orchestra

| Plugin | Produttore | Perché è usato |
|---|---|---|
| Kontakt 8 | Native Instruments | Player di librerie standard del settore |
| Keyscape | Spectrasonics | Pianoforti e tastiere |
| Pianoteq 8 | Modartt | Pianoforte a modellazione fisica, leggerissimo |
| Noire / The Grandeur | Native Instruments | Pianoforti campionati |
| Spitfire BBC Symphony Orchestra, LABS | Spitfire Audio | Orchestra; LABS gratuito e molto diffuso |
| Orchestral Tools (Berlin Series, SINE) | Orchestral Tools | Orchestra per colonne sonore |
| Vienna Symphonic Library (Synchron, VI) | VSL | Orchestra di riferimento |
| EastWest Composer Cloud (Hollywood Orchestra, Play/Opus) | EastWest | Molto diffuso in abbonamento |
| Native Instruments Symphony Series | NI | Orchestra |
| Decent Sampler | Decent Samples | Player gratuito di librerie |

## 12. Batteria e percussioni

| Plugin | Produttore | Perché è usato |
|---|---|---|
| Superior Drummer 3 / EZdrummer 3 | Toontrack | Batterie acustiche di riferimento |
| BFD 3 | inMusic | Alternativa a Superior |
| Addictive Drums 2 | XLN Audio | Semplice e diffuso |
| Steven Slate Drums 5.5 | Slate Digital | Rock/metal |
| Battery 4 | Native Instruments | Drum sampler elettronico |
| XO | XLN Audio | Organizzazione campioni di batteria |
| Microtonic | Sonic Charge | Drum synth |
| Trigger 2 | Slate Digital | Sostituzione e rinforzo dei colpi |

## 13. Chitarra e basso

| Plugin | Produttore | Perché è usato |
|---|---|---|
| Neural DSP Archetype (Gojira, Plini, Nolly, Cory Wong, ecc.) | Neural DSP | Standard attuale per ampli di chitarra |
| Guitar Rig 7 | Native Instruments | Multieffetto completo |
| Amplitube 5 | IK Multimedia | Molti modelli ufficiali (Fender, Mesa, Orange) |
| BIAS FX 2 / Amp 2 | Positive Grid | Diffuso tra i chitarristi |
| Helix Native | Line 6 | Stessi modelli dell'Helix hardware |
| Darkglass / Ampeg SVT (Plugin Alliance, Softube) | vari | Basso |
| Ample Guitar / Ample Bass | Ample Sound | Chitarre e bassi virtuali |

## 14. Utility, stereo e immagine

| Plugin | Produttore | Perché è usato |
|---|---|---|
| Ozone Imager | iZotope | Allargamento stereo, gratuito |
| Metric AB | ADPTR Audio | Confronto con tracce di riferimento |
| REFERENCE | Mastering The Mix | Confronto con reference |
| SoundID Reference | Sonarworks | Correzione acustica di stanza e cuffie |
| Kickstart 2 / LFO Tool | Nicky Romero / Xfer | Sidechain ducking |
| ShaperBox 3 | Cableguys | Modulazioni ritmiche di volume, filtro, pan |
| bx_solo / bx_control | Plugin Alliance | Mid-side e utility, gratuiti |
| Melda MAutoPitch / MFreeFXBundle | MeldaProduction | Bundle gratuito molto ampio |

## 15. Plugin gratuiti molto diffusi (utili per i test)

Comodi per verificare l'hosting VST3 senza licenze:

- **Vital** (synth wavetable)
- **Surge XT** (synth, open source, VST3 + CLAP)
- **Valhalla Supermassive** (riverbero/delay)
- **TDR Nova**, **TDR Kotelnikov** (EQ dinamico, compressore)
- **Spitfire LABS** (strumenti campionati)
- **Youlean Loudness Meter 2**, **Voxengo SPAN** (metering)
- **MeldaProduction MFreeFXBundle** (decine di effetti)
- **CHOW Tape Model**, **OB-Xd**, **Dexed** (open source)
- **Kilohearts Essentials** (effetti base)
- **Native Instruments Komplete Start** (Kontakt Player, Raum, Replika, synth)
- **Decent Sampler**, **Pianoteq** (demo)

---

## Note per Ondina

- Quasi tutti i plugin elencati sono disponibili come **VST3 a 64 bit** su Windows; il VST2 è ormai legacy e
  diversi produttori (Steinberg ha chiuso l'SDK VST2) non lo distribuiscono più per i nuovi prodotti.
- Il formato **CLAP** sta crescendo (FabFilter, u-he, Surge XT, Arturia, Kilohearts): vale la pena tenerlo
  presente.
- Aspetti dell'host che questi plugin mettono alla prova: GUI ridimensionabili e HiDPI (FabFilter, Serum 2,
  Neural DSP), latenza dichiarata e compensazione (limiter lookahead, EQ a fase lineare, Ozone), sidechain
  (compressori, Kickstart, Trigger), ARA (Melodyne, RX, Revoice Pro), MPE (Pigments, Vital, Phase Plant),
  attivazione con iLok/licenze online (Waves, UAD, Plugin Alliance, Slate), librerie pesanti con streaming da disco
  (Kontakt, Omnisphere, Superior Drummer).
- Per un primo giro di test di compatibilità: Surge XT, Vital, Valhalla Supermassive, TDR Nova, Youlean e
  Spitfire LABS coprono synth, effetti, sidechain, latenza e GUI ridimensionabili a costo zero.
