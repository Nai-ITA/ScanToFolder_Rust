//! Linux (Debian/Ubuntu e derivate; Fedora con dnf): Samba per SMB, vsftpd per
//! l'FTP di sistema. Gli script girano come root tramite pkexec.

use crate::common::{shq, FtpParams, SmbParams};

pub const FTP_SYSTEM: bool = true;
pub const FTP_SYSTEM_NAME: &str = "vsftpd (servizio FTP di Linux)";
pub const SMB1_SUPPORTED: bool = true;
pub const POLICY_SUPPORTED: bool = false;
pub const DEFAULT_FOLDER: &str = "/srv/scan";
/// la porta 21 richiede root: il server integrato usa la 2121
pub const DEFAULT_BUILTIN_PORT: u16 = 2121;

pub const PRELUDE: &str = r##"exec 2>&1
OK() { echo "  [OK] $1"; }
W()  { echo "  [ATTENZIONE] $1"; }
F()  { echo "  [ERRORE] $1"; exit 1; }
Say() { echo "$1"; }
pkg_install() {
  if command -v apt-get >/dev/null 2>&1; then DEBIAN_FRONTEND=noninteractive apt-get install -y "$@" >/dev/null 2>&1
  elif command -v dnf >/dev/null 2>&1; then dnf install -y "$@" >/dev/null 2>&1
  elif command -v zypper >/dev/null 2>&1; then zypper --non-interactive install "$@" >/dev/null 2>&1
  else return 1; fi
}
NOLOGIN=$(command -v nologin || echo /usr/sbin/nologin)
ufw_on() { command -v ufw >/dev/null 2>&1 && ufw status 2>/dev/null | grep -q "Status: active"; }
fw_add() { if ufw_on; then ufw allow "$1" >/dev/null && OK "ufw: aperta $1"; elif command -v firewall-cmd >/dev/null 2>&1 && firewall-cmd --state >/dev/null 2>&1; then firewall-cmd --permanent --add-port="$1" >/dev/null && firewall-cmd --reload >/dev/null && OK "firewalld: aperta $1"; else OK "nessun firewall attivo"; fi; }
fw_del() { if ufw_on; then ufw delete allow "$1" >/dev/null 2>&1; fi; if command -v firewall-cmd >/dev/null 2>&1 && firewall-cmd --state >/dev/null 2>&1; then firewall-cmd --permanent --remove-port="$1" >/dev/null 2>&1; firewall-cmd --reload >/dev/null 2>&1; fi; OK "regola $1 rimossa"; }
"##;

fn vars(pairs: &[(&str, &str)]) -> String {
    pairs.iter().map(|(k, v)| format!("{k}={}\n", shq(v))).collect()
}

pub fn smb_setup(p: &SmbParams) -> String {
    let mut s = String::from(PRELUDE);
    s += &vars(&[("FOLDER", &p.folder), ("SHARE", &p.share), ("U", &p.user), ("PASS", &p.password), ("SMB1", if p.smb1 { "1" } else { "0" })]);
    s += r##"CONF=/etc/samba/smb.conf
Say "[1/7] Samba ..."
if command -v smbd >/dev/null 2>&1; then OK "presente"; else
  Say "  installazione di samba (qualche minuto)..."
  pkg_install samba || F "samba non installato e installazione automatica non riuscita"
  OK "samba installato"
fi
[ -f "$CONF" ] || F "manca $CONF"
Say "[2/7] Cartella $FOLDER ..."
if [ -d "$FOLDER" ]; then OK "GIA ESISTENTE"; else mkdir -p "$FOLDER" || F "creazione cartella"; OK "CREATA"; fi
Say "[3/7] Utente '$U' ..."
if id "$U" >/dev/null 2>&1; then OK "GIA ESISTENTE"; else useradd -M -s "$NOLOGIN" "$U" || F "creazione utente"; OK "CREATO"; fi
printf '%s\n%s\n' "$PASS" "$PASS" | smbpasswd -a -s "$U" >/dev/null || F "password samba"
smbpasswd -e "$U" >/dev/null 2>&1
OK "password samba impostata"
Say "[4/7] Permessi su $FOLDER ..."
chown "$U" "$FOLDER" && chmod 2775 "$FOLDER" || F "permessi"
OK "proprietario $U, lettura/scrittura"
Say "[5/7] Condivisione '$SHARE' ..."
[ -f "$CONF.scantofolder.bak" ] || cp "$CONF" "$CONF.scantofolder.bak"
sed -i "/^# BEGIN scantofolder $SHARE\$/,/^# END scantofolder $SHARE\$/d" "$CONF"
{
  echo "# BEGIN scantofolder $SHARE"
  echo "[$SHARE]"
  echo "   path = $FOLDER"
  echo "   valid users = $U"
  echo "   writable = yes"
  echo "   browseable = yes"
  echo "   create mask = 0664"
  echo "   directory mask = 2775"
  echo "   force user = $U"
  echo "# END scantofolder $SHARE"
} >> "$CONF"
sed -i '/^# BEGIN scantofolder-smb1$/,/^# END scantofolder-smb1$/d' "$CONF"
if [ "$SMB1" = 1 ]; then
  sed -i '/^\[global\]/a # BEGIN scantofolder-smb1\n   server min protocol = NT1\n   ntlm auth = ntlmv1-permitted\n# END scantofolder-smb1' "$CONF"
  OK "SMB v1 abilitato (per stampanti legacy)"
fi
if command -v testparm >/dev/null 2>&1 && ! testparm -s "$CONF" >/dev/null 2>&1; then
  cp "$CONF.scantofolder.bak" "$CONF"
  F "configurazione samba non valida: ripristinato smb.conf originale"
fi
OK "condivisione configurata"
Say "[6/7] Firewall ..."
if ufw_on; then ufw allow samba >/dev/null && OK "ufw: samba consentito"; else fw_add 445/tcp; fi
Say "[7/7] Avvio servizio Samba ..."
RESTARTED=0
for svc in smbd smb; do
  if systemctl list-unit-files 2>/dev/null | grep -q "^$svc.service"; then systemctl enable "$svc" >/dev/null 2>&1; systemctl restart "$svc" && OK "$svc riavviato" && RESTARTED=1 && break; fi
done
if [ "$RESTARTED" = 0 ]; then
  if service smbd restart >/dev/null 2>&1; then OK "smbd riavviato"
  elif pgrep -x smbd >/dev/null 2>&1; then pkill -HUP -x smbd; OK "smbd: configurazione ricaricata"
  else smbd -D && OK "smbd avviato" || W "avvia Samba a mano (servizio smbd)"; fi
fi
"##;
    s
}

pub fn smb_remove(p: &SmbParams) -> String {
    let mut s = String::from(PRELUDE);
    s += &vars(&[("SHARE", &p.share), ("U", &p.user), ("SMB1", if p.smb1 { "1" } else { "0" })]);
    s += r##"CONF=/etc/samba/smb.conf
Say "[1/5] Rimozione condivisione '$SHARE' ..."
if [ -f "$CONF" ]; then sed -i "/^# BEGIN scantofolder $SHARE\$/,/^# END scantofolder $SHARE\$/d" "$CONF"; OK "rimossa"; else W "smb.conf non trovato"; fi
Say "[2/5] Rimozione utente '$U' ..."
smbpasswd -x "$U" >/dev/null 2>&1
if id "$U" >/dev/null 2>&1; then userdel "$U" && OK "rimosso" || W "rimozione utente"; else OK "non presente"; fi
Say "[3/5] Cartella conservata (eliminare manualmente se necessario)."
Say "[4/5] SMB v1 ..."
if [ "$SMB1" = 1 ]; then sed -i '/^# BEGIN scantofolder-smb1$/,/^# END scantofolder-smb1$/d' "$CONF"; OK "SMB v1 disabilitato"; else OK "saltato"; fi
Say "[5/5] Riavvio Samba ..."
systemctl restart smbd 2>/dev/null || systemctl restart smb 2>/dev/null || service smbd restart >/dev/null 2>&1 || pkill -HUP -x smbd 2>/dev/null; OK "fatto"
Say ""
Say "Rimozione completata."
"##;
    s
}

pub fn smb_status() -> String {
    let mut s = String::from(PRELUDE);
    s += r##"Say "--- SHARE ATTIVE ---"
if command -v testparm >/dev/null 2>&1; then testparm -s 2>/dev/null | grep -E '^\[|path =' ; else Say "samba non installato"; fi
Say ""
Say "--- UTENTI SAMBA ---"
pdbedit -L 2>/dev/null || Say "nessuno"
Say ""
Say "--- SMB VERSIONI ---"
testparm -s 2>/dev/null | grep -i "min protocol" || Say "predefinite (SMB2/SMB3)"
"##;
    s
}

pub fn ftp_setup(p: &FtpParams) -> String {
    let mut s = String::from(PRELUDE);
    s += &vars(&[("FOLDER", &p.folder), ("U", &p.user), ("PASS", &p.password)]);
    s += &format!("PORT={}\nPA={}\nPB={}\n", p.port, p.pasv_start, p.pasv_end);
    s += r##"Say "[1/6] vsftpd ..."
if command -v vsftpd >/dev/null 2>&1 || [ -x /usr/sbin/vsftpd ]; then OK "presente"; else
  Say "  installazione di vsftpd..."
  pkg_install vsftpd || F "vsftpd non installato e installazione automatica non riuscita"
  OK "vsftpd installato"
fi
if [ -d /etc/vsftpd ]; then CONF=/etc/vsftpd/vsftpd.conf; else CONF=/etc/vsftpd.conf; fi
Say "[2/6] Cartella $FOLDER ..."
if [ -d "$FOLDER" ]; then OK "GIA ESISTENTE"; else mkdir -p "$FOLDER" || F "creazione cartella"; OK "CREATA"; fi
Say "[3/6] Utente FTP '$U' ..."
if id "$U" >/dev/null 2>&1; then usermod -d "$FOLDER" "$U" 2>/dev/null; OK "GIA ESISTENTE"; else useradd -M -d "$FOLDER" -s "$NOLOGIN" "$U" || F "creazione utente"; OK "CREATO"; fi
printf '%s:%s\n' "$U" "$PASS" | chpasswd || F "password utente"
grep -qx "$NOLOGIN" /etc/shells || echo "$NOLOGIN" >> /etc/shells
chown "$U" "$FOLDER" && chmod 2775 "$FOLDER" || F "permessi"
OK "password e permessi impostati"
Say "[4/6] Configurazione vsftpd (porta $PORT, passivo $PA-$PB) ..."
[ -f "$CONF" ] && [ ! -f "$CONF.scantofolder.bak" ] && cp "$CONF" "$CONF.scantofolder.bak"
cat > "$CONF" <<EOF
# generato da Scan to Folder (originale in $CONF.scantofolder.bak)
listen=YES
listen_ipv6=NO
listen_port=$PORT
anonymous_enable=NO
local_enable=YES
write_enable=YES
local_umask=002
chroot_local_user=YES
allow_writeable_chroot=YES
pasv_enable=YES
pasv_min_port=$PA
pasv_max_port=$PB
userlist_enable=YES
userlist_deny=NO
userlist_file=/etc/vsftpd.scantofolder.userlist
pam_service_name=vsftpd
xferlog_enable=YES
EOF
echo "$U" > /etc/vsftpd.scantofolder.userlist
OK "configurazione scritta"
Say "[5/6] Avvio servizio vsftpd ..."
systemctl enable vsftpd >/dev/null 2>&1
if systemctl restart vsftpd 2>/dev/null || service vsftpd restart >/dev/null 2>&1; then OK "vsftpd avviato"
else pkill -x vsftpd 2>/dev/null; sleep 1; (vsftpd "$CONF" >/dev/null 2>&1 &) ; sleep 1; pgrep -x vsftpd >/dev/null && OK "vsftpd avviato" || F "vsftpd non si avvia: controlla con 'journalctl -u vsftpd'"; fi
Say "[6/6] Firewall (porta $PORT + passivo $PA-$PB) ..."
fw_add "$PORT/tcp"
fw_add "$PA:$PB/tcp"
"##;
    s
}

pub fn ftp_remove(p: &FtpParams) -> String {
    let mut s = String::from(PRELUDE);
    s += &vars(&[("U", &p.user)]);
    s += &format!("PORT={}\nPA={}\nPB={}\n", p.port, p.pasv_start, p.pasv_end);
    s += r##"if [ -d /etc/vsftpd ]; then CONF=/etc/vsftpd/vsftpd.conf; else CONF=/etc/vsftpd.conf; fi
Say "[1/4] Stop di vsftpd e ripristino configurazione ..."
systemctl stop vsftpd 2>/dev/null || service vsftpd stop >/dev/null 2>&1 || pkill -x vsftpd 2>/dev/null; systemctl disable vsftpd >/dev/null 2>&1
if [ -f "$CONF.scantofolder.bak" ]; then mv -f "$CONF.scantofolder.bak" "$CONF"; OK "configurazione originale ripristinata"; else OK "fermato"; fi
rm -f /etc/vsftpd.scantofolder.userlist
Say "[2/4] Rimozione utente '$U' ..."
if id "$U" >/dev/null 2>&1; then userdel "$U" && OK "rimosso" || W "rimozione utente"; else OK "non presente"; fi
Say "[3/4] Firewall ..."
fw_del "$PORT/tcp"
fw_del "$PA:$PB/tcp"
Say "[4/4] Cartella conservata."
Say "Rimozione FTP completata."
"##;
    s
}

/// Server integrato: apre le porte se c'e' un firewall attivo.
pub fn builtin_firewall(port: u16, a: u16, b: u16, add: bool) -> Option<String> {
    let mut s = String::from(PRELUDE);
    s += &format!("PORT={port}\nPA={a}\nPB={b}\n");
    if !add {
        s += "fw_del \"$PORT/tcp\"\nfw_del \"$PA:$PB/tcp\"\n";
        return Some(s);
    }
    s += "Say \"[firewall] porte del server FTP integrato ...\"\nfw_add \"$PORT/tcp\"\nfw_add \"$PA:$PB/tcp\"\n";
    Some(s)
}


#[cfg(test)]
mod tests {
    use super::*;
    /// scrive gli script in /tmp/stf_scripts per provarli a mano come root
    #[test]
    fn genera_script() {
        let d = std::path::Path::new("/tmp/stf_scripts");
        let _ = std::fs::create_dir_all(d);
        let p = SmbParams { folder: "/srv/scan prova".into(), share: "Scan".into(), user: "scan".into(), password: "pw'$x`y".into(), smb1: true, policy: false };
        let f = FtpParams { folder: "/srv/scanftp".into(), user: "scanftp".into(), password: "pw'$x:y".into(), port: 2221, pasv_start: 50100, pasv_end: 50110 };
        for (n, s) in [("smb_setup", smb_setup(&p)), ("smb_remove", smb_remove(&p)), ("smb_status", smb_status()), ("ftp_setup", ftp_setup(&f)), ("ftp_remove", ftp_remove(&f))] {
            std::fs::write(d.join(format!("{n}.sh")), s).unwrap();
        }
    }
}
