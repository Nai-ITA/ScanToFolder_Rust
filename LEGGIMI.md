# Scan to Folder 2.0 — Rust (Windows, Linux, macOS)

Conversione in Rust di `scan_setup_gui_nai_v2.py`: un solo eseguibile, niente Python.
Interfaccia Tauri chiara/scura come l'agente SNMP, con tre schede.

| | Windows | Linux | macOS |
|---|---|---|---|
| **SMB / cartella condivisa** | come la versione Python: cartella, utente locale, permessi NTFS, share, firewall, SMB v1, policy | Samba (installato se manca), utente, permessi, `smb.conf`, SMB v1 opzionale | condivisione SMB di sistema, utente nascosto con accesso SMB |
| **FTP di sistema** | IIS (installato e configurato) | vsftpd (installato e configurato) | non disponibile |
| **FTP integrato** | server FTP dentro il programma | uguale | uguale |

## Privilegi di amministratore
Il programma parte come utente normale. Ogni operazione su utenti, condivisioni e servizi
chiede l'autorizzazione al momento: UAC su Windows, la finestra di autorizzazione su Linux
(pkexec), la password di amministratore su macOS. Il server FTP integrato non ne ha bisogno.

## FTP integrato
- Riceve le scansioni finché il programma è aperto. Chiudendo la finestra resta nell'area di
  notifica; "avvia con il computer" lo fa partire nascosto all'accensione.
- Porta predefinita: 21 su Windows, 2121 su Linux e macOS (le porte sotto 1024 richiedono root).
  Nella stampante va indicata la stessa porta.
- Impostazioni in `%APPDATA%\ScanToFolder` (Windows), `~/.config/ScanToFolder` (Linux),
  `~/Library/Application Support/ScanToFolder` (macOS).

## Differenze rispetto alla versione Python
- Gruppo "Users" e regole firewall "Condivisione file e stampanti" indicati in modo indipendente
  dalla lingua di Windows (prima su Windows in italiano quei due passi fallivano in silenzio).
- I valori inseriti non possono più rompere gli script (apici, `$`, spazi): nomi utente e
  condivisione accettano solo lettere, numeri, `.`, `_`, `-`.

## Build
- Windows sul PC: `build.bat` → `dist\ScanToFolder.exe` (`build.bat installer` per l'installer NSIS).
- GitHub: Actions → build → Run workflow compila Windows, Linux e macOS. Con un tag (`v2.0.0`)
  crea anche una Release con i file da scaricare.
- macOS non è firmato: al primo avvio clic destro → Apri.

## Prove fatte in sviluppo
Su Linux (come root): configurazione SMB con password contenente `'`, `$` e backtick, scrittura
di un file via SMB, login errato rifiutato, SMB v1, stato, rimozione; FTP vsftpd con upload
in modalità passiva e rimozione; server FTP integrato con upload, login errato rifiutato e arresto.
Le parti Windows e macOS sono da provare sul posto.
