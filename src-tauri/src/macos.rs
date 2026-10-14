//! macOS: condivisione SMB nativa (sharing + utente con hash SMB).
//! macOS non ha piu' un server FTP di sistema: si usa il server FTP integrato.
//! Gli script girano come root dopo la richiesta della password di amministratore.

use crate::common::{shq, FtpParams, SmbParams};

pub const FTP_SYSTEM: bool = false;
pub const FTP_SYSTEM_NAME: &str = "non disponibile su macOS";
pub const SMB1_SUPPORTED: bool = false;
pub const POLICY_SUPPORTED: bool = false;
pub const DEFAULT_FOLDER: &str = "/Users/Shared/Scan";
/// la porta 21 richiede root: il server integrato usa la 2121
pub const DEFAULT_BUILTIN_PORT: u16 = 2121;

const PRELUDE: &str = r##"exec 2>&1
OK() { echo "  [OK] $1"; }
W()  { echo "  [ATTENZIONE] $1"; }
F()  { echo "  [ERRORE] $1"; exit 1; }
Say() { echo "$1"; }
"##;

fn vars(pairs: &[(&str, &str)]) -> String {
    pairs.iter().map(|(k, v)| format!("{k}={}\n", shq(v))).collect()
}

pub fn smb_setup(p: &SmbParams) -> String {
    let mut s = String::from(PRELUDE);
    s += &vars(&[("FOLDER", &p.folder), ("SHARE", &p.share), ("U", &p.user), ("PASS", &p.password)]);
    s += r##"Say "[1/5] Cartella $FOLDER ..."
if [ -d "$FOLDER" ]; then OK "GIA ESISTENTE"; else mkdir -p "$FOLDER" || F "creazione cartella"; OK "CREATA"; fi
Say "[2/5] Utente '$U' ..."
if dscl . -read "/Users/$U" >/dev/null 2>&1; then
  dscl . -passwd "/Users/$U" "$PASS" || F "aggiornamento password"
  OK "AGGIORNATO"
else
  sysadminctl -addUser "$U" -fullName "Scan User" -password "$PASS" -home /var/empty >/dev/null 2>&1 || F "creazione utente"
  dscl . -create "/Users/$U" IsHidden 1
  dscl . -create "/Users/$U" UserShell /usr/bin/false
  OK "CREATO (nascosto dalla schermata di login)"
fi
# accesso SMB: serve l'hash SMB-NT, che si genera reimpostando la password
pwpolicy -u "$U" -sethashtypes SMB-NT on >/dev/null 2>&1
dscl . -passwd "/Users/$U" "$PASS" || F "password SMB"
OK "accesso SMB abilitato per $U"
Say "[3/5] Permessi su $FOLDER ..."
chmod +a "$U allow list,add_file,search,add_subdirectory,delete_child,readattr,writeattr,readextattr,writeextattr,readsecurity,read,write,append,execute,delete,file_inherit,directory_inherit" "$FOLDER" || F "permessi"
OK "lettura/scrittura per $U"
Say "[4/5] Condivisione '$SHARE' ..."
sharing -r "$SHARE" >/dev/null 2>&1
sharing -a "$FOLDER" -S "$SHARE" -s 001 -g 000 >/dev/null 2>&1 || F "creazione condivisione"
OK "condivisione SMB creata"
Say "[5/5] Servizio Condivisione file (SMB) ..."
launchctl enable system/com.apple.smbd >/dev/null 2>&1
launchctl load -w /System/Library/LaunchDaemons/com.apple.smbd.plist >/dev/null 2>&1
defaults write /Library/Preferences/SystemConfiguration/com.apple.smb.server EnabledServices -array disk
OK "servizio SMB attivo"
"##;
    s
}

pub fn smb_remove(p: &SmbParams) -> String {
    let mut s = String::from(PRELUDE);
    s += &vars(&[("SHARE", &p.share), ("U", &p.user)]);
    s += r##"Say "[1/3] Rimozione condivisione '$SHARE' ..."
sharing -r "$SHARE" >/dev/null 2>&1 && OK "rimossa" || W "condivisione non trovata"
Say "[2/3] Rimozione utente '$U' ..."
if dscl . -read "/Users/$U" >/dev/null 2>&1; then sysadminctl -deleteUser "$U" >/dev/null 2>&1 && OK "rimosso" || W "rimozione utente"; else OK "non presente"; fi
Say "[3/3] Cartella conservata (eliminare manualmente se necessario)."
Say ""
Say "Rimozione completata."
"##;
    s
}

pub fn smb_status() -> String {
    let mut s = String::from(PRELUDE);
    s += r##"Say "--- SHARE ATTIVE ---"
sharing -l | grep -E "^name:|^path:" || Say "nessuna"
Say ""
Say "--- UTENTI LOCALI ---"
dscl . list /Users | grep -v "^_" | grep -vE "^(daemon|nobody|root)$"
Say ""
Say "--- SMB ---"
launchctl print system/com.apple.smbd >/dev/null 2>&1 && Say "servizio smbd attivo" || Say "servizio smbd non attivo"
"##;
    s
}

pub fn ftp_setup(_p: &FtpParams) -> String {
    String::new()
}
pub fn ftp_remove(_p: &FtpParams) -> String {
    String::new()
}

/// Il firewall applicativo di macOS chiede da solo se consentire le connessioni.
pub fn builtin_firewall(_port: u16, _a: u16, _b: u16, _add: bool) -> Option<String> {
    None
}
