//! Funzioni comuni a tutti i sistemi: log verso la pagina, esecuzione di script
//! con privilegi di amministratore, rete, controlli sui dati inseriti.

use serde::Deserialize;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use tauri::{AppHandle, Emitter};

/// Scrive righe nel riquadro "log" della pagina.
#[derive(Clone)]
pub struct Log(pub AppHandle);

impl Log {
    pub fn line(&self, s: &str) {
        let _ = self.0.emit("log", s.to_string());
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct SmbParams {
    pub folder: String,
    pub share: String,
    pub user: String,
    pub password: String,
    #[serde(default)]
    pub smb1: bool,
    #[serde(default)]
    #[allow(dead_code)]
    pub policy: bool,
}

#[derive(Deserialize, Clone, Debug)]
pub struct FtpParams {
    pub folder: String,
    pub user: String,
    pub password: String,
    pub port: u16,
    pub pasv_start: u16,
    pub pasv_end: u16,
}

/// Comando senza finestra console su Windows.
pub fn cmd(program: &str) -> Command {
    #[allow(unused_mut)]
    let mut c = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    c
}

// ── controlli sui dati ──────────────────────────────────────────────────────

/// Nomi di utente e condivisione: lettere, numeri, punto, trattino, trattino basso.
pub fn check_name(what: &str, s: &str) -> Result<(), String> {
    let ok = !s.is_empty()
        && s.len() <= 20
        && !s.starts_with('-')
        && !s.starts_with('.')
        && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if ok {
        Ok(())
    } else {
        Err(format!("{what} non valido: usa da 1 a 20 caratteri tra lettere, numeri, punto, trattino e trattino basso"))
    }
}

pub fn check_folder(s: &str) -> Result<(), String> {
    let abs = std::path::Path::new(s).is_absolute();
    if s.trim().is_empty() || !abs || s.chars().any(|c| c.is_control()) {
        return Err("cartella non valida: indica un percorso completo (es. C:\\Scan oppure /srv/scan)".into());
    }
    Ok(())
}

pub fn check_password(s: &str) -> Result<(), String> {
    if s.is_empty() || s.chars().any(|c| c.is_control()) {
        return Err("password non valida: non puo' essere vuota".into());
    }
    Ok(())
}

pub fn check_smb(p: &SmbParams) -> Result<(), String> {
    check_folder(&p.folder)?;
    check_name("nome condivisione", &p.share)?;
    check_name("utente", &p.user)?;
    check_password(&p.password)
}

pub fn check_ftp(p: &FtpParams) -> Result<(), String> {
    check_folder(&p.folder)?;
    check_name("utente", &p.user)?;
    check_password(&p.password)?;
    if p.port == 0 || p.pasv_start == 0 || p.pasv_end < p.pasv_start {
        return Err("porte non valide: la porta e l'intervallo passivo devono essere numeri e 'da' <= 'a'".into());
    }
    if p.pasv_end - p.pasv_start > 1000 {
        return Err("intervallo passivo troppo ampio (massimo 1000 porte)".into());
    }
    Ok(())
}

// ── rete ────────────────────────────────────────────────────────────────────

pub fn local_ip() -> String {
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| {
            s.connect("8.8.8.8:80")?;
            s.local_addr()
        })
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|_| "127.0.0.1".into())
}

pub fn hostname() -> String {
    if let Ok(h) = std::env::var("COMPUTERNAME") {
        return h;
    }
    cmd("hostname")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "pc".into())
}

/// Cartella dei dati dell'utente (impostazioni del server FTP integrato).
pub fn data_dir() -> PathBuf {
    let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("."));
    let base = if cfg!(windows) {
        std::env::var("APPDATA").map(PathBuf::from).unwrap_or(home)
    } else if cfg!(target_os = "macos") {
        home.join("Library/Application Support")
    } else {
        std::env::var("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|_| home.join(".config"))
    };
    let d = base.join("ScanToFolder");
    let _ = std::fs::create_dir_all(&d);
    d
}

pub fn open_url(url: &str) {
    #[cfg(windows)]
    let _ = cmd("explorer").arg(url).spawn();
    #[cfg(target_os = "macos")]
    let _ = Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = Command::new("xdg-open").arg(url).spawn();
}

// ── script con privilegi di amministratore ──────────────────────────────────

/// File temporaneo leggibile solo dall'utente (contiene la password inserita).
fn temp_file(ext: &str, content: &[u8]) -> std::io::Result<PathBuf> {
    let n: u64 = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
    let p = std::env::temp_dir().join(format!("scantofolder_{}_{n}.{ext}", std::process::id()));
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&p)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600))?;
    }
    f.write_all(content)?;
    Ok(p)
}

/// Esegue lo script con i privilegi di amministratore e manda l'output al log.
/// true = terminato con codice 0.
///   Windows: PowerShell con richiesta UAC (una per operazione)
///   Linux:   pkexec (finestra di autorizzazione del sistema)
///   macOS:   richiesta della password di amministratore
pub fn run_privileged(script: &str, log: &Log) -> bool {
    #[cfg(windows)]
    {
        run_privileged_windows(script, log)
    }
    #[cfg(unix)]
    {
        run_privileged_unix(script, log)
    }
}

#[cfg(unix)]
fn is_root() -> bool {
    cmd("id").arg("-u").output().map(|o| String::from_utf8_lossy(&o.stdout).trim() == "0").unwrap_or(false)
}

#[cfg(unix)]
fn run_privileged_unix(script: &str, log: &Log) -> bool {
    // senza "exit 0" finale il codice d'uscita sarebbe quello dell'ultimo comando
    let full = format!("{script}\nexit 0\n");
    let path = match temp_file("sh", full.as_bytes()) {
        Ok(p) => p,
        Err(e) => {
            log.line(&format!("[ERRORE] file temporaneo: {e}"));
            return false;
        }
    };
    let ps = path.display().to_string();
    let mut c = if is_root() {
        let mut c = Command::new("/bin/sh");
        c.arg(&ps);
        c
    } else if cfg!(target_os = "macos") {
        // la stringa AppleScript contiene solo il percorso del file temporaneo
        let apple = format!("do shell script \"/bin/sh '{}' 2>&1\" with administrator privileges", ps.replace('\'', ""));
        let mut c = Command::new("osascript");
        c.arg("-e").arg(apple);
        c
    } else {
        let mut c = Command::new("pkexec");
        c.arg("/bin/sh").arg(&ps);
        c
    };
    log.line("  (richiesta autorizzazione di amministratore...)");
    let ok = match c.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() {
        Ok(mut child) => {
            let err = child.stderr.take();
            if let Some(out) = child.stdout.take() {
                for l in BufReader::new(out).lines().map_while(Result::ok) {
                    log.line(&l);
                }
            }
            let mut err_txt = String::new();
            if let Some(mut e) = err {
                use std::io::Read;
                let _ = e.read_to_string(&mut err_txt);
            }
            let status = child.wait();
            let code = status.as_ref().ok().and_then(|s| s.code()).unwrap_or(-1);
            if code == 126 || code == 127 || err_txt.contains("-128") || err_txt.contains("User canceled") {
                log.line("[ANNULLATO] autorizzazione di amministratore negata o annullata");
            } else if !err_txt.trim().is_empty() {
                for l in err_txt.lines() {
                    log.line(&format!("  {l}"));
                }
            }
            code == 0
        }
        Err(e) => {
            log.line(&format!("[ERRORE] avvio non riuscito: {e}"));
            false
        }
    };
    let _ = std::fs::remove_file(&path);
    ok
}

#[cfg(windows)]
fn is_admin() -> bool {
    #[link(name = "shell32")]
    extern "system" {
        fn IsUserAnAdmin() -> i32;
    }
    unsafe { IsUserAnAdmin() != 0 }
}

#[cfg(windows)]
pub fn admin() -> bool {
    is_admin()
}
#[cfg(unix)]
pub fn admin() -> bool {
    is_root()
}

/// Stringa PowerShell tra apici singoli (sicura: dentro gli apici singoli
/// PowerShell non interpreta niente, basta raddoppiare l'apice).
#[allow(dead_code)]
pub fn psq(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Stringa shell tra apici singoli (sicura per /bin/sh).
pub fn shq(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(windows)]
fn run_privileged_windows(script: &str, log: &Log) -> bool {
    // lo script scrive tutto il suo output in un file di log che poi si legge
    let out = match temp_file("log", b"") {
        Ok(p) => p,
        Err(e) => {
            log.line(&format!("[ERRORE] file temporaneo: {e}"));
            return false;
        }
    };
    // Say scrive una riga nel file di log; gli script usano solo Say/OK/W/F
    let wrapped = format!(
        "$ErrorActionPreference = 'Stop'\r\n$ProgressPreference = 'SilentlyContinue'\r\n$LOG = {}\r\n\
         function Say([string]$t) {{ Add-Content -LiteralPath $LOG -Value $t -Encoding UTF8 }}\r\n\
         function OK([string]$t) {{ Say \"  [OK] $t\" }}\r\n\
         function W([string]$t) {{ Say \"  [ATTENZIONE] $t\" }}\r\n\
         function F([string]$t) {{ Say \"  [ERRORE] $t\"; exit 1 }}\r\n\
         {script}\r\nexit 0\r\n",
        psq(&out.display().to_string())
    );
    // BOM UTF-8: Windows PowerShell 5 altrimenti legge lo script come ANSI
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(wrapped.as_bytes());
    let ps1 = match temp_file("ps1", &bytes) {
        Ok(p) => p,
        Err(e) => {
            log.line(&format!("[ERRORE] file temporaneo: {e}"));
            return false;
        }
    };
    let ps1s = psq(&ps1.display().to_string());
    let status = if is_admin() {
        cmd("powershell").args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", &ps1.display().to_string()]).status()
    } else {
        log.line("  (richiesta autorizzazione di amministratore...)");
        let launcher = format!(
            "try {{ $p = Start-Process -FilePath powershell -ArgumentList @('-NoProfile','-ExecutionPolicy','Bypass','-File',{ps1s}) -Verb RunAs -WindowStyle Hidden -Wait -PassThru; exit $p.ExitCode }} catch {{ exit 1223 }}"
        );
        cmd("powershell").args(["-NoProfile", "-Command", &launcher]).status()
    };
    let code = status.ok().and_then(|s| s.code()).unwrap_or(-1);
    let text = std::fs::read(&out).map(|b| String::from_utf8_lossy(&b).trim_start_matches('\u{feff}').to_string()).unwrap_or_default();
    for l in text.lines() {
        log.line(l.trim_end());
    }
    if code == 1223 {
        log.line("[ANNULLATO] autorizzazione di amministratore negata o annullata");
    }
    let _ = std::fs::remove_file(&out);
    let _ = std::fs::remove_file(&ps1);
    code == 0
}

/// Riquadro finale con i dati da inserire nella stampante.
pub fn summary(title: &str, rows: &[(&str, String)]) -> String {
    let mut s = String::from("\n══════════════════════════════════════════════════\n");
    s += &format!("  {title}\n");
    s += "══════════════════════════════════════════════════\n";
    for (k, v) in rows {
        s += &format!("  {k:<12}: {v}\n");
    }
    s += "══════════════════════════════════════════════════";
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nomi() {
        assert!(check_name("u", "scan").is_ok());
        assert!(check_name("u", "scan.ufficio_2").is_ok());
        assert!(check_name("u", "-x").is_err());
        assert!(check_name("u", "a b").is_err());
        assert!(check_name("u", "a'b").is_err());
        assert!(check_name("u", "").is_err());
    }
    #[test]
    fn apici() {
        assert_eq!(psq("a'b"), "'a''b'");
        assert_eq!(shq("a'b"), "'a'\\''b'");
    }
}
