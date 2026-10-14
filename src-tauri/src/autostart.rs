//! Avvio automatico con il computer (per il server FTP integrato), sempre
//! con --minimized: il programma parte nell'area di notifica.

#[cfg(windows)]
const RUN: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";

fn exe() -> Option<String> {
    if let Ok(a) = std::env::var("APPIMAGE") {
        if !a.is_empty() {
            return Some(a);
        }
    }
    std::env::current_exe().ok().map(|p| p.display().to_string())
}

#[cfg(not(windows))]
fn file() -> std::path::PathBuf {
    let home = std::env::var("HOME").map(std::path::PathBuf::from).unwrap_or_default();
    if cfg!(target_os = "macos") {
        home.join("Library/LaunchAgents/it.naict.scantofolder.plist")
    } else {
        std::env::var("XDG_CONFIG_HOME").map(std::path::PathBuf::from).unwrap_or_else(|_| home.join(".config")).join("autostart/scan-to-folder.desktop")
    }
}

pub fn get() -> bool {
    #[cfg(windows)]
    {
        crate::common::cmd("reg").args(["query", RUN, "/v", "ScanToFolder"]).output().map(|o| o.status.success()).unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        file().exists()
    }
}

pub fn set(on: bool) -> Result<(), String> {
    let exe = exe().ok_or("eseguibile non trovato")?;
    #[cfg(windows)]
    {
        let r = if on {
            crate::common::cmd("reg").args(["add", RUN, "/v", "ScanToFolder", "/t", "REG_SZ", "/d", &format!("\"{exe}\" --minimized"), "/f"]).output()
        } else {
            crate::common::cmd("reg").args(["delete", RUN, "/v", "ScanToFolder", "/f"]).output()
        };
        match r {
            Ok(o) if o.status.success() || !on => Ok(()),
            Ok(o) => Err(String::from_utf8_lossy(&o.stderr).trim().to_string()),
            Err(e) => Err(e.to_string()),
        }
    }
    #[cfg(not(windows))]
    {
        let f = file();
        if !on {
            let _ = std::fs::remove_file(&f);
            return Ok(());
        }
        if let Some(d) = f.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let xml = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
        let body = if cfg!(target_os = "macos") {
            format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n  <key>Label</key><string>it.naict.scantofolder</string>\n  <key>ProgramArguments</key>\n  <array><string>{}</string><string>--minimized</string></array>\n  <key>RunAtLoad</key><true/>\n</dict>\n</plist>\n", xml(&exe))
        } else {
            format!("[Desktop Entry]\nType=Application\nName=Scan to Folder\nExec=\"{exe}\" --minimized\nX-GNOME-Autostart-enabled=true\nTerminal=false\n")
        };
        std::fs::write(&f, body).map_err(|e| e.to_string())
    }
}
