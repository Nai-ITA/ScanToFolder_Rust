//! Scan to Folder 2.0 (Rust): prepara una cartella per le scansioni delle
//! stampanti multifunzione. Tre modi:
//!   1. SMB / cartella condivisa (Windows, Linux con Samba, macOS)
//!   2. FTP di sistema (IIS su Windows, vsftpd su Linux)
//!   3. FTP integrato: server FTP dentro il programma, uguale su tutti i sistemi
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod builtin;
mod common;

#[cfg(target_os = "linux")]
#[path = "linux.rs"]
mod sys;
#[cfg(target_os = "macos")]
#[path = "macos.rs"]
mod sys;
#[cfg(windows)]
#[path = "win.rs"]
mod sys;

use common::{check_ftp, check_smb, hostname, local_ip, summary, FtpParams, Log, SmbParams};
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, State, WindowEvent};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

struct App {
    ftp: Arc<builtin::Server>,
}

/// Esegue un'operazione lunga fuori dal thread dell'interfaccia.
async fn blocking<F>(app: AppHandle, f: F) -> Result<bool, String>
where
    F: FnOnce(&Log) -> bool + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || {
        let log = Log(app);
        f(&log)
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn info(state: State<'_, App>) -> Value {
    let saved = builtin::load();
    json!({
        "version": VERSION,
        "os": std::env::consts::OS,
        "admin": common::admin(),
        "ip": local_ip(),
        "host": hostname(),
        "ftp_system": sys::FTP_SYSTEM,
        "ftp_system_name": sys::FTP_SYSTEM_NAME,
        "smb1": sys::SMB1_SUPPORTED,
        "policy": sys::POLICY_SUPPORTED,
        "default_folder": sys::DEFAULT_FOLDER,
        "default_builtin_port": sys::DEFAULT_BUILTIN_PORT,
        "builtin": saved.map(|s| json!({"folder": s.folder, "user": s.user, "port": s.port,
            "pasv_start": s.pasv_start, "pasv_end": s.pasv_end, "enabled": s.enabled, "has_password": !s.password.is_empty()})),
        "builtin_running": state.ftp.running().is_some(),
        "builtin_firewall": cfg!(windows) || cfg!(target_os = "linux"),
        "autostart": autostart::get(),
    })
}

fn smb_rows(p: &SmbParams) -> Vec<(&'static str, String)> {
    let (ip, host) = (local_ip(), hostname());
    vec![
        ("Protocollo", "SMB / CIFS".into()),
        ("Host / IP", format!("{ip}  ({host})")),
        ("Share", p.share.clone()),
        ("Percorso", format!("\\\\{ip}\\{}   oppure   \\\\{host}\\{}", p.share, p.share)),
        ("Utente", p.user.clone()),
        ("Password", p.password.clone()),
    ]
}

#[tauri::command]
async fn smb_setup(app: AppHandle, p: SmbParams) -> Result<bool, String> {
    check_smb(&p)?;
    blocking(app, move |log| {
        log.line("────────────────────────────────────────────────────");
        let ok = common::run_privileged(&sys::smb_setup(&p), log);
        if ok {
            log.line(&summary("SETUP COMPLETATO - Dati per la stampante", &smb_rows(&p)));
        } else {
            log.line("[ERRORE] configurazione SMB non completata: vedi i messaggi sopra");
        }
        ok
    })
    .await
}

#[tauri::command]
async fn smb_remove(app: AppHandle, p: SmbParams) -> Result<bool, String> {
    common::check_name("nome condivisione", &p.share)?;
    common::check_name("utente", &p.user)?;
    blocking(app, move |log| {
        log.line("────────────────────────────────────────────────────");
        common::run_privileged(&sys::smb_remove(&p), log)
    })
    .await
}

#[tauri::command]
async fn smb_status(app: AppHandle) -> Result<bool, String> {
    blocking(app, move |log| {
        log.line("────────────────────────────────────────────────────");
        common::run_privileged(&sys::smb_status(), log)
    })
    .await
}

fn ftp_rows(p: &FtpParams, kind: &str) -> Vec<(&'static str, String)> {
    let (ip, host) = (local_ip(), hostname());
    vec![
        ("Protocollo", format!("FTP (modalita' passiva) - {kind}")),
        ("Host / IP", format!("{ip}  ({host})")),
        ("Porta", p.port.to_string()),
        ("Cartella", p.folder.clone()),
        ("Utente", p.user.clone()),
        ("Password", p.password.clone()),
        ("Passivo", format!("{} - {}", p.pasv_start, p.pasv_end)),
    ]
}

#[tauri::command]
async fn ftp_setup(app: AppHandle, p: FtpParams) -> Result<bool, String> {
    if !sys::FTP_SYSTEM {
        return Err("su questo sistema non c'e' un server FTP di sistema: usa la scheda FTP integrato".into());
    }
    check_ftp(&p)?;
    blocking(app, move |log| {
        log.line("────────────────────────────────────────────────────");
        let ok = common::run_privileged(&sys::ftp_setup(&p), log);
        if ok {
            log.line(&summary("FTP SETUP COMPLETATO - Dati per la stampante", &ftp_rows(&p, sys::FTP_SYSTEM_NAME)));
        } else {
            log.line("[ERRORE] configurazione FTP non completata: vedi i messaggi sopra");
        }
        ok
    })
    .await
}

#[tauri::command]
async fn ftp_remove(app: AppHandle, p: FtpParams) -> Result<bool, String> {
    if !sys::FTP_SYSTEM {
        return Err("non disponibile su questo sistema".into());
    }
    common::check_name("utente", &p.user)?;
    blocking(app, move |log| {
        log.line("────────────────────────────────────────────────────");
        common::run_privileged(&sys::ftp_remove(&p), log)
    })
    .await
}

/// Avvia il server FTP integrato. Password vuota = usa quella salvata.
#[tauri::command]
async fn builtin_start(app: AppHandle, state: State<'_, App>, mut p: FtpParams) -> Result<bool, String> {
    if p.password.is_empty() {
        p.password = builtin::load().map(|s| s.password).unwrap_or_default();
    }
    check_ftp(&p)?;
    let log = Log(app);
    let cfg = builtin::Saved {
        folder: p.folder.clone(),
        user: p.user.clone(),
        password: p.password.clone(),
        port: p.port,
        pasv_start: p.pasv_start,
        pasv_end: p.pasv_end,
        enabled: true,
    };
    log.line("────────────────────────────────────────────────────");
    log.line(&format!("Avvio del server FTP integrato sulla porta {} ...", p.port));
    state.ftp.start(cfg.clone(), log.clone()).await?;
    builtin::save(&cfg);
    log.line("  [OK] server in ascolto: riceve le scansioni finche' il programma e' aperto (anche nascosto nell'area di notifica)");
    log.line(&summary("FTP INTEGRATO ATTIVO - Dati per la stampante", &ftp_rows(&p, "server integrato")));
    Ok(true)
}

#[tauri::command]
async fn builtin_stop(app: AppHandle, state: State<'_, App>) -> Result<bool, String> {
    state.ftp.stop().await;
    if let Some(mut s) = builtin::load() {
        s.enabled = false;
        builtin::save(&s);
    }
    Log(app).line("Server FTP integrato fermato: non si riavviera' all'apertura del programma.");
    Ok(true)
}

/// Apre (o chiude) le porte del server integrato nel firewall del sistema.
#[tauri::command]
async fn builtin_firewall(app: AppHandle, p: FtpParams, add: bool) -> Result<bool, String> {
    let Some(script) = sys::builtin_firewall(p.port, p.pasv_start, p.pasv_end, add) else {
        return Err("su questo sistema il firewall chiede da solo il permesso al primo collegamento".into());
    };
    blocking(app, move |log| {
        log.line("────────────────────────────────────────────────────");
        common::run_privileged(&script, log)
    })
    .await
}

#[tauri::command]
fn autostart_set(on: bool) -> Result<bool, String> {
    autostart::set(on)?;
    Ok(autostart::get())
}

#[tauri::command]
fn open_url(url: String) {
    if url.starts_with("https://") {
        common::open_url(&url);
    }
}

#[tauri::command]
async fn quit(app: AppHandle, state: State<'_, App>) -> Result<(), String> {
    state.ftp.stop().await;
    app.exit(0);
    Ok(())
}

fn show(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn main() {
    let minimized = std::env::args().any(|a| a == "--minimized");
    let ftp = Arc::new(builtin::Server::default());
    let ftp_for_setup = ftp.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show(app)))
        .manage(App { ftp: ftp.clone() })
        .invoke_handler(tauri::generate_handler![
            info, smb_setup, smb_remove, smb_status, ftp_setup, ftp_remove,
            builtin_start, builtin_stop, builtin_firewall, autostart_set, open_url, quit
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            let open = MenuItem::with_id(app, "open", "apri Scan to Folder", true, None::<&str>)?;
            let sep = PredefinedMenuItem::separator(app)?;
            let exit = MenuItem::with_id(app, "quit", "esci (ferma il server FTP integrato)", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &sep, &exit])?;
            let ftp_q = ftp_for_setup.clone();
            let mut tray = TrayIconBuilder::with_id("stf")
                .tooltip(format!("Scan to Folder {VERSION}"))
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(move |app, ev| match ev.id().as_ref() {
                    "quit" => {
                        let ftp = ftp_q.clone();
                        let app = app.clone();
                        tauri::async_runtime::spawn(async move {
                            ftp.stop().await;
                            app.exit(0);
                        });
                    }
                    _ => show(app),
                })
                .on_tray_icon_event(|tray, ev| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev {
                        show(tray.app_handle());
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;

            // la X: se il server integrato e' attivo il programma resta nell'area di notifica
            if let Some(w) = app.get_webview_window("main") {
                let w2 = w.clone();
                let ftp_w = ftp_for_setup.clone();
                let h2 = handle.clone();
                w.on_window_event(move |ev| {
                    if let WindowEvent::CloseRequested { api, .. } = ev {
                        if ftp_w.running().is_some() {
                            api.prevent_close();
                            let _ = w2.hide();
                        } else {
                            h2.exit(0);
                        }
                    }
                });
                if !minimized {
                    let _ = w.show();
                }
            }

            // riavvia il server integrato se era attivo
            if let Some(saved) = builtin::load().filter(|s| s.enabled) {
                let ftp = ftp_for_setup.clone();
                let log = Log(handle.clone());
                tauri::async_runtime::spawn(async move {
                    match ftp.start(saved.clone(), log.clone()).await {
                        Ok(()) => log.line(&format!("Server FTP integrato attivo sulla porta {} (cartella {})", saved.port, saved.folder)),
                        Err(e) => log.line(&format!("[ERRORE] riavvio del server FTP integrato: {e}")),
                    }
                });
            } else if minimized {
                // avviato all'accensione ma senza server da far girare: si mostra la finestra
                show(&handle);
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("avvio dell'interfaccia non riuscito");
}
