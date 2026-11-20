//! Server FTP integrato (libunftp): funziona uguale su Windows, Linux e macOS,
//! senza IIS o vsftpd. Riceve le scansioni finche' il programma e' aperto
//! (resta nell'area di notifica e puo' partire con il computer).

use crate::common::{data_dir, Log};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use unftp_core::auth::{AuthenticationError, Authenticator, Credentials, Principal};

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Saved {
    pub folder: String,
    pub user: String,
    pub password: String,
    pub port: u16,
    pub pasv_start: u16,
    pub pasv_end: u16,
    /// riavvia il server all'apertura del programma
    pub enabled: bool,
}

fn file() -> std::path::PathBuf {
    data_dir().join("ftp_integrato.json")
}

pub fn load() -> Option<Saved> {
    std::fs::read_to_string(file()).ok().and_then(|t| serde_json::from_str(&t).ok())
}

pub fn save(s: &Saved) {
    let Ok(t) = serde_json::to_string_pretty(s) else { return };
    let p = file();
    if std::fs::write(&p, t).is_ok() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
        }
    }
}

#[derive(Debug)]
struct OneUser {
    user: String,
    pass: String,
    log: LogSink,
}

#[async_trait]
impl Authenticator for OneUser {
    async fn authenticate(&self, username: &str, creds: &Credentials) -> Result<Principal, AuthenticationError> {
        if !username.eq_ignore_ascii_case(&self.user) {
            self.log.0.line(&format!("[{}] login rifiutato: utente '{username}' sconosciuto", now_hms()));
            return Err(AuthenticationError::BadUser);
        }
        match &creds.password {
            Some(p) if *p == self.pass => Ok(Principal { username: self.user.clone() }),
            _ => {
                self.log.0.line(&format!("[{}] login rifiutato: password errata per '{username}'", now_hms()));
                Err(AuthenticationError::BadPassword)
            }
        }
    }
}

/// Scrive nel log ogni file ricevuto.
#[derive(Debug)]
struct Notifier(LogSink);

#[derive(Clone)]
struct LogSink(Log);
impl std::fmt::Debug for LogSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LogSink")
    }
}

#[async_trait]
impl libunftp::notification::DataListener for Notifier {
    async fn receive_data_event(&self, e: libunftp::notification::DataEvent, _m: libunftp::notification::EventMeta) {
        if let libunftp::notification::DataEvent::Put { path, bytes } = e {
            let now = now_hms();
            self.0 .0.line(&format!("[{now}] ricevuto {path} ({} KB)", bytes.div_ceil(1024)));
        }
    }
}

/// Porta nel log del programma i messaggi di libunftp (connessioni in arrivo,
/// porte passive, errori di connessione in modalita' attiva, timeout...), cosi'
/// si capisce dove si ferma una stampante che resta "in attesa".
struct SlogToLog(std::panic::AssertUnwindSafe<Log>);

impl slog::Drain for SlogToLog {
    type Ok = ();
    type Err = slog::Never;
    fn log(&self, r: &slog::Record, _v: &slog::OwnedKVList) -> Result<(), slog::Never> {
        let msg = r.msg().to_string();
        // righe ripetute ad ogni sessione che non aggiungono nulla
        if msg.starts_with("Starting control loop") || msg.starts_with("Spawning") || msg.starts_with("PASS:") {
            return Ok(());
        }
        let lvl = match r.level() {
            slog::Level::Critical | slog::Level::Error => "ERRORE ",
            slog::Level::Warning => "ATTENZIONE ",
            _ => "",
        };
        self.0.line(&format!("[{}] {lvl}ftp: {msg}", now_hms()));
        Ok(())
    }
}

/// Login e logout di ogni sessione.
#[derive(Debug)]
struct Presence(LogSink);

#[async_trait]
impl libunftp::notification::PresenceListener for Presence {
    async fn receive_presence_event(&self, e: libunftp::notification::PresenceEvent, m: libunftp::notification::EventMeta) {
        let what = match e {
            libunftp::notification::PresenceEvent::LoggedIn => "login riuscito",
            libunftp::notification::PresenceEvent::LoggedOut => "disconnesso",
        };
        self.0 .0.line(&format!("[{}] ftp: {what} ({})", now_hms(), m.username));
    }
}

fn now_hms() -> String {
    chrono::Local::now().format("%d/%m %H:%M:%S").to_string()
}

#[derive(Default)]
pub struct Server {
    stop: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    running: Mutex<Option<Saved>>,
}

impl Server {
    pub fn running(&self) -> Option<Saved> {
        self.running.lock().unwrap().clone()
    }

    /// Avvia il server; errore se la porta e' occupata o la cartella non e' valida.
    pub async fn start(self: &Arc<Self>, cfg: Saved, log: Log) -> Result<(), String> {
        self.stop().await;
        let folder = std::path::PathBuf::from(&cfg.folder);
        std::fs::create_dir_all(&folder).map_err(|e| format!("cartella {}: {e}", cfg.folder))?;
        // la porta si prova prima, cosi' l'errore arriva subito alla pagina
        let addr = format!("0.0.0.0:{}", cfg.port);
        drop(std::net::TcpListener::bind(&addr).map_err(|e| {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                format!("porta {} non consentita: su Linux e macOS le porte sotto 1024 richiedono root, usa per esempio 2121", cfg.port)
            } else {
                format!("porta {} gia' in uso o non disponibile ({e}): chiudi l'altro server FTP o scegli un'altra porta", cfg.port)
            }
        })?);
        let f2 = folder.clone();
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let server = libunftp::ServerBuilder::with_authenticator(
            Box::new(move || unftp_sbe_fs::Filesystem::new(f2.clone()).expect("cartella FTP")),
            Arc::new(OneUser { user: cfg.user.clone(), pass: cfg.password.clone(), log: LogSink(log.clone()) }),
        )
        .greeting("Scan to Folder - N.A.I")
        .passive_ports(cfg.pasv_start..=cfg.pasv_end)
        // libunftp di default e' solo passivo e risponde 502 al PORT: molte
        // stampanti (Ricoh, Kyocera, Canon...) usano l'attivo e restano in attesa.
        .active_passive_mode(libunftp::options::ActivePassiveMode::ActiveAndPassive)
        .notify_data(Notifier(LogSink(log.clone())))
        .notify_presence(Presence(LogSink(log.clone())))
        .logger(slog::Logger::root(SlogToLog(std::panic::AssertUnwindSafe(log.clone())), slog::o!()))
        .shutdown_indicator(async move {
            let _ = rx.await;
            libunftp::options::Shutdown::new()
        })
        .build()
        .map_err(|e| format!("configurazione server FTP: {e}"))?;
        *self.stop.lock().unwrap() = Some(tx);
        *self.running.lock().unwrap() = Some(cfg.clone());
        let me = self.clone();
        let log2 = log.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = server.listen(addr).await {
                log2.line(&format!("[ERRORE] server FTP integrato: {e}"));
            }
            *me.running.lock().unwrap() = None;
            log2.line("server FTP integrato fermato");
        });
        Ok(())
    }

    pub async fn stop(&self) {
        let tx = self.stop.lock().unwrap().take();
        if let Some(tx) = tx {
            let _ = tx.send(());
            // attende la chiusura (porta libera) per un massimo di 3 secondi
            for _ in 0..30 {
                if self.running.lock().unwrap().is_none() {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            *self.running.lock().unwrap() = None;
        }
    }
}
