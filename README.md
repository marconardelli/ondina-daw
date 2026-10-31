# Aurora DAW

Una digital audio workstation nativa per **Windows**, progettata usando come riferimento funzionale i manuali utente di Logic Pro (non inclusi nel repository; prodotto originale, nessun asset o marchio Apple), con:

- **motore audio real-time in Rust** (WASAPI via `cpal`, nessun garbage collector nel thread audio);
- **AI integrata**: un assistente Claude dentro l'app che opera direttamente sul progetto;
- **API completa via MCP**: *ogni* operazione dell'app è un tool MCP (156 tool), sia verso l'app in esecuzione (HTTP) sia headless (stdio).

## Perché Rust

Il thread audio deve produrre un buffer ogni ~1–10 ms senza mai fermarsi. Rust offre prestazioni pari a C/C++ (compilazione nativa, SIMD, zero-cost abstractions), ma **senza garbage collector** (nessuna pausa imprevedibile, a differenza di C#/Java/Go) e con **sicurezza della memoria e dei thread verificata dal compilatore**, fondamentale quando GUI, motore audio, server MCP e agente AI lavorano in parallelo sullo stesso progetto.

## Architettura

```
crates/
  core    modello del progetto, enum Command (= l'API), teoria musicale, generatori, MIDI I/O
  dsp     strumenti (6), effetti (16), effetti MIDI (4), Flex/Stem/tempo (audio_ops), spatial, host VST3
  engine  renderer, motore real-time cpal/WASAPI, Live Loops, registrazione, bounce stereo/surround, analisi LUFS
  studio  Session (esegue i Command, undo/redo), server MCP (HTTP + stdio + bridge), agente Claude
  app     GUI egui: aurora.exe (desktop) + aurora-mcp.exe (console, per client MCP stdio)
```

Il principio chiave: **un'unica API**. Ogni azione (GUI, undo/redo, MCP, assistente AI) è un `Command` serializzabile (`crates/core/src/command.rs`). Lo schema JSON di ogni comando viene generato automaticamente ed esposto come tool MCP con la sua documentazione: aggiungere un comando lo rende subito disponibile a GUI, MCP e AI.

```
 GUI ─┐
 MCP ─┼─► Session::execute(Command) ─► Project (+undo) ─► snapshot Arc ─► Engine (thread audio)
 AI  ─┘
```

## Funzionalità (mappate sui capitoli dei manuali)

Ogni riga è disponibile sia nella GUI sia come tool MCP (156 tool in totale, elenco in [`MCP_TOOLS.md`](MCP_TOOLS.md)).

| Area del manuale | In Aurora | Tool MCP principali |
|---|---|---|
| Progetti, proprietà, tempo/metrica/tonalità | file `.aurora`, cambi di tempo, count-in, note/testi | `new_project` `save_project` `set_tempo` `add_tempo_change` `set_key` |
| Riproduzione, ciclo, metronomo | trasporto sample-accurate, ciclo, click, count-in | `play` `stop` `set_cycle` `set_metronome` `set_count_in` |
| Tracce, Library/patch | instrument, audio, aux bus, 23 patch | `add_track` `apply_patch` `list_patches` |
| Registrazione audio e MIDI | input WASAPI e porte MIDI, Musical Typing | `record` `set_record_arm` `connect_midi_input` `send_live_midi` |
| **Comping** | la registrazione in ciclo crea un take per passaggio; comp per segmenti | `list_takes` `comp_takes` |
| Arrangiamento, regioni, fade, traccia accordi, marker di sezione | drag/copy/resize/split/join/loop, sezioni copiabili | `move_region` `split_region` `copy_section` `set_chord_progression` |
| Piano Roll, Step Sequencer | editing completo, quantize, humanize, velocity | `add_notes` `quantize_notes` `set_pattern_row` |
| **MIDI Transform / Environment** | trasformazioni condizionali; split/layer dell'input MIDI | `transform_notes` `set_midi_routes` |
| Session Players / Drummer | batteria (13 stili), basso, accordi, arpeggi, melodie | `generate_drums` `generate_bass` `generate_chords` … |
| **Flex Time / Flex Pitch / Varispeed** | time-stretch WSOLA, pitch shift, varispeed, pitch correction sulla tonalità, audio→MIDI (YIN) | `time_stretch_region` `pitch_shift_region` `varispeed_region` `pitch_correct_region` `audio_to_midi` |
| **Smart Tempo** | stima del BPM da audio | `detect_tempo` |
| **Stem Splitter** | rete neurale (Spleeter, ONNX Runtime) per voce/accompagnamento + separazione spettrale di batteria/basso/altro; motore solo-DSP disponibile | `split_stems` |
| Mixer, send/bus, automazione | canali con meter, routing, automazione di volume/pan/send/parametri | `set_volume` `add_send` `set_automation` |
| **Live Loops** | griglia celle × scene, lancio quantizzato nel motore, registrazione della performance nella timeline | `create_live_cell` `launch_cell` `launch_scene` `commit_live_performance` |
| **Partitura** | vista Score (pentagramma, valori, legature, testi), testi, export MusicXML | `get_score` `set_lyrics` `export_music_xml` |
| **Surround / Spatial Audio** | 5.1, 7.1, 7.1.4, panner 3D VBAP, traiettorie automatizzate, monitoraggio e bounce binaurale | `set_surround_format` `set_surround_pan` `bounce_surround` `bounce_binaural` |
| **Mix object-based (Dolby Atmos)** | tracce come oggetti audio con traiettorie, export **ADM BWF** (ITU-R BS.2076 + chna BS.2088, 48 kHz/24 bit): bed 7.1.2 + oggetti | `set_surround_pan` (object) `export_adm` |
| **Video e sincronizzazione** | film collegato con offset, audio del film, timecode SMPTE, finestra video, export del film con la colonna sonora | `load_movie` `set_frame_rate` `extract_movie_frame` `export_movie` |
| Strumenti integrati | Analog Synth, FM Synth, Drum Synth, Sampler, Tonewheel Organ, Plucked String | `set_instrument` `set_plugin_params` |
| Effetti integrati | 16 effetti (EQ, dinamica, delay, riverbero, modulazione, distorsione, filtri, utility) + 4 effetti MIDI | `add_effect` `add_midi_effect` |
| **Plug-in di terze parti (VST3)** | scansione e hosting di strumenti ed effetti VST3, **finestre native dei plug-in** sincronizzate col progetto, parametri automatizzabili, salvataggio nel progetto | `scan_vst3` `add_vst3` `open_plugin_editor` `set_plugin_param` |
| Mastering Assistant, misure di loudness | LUFS integrati, true peak, LRA, spettro, correlazione, consigli; mastering automatico | `analyze_mix` `auto_master` |
| Bounce ed export | WAV 16/24/32f con dither, MIDI, bounce in place, surround multicanale | `bounce` `export_midi` `bounce_in_place` |
| Impostazioni | modello/effort AI, dispositivi audio, MIDI | `get_settings` `set_ai_settings` `set_audio_device` |

### Come è stato verificato

- `cargo test --workspace`: **28 test** (DSP, motore, sessione, MCP via HTTP, agente AI contro un server mock, più 7 test end-to-end delle nuove funzioni), incluso un test che **chiama ogni tool MCP** e fallisce se uno solo non è implementato.
- **Plug-in VST3 reali di terze parti**: Dexed (strumento FM, 2238 parametri) e Airwindows Consolidated (effetto) caricati, suonati in tempo reale sul dispositivo WASAPI, analizzati, salvati e riaperti.
- **Assistente AI reale, offline**: con Qwen2.5-3B-Instruct in locale (llama.cpp avviato automaticamente da Aurora) la richiesta "imposta 90 bpm, aggiungi una batteria Pop Drum Kit e genera 4 battute rock" è stata eseguita in 18 s con 3 tool concatenati; una richiesta in italiano (tonalità, accordi Am F C G, basso root_fifth) è stata completata anche correggendo da sola un id errato.
- **MIDI reale via WinMM (porta loopMIDI)**: un processo esterno ha suonato 5 note; Aurora le ha registrate con tempi corretti (0,7 beat = 350 ms a 120 BPM) e lo split dell'Environment ha mandato il C3 al basso trasposto (C2) e le note alte al piano.
- **Registrazione hardware reale**: registrazione dal microfono integrato mentre le casse suonavano note del synth; la trascrizione audio→MIDI della registrazione ha riconosciuto la nota suonata (E5).
- **ADM validato col renderer di riferimento EBU** (`ear`, ITU-R BS.2127): il file è stato letto e renderizzato in 5.1 e 7.1.4; l'oggetto automatizzato risulta a sinistra all'inizio, al centro a metà e a destra alla fine (±11 dB fra i diffusori laterali).
- **Stem Splitter ML su un brano reale**: 10,9 s separati in 0,38 s; lo stem voce è a zero nell'intro e nella coda strumentali e attivo nelle strofe; ricostruzione esatta voce + accompagnamento.
- **Finestra nativa di Dexed** aperta da MCP: trascinando la manopola Cutoff col mouse il parametro passa a 0,52 nel progetto (annullabile) e raggiunge l'istanza in riproduzione.
- **Video reale**: filmato H.264/AAC generato, collegato, fotogrammi estratti nella finestra video, export con colonna sonora.
- GUI verificata con screenshot di arrangiamento, mixer, Piano Roll, Score, Live Loops, panner surround, finestra video ed editor VST3.

### Limiti attuali

- Lo **Stem Splitter ML** separa voce e accompagnamento con una rete neurale; batteria e basso vengono poi estratti con DSP dall'accompagnamento (non da una rete a 4 stem). Il modello (~38 MB) va in `tools\models`.
- L'export **ADM BWF** segue gli standard ITU aperti ed è validato con il renderer EBU; il **Dolby Atmos Renderer** proprietario non è disponibile qui, quindi l'importazione in quel software specifico non è stata provata.
- Le **finestre native VST3** usano un'istanza "proxy" dedicata all'interfaccia; parametri e **stato completo** (programmi, banchi: `save_state`/`load_state`) vengono sincronizzati con l'istanza che suona e salvati nel progetto. Lo stato si trasferisce ogni secondo circa e alla chiusura della finestra.
- Frame video ed export del film richiedono **ffmpeg** (nel PATH oppure in `tools\ffmpeg\bin`); l'audio dei film è decodificato in modo nativo e, se serve, con ffmpeg.
- L'**input MIDI** è stato provato end-to-end attraverso il sottosistema MIDI di Windows con una porta virtuale loopMIDI (la stessa API WinMM usata dalle tastiere USB); una tastiera fisica non era disponibile.
- Con un modello locale da 3B il pannello AI segue bene richieste dirette, ma è meno capace di Claude su compiti lunghi o creativi complessi. Il provider Claude (opzionale) è verificato contro un server mock che riproduce l'API.

## Compilare ed eseguire

Requisiti: Rust stable (MSVC) su Windows 10/11.

```powershell
cargo build --release
.\target\release\aurora.exe                         # GUI + server MCP su http://127.0.0.1:3917/mcp
.\target\release\aurora.exe examples\demo-song.aurora
cargo test --workspace                              # 28 test, inclusa la copertura di tutti i tool MCP
```

Opzioni: `--no-audio` (null device), `--mcp-port N`, `--no-mcp`.

## AI integrata

L'AI è integrata a tre livelli, tutti sopra la stessa API di comandi:

1. **MCP (nessuna chiave richiesta)**: qualsiasi agente AI (Claude Code, Claude Desktop, altri client MCP) pilota Aurora con 156 tool.
2. **Assistente interno offline**: il pannello **✨ AI Assistant** usa di default un **modello linguistico locale** tramite un server compatibile OpenAI (llama.cpp, Ollama, LM Studio…). Aurora avvia automaticamente il server llama.cpp incluso se trova un modello `.gguf` in `tools\llm` (es. Qwen2.5-3B-Instruct). Nessun account, nessun dato esce dalla macchina; ai modelli piccoli viene offerto un sottoinsieme curato di 31 tool.
3. **Assistente con Claude (opzionale)**: con una chiave Anthropic, il pannello può usare `claude-opus-5-5` con tutti i 156 tool.

Provider selezionabile in **Settings ⚙ → Provider** (Auto / Local / Claude API): *Auto* usa Claude se è configurata una chiave, altrimenti il modello locale. Il pannello riceve il contesto della selezione corrente (traccia, regione, note, playhead) e offre azioni rapide; pulsanti AI contestuali sono in Piano Roll, Step Sequencer, editor dei plug-in e menu delle regioni. Ogni modifica è annullabile. L'agente "ascolta" il mix tramite `analyze_mix`.

## MCP: controllare Aurora da qualsiasi agente

| Modalità | Comando |
|---|---|
| App in esecuzione (Streamable HTTP) | `http://127.0.0.1:3917/mcp` |
| Claude Code | `claude mcp add --transport http aurora http://127.0.0.1:3917/mcp` |
| Client stdio (Claude Desktop…) → app in esecuzione | `aurora-mcp.exe` |
| Sessione headless senza GUI | `aurora-mcp.exe --headless [--audio] [--project file.aurora]` |

Esempio di `claude_desktop_config.json`:

```json
{ "mcpServers": { "aurora": { "command": "C:\\path\\to\\aurora-mcp.exe" } } }
```

Il server espone **tools** (156, con annotazioni read-only/destructive), **resources** (`aurora://guide`, `aurora://project`, `aurora://project/document`, `aurora://plugins`, `aurora://patches`) e **prompts** (`compose_song`, `mix_review`, `arrange_sections`). Ascolta solo su `127.0.0.1`, rifiuta Origin non locali (protezione dal DNS rebinding) e richiede un bearer token se è impostato `AURORA_MCP_TOKEN`. Elenco completo dei tool: [`MCP_TOOLS.md`](MCP_TOOLS.md).

Convenzioni dell'API: posizioni in **beat** (semiminime, beat 0 = battuta 1), note relative all'inizio della regione, pitch MIDI 60 = C4, `track_id` 0 = master.

## Scorciatoie

Spazio play/stop · R registra · C ciclo · K metronomo · Home inizio · Ctrl+Z/Y undo/redo · Canc elimina · Ctrl+D duplica · Ctrl+S/O/N · Ctrl+B bounce · doppio clic su una corsia = nuova regione MIDI · Alt+drag = copia · Ctrl+rotella = zoom · Shift+drag sul righello = ciclo · Musical Typing: A W S E D F T G Y H U J K (Z/X ottava).
