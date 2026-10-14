//! Windows: stessi passi della versione Python (PowerShell), eseguiti con UAC.
//! I valori inseriti entrano nello script solo come stringhe tra apici singoli.

use crate::common::{psq, FtpParams, SmbParams};

pub const FTP_SYSTEM: bool = true;
pub const FTP_SYSTEM_NAME: &str = "IIS (servizio FTP di Windows)";
pub const SMB1_SUPPORTED: bool = true;
pub const POLICY_SUPPORTED: bool = true;
pub const DEFAULT_FOLDER: &str = r"C:\Scan";
pub const DEFAULT_BUILTIN_PORT: u16 = 21;

/// Gruppo firewall "Condivisione file e stampanti" indipendente dalla lingua di Windows.
const FPS_GROUP: &str = "@FirewallAPI.dll,-28502";

fn vars(pairs: &[(&str, &str)]) -> String {
    pairs.iter().map(|(k, v)| format!("${k} = {}\r\n", psq(v))).collect()
}

const ENSURE_FOLDER: &str = r##"
try {
    if (-not (Test-Path -LiteralPath $Folder)) { New-Item -ItemType Directory -Path $Folder | Out-Null; OK 'CREATA' }
    else { OK 'GIA ESISTENTE' }
} catch { F "cartella: $($_.Exception.Message)" }
"##;

fn ensure_user(fullname: &str, descr: &str) -> String {
    format!(
        r##"
try {{
    $sec = ConvertTo-SecureString $Password -AsPlainText -Force
    $u = Get-LocalUser -Name $User -ErrorAction SilentlyContinue
    if ($u) {{ Set-LocalUser -Name $User -Password $sec; Enable-LocalUser -Name $User; OK 'AGGIORNATO' }}
    else {{
        New-LocalUser -Name $User -Password $sec -FullName {} -Description {} -PasswordNeverExpires -UserMayNotChangePassword | Out-Null
        OK 'CREATO'
    }}
}} catch {{ F "utente locale: $($_.Exception.Message)" }}
"##,
        psq(fullname),
        psq(descr)
    )
}

const NTFS_MODIFY: &str = r##"
try {
    $acl = Get-Acl -LiteralPath $Folder
    $rule = New-Object System.Security.AccessControl.FileSystemAccessRule($User, 'Modify', 'ContainerInherit,ObjectInherit', 'None', 'Allow')
    $acl.SetAccessRule($rule)
    Set-Acl -LiteralPath $Folder -AclObject $acl
    OK 'Modify'
} catch { F "permessi NTFS: $($_.Exception.Message)" }
"##;

pub fn smb_setup(p: &SmbParams) -> String {
    let mut s = vars(&[("Folder", &p.folder), ("Share", &p.share), ("User", &p.user), ("Password", &p.password)]);
    s += "Say \"[1/8] Creazione cartella $Folder ...\"\r\n";
    s += ENSURE_FOLDER;
    s += "Say \"[2/8] Gestione utente '$User' ...\"\r\n";
    s += &ensure_user("Scan User", "Utente Scan to Folder");
    s += r##"
Say "[3/8] Aggiunta al gruppo Users ..."
try { Add-LocalGroupMember -SID 'S-1-5-32-545' -Member $User -ErrorAction Stop; OK 'aggiunto' }
catch { if ($_.Exception.Message -match 'already|gia|già') { OK 'gia presente' } else { W "gruppo Users: $($_.Exception.Message)" } }
Say "[4/8] Permessi NTFS (Modify) su $Folder ..."
"##;
    s += NTFS_MODIFY;
    s += r##"
Say "[5/8] Share SMB '$Share' ..."
try {
    $sh = Get-SmbShare -Name $Share -ErrorAction SilentlyContinue
    if ($sh) { Remove-SmbShare -Name $Share -Force }
    New-SmbShare -Name $Share -Path $Folder -FullAccess $User -Description 'Scan to Folder' | Out-Null
    OK 'OK'
} catch { F "share SMB: $($_.Exception.Message)" }
"##;
    s += &format!(
        "Say '[6/8] Firewall Condivisione file e stampanti ...'\r\ntry {{ Enable-NetFirewallRule -Group {} ; OK 'regole abilitate' }} catch {{ W \"firewall: $($_.Exception.Message)\" }}\r\n",
        psq(FPS_GROUP)
    );
    if p.smb1 {
        s += r##"
Say '[7/8] Abilitazione SMB v1 (per stampanti legacy) ...'
try {
    Set-SmbServerConfiguration -EnableSMB1Protocol $true -Force -Confirm:$false
    Enable-WindowsOptionalFeature -Online -FeatureName SMB1Protocol -NoRestart -ErrorAction SilentlyContinue | Out-Null
    Set-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Services\LanmanServer\Parameters' -Name 'SMB1' -Value 1 -Type DWord -Force
    OK 'SMB v1 abilitato (riavvio consigliato)'
} catch { W "SMB v1: $($_.Exception.Message)" }
"##;
    } else {
        s += "Say '[7/8] SMB v1 - saltato (non selezionato)'\r\n";
    }
    if p.policy {
        s += r##"
Say '[8/8] Correzione policy di sicurezza ...'
try {
    $wk = 'HKLM:\SYSTEM\CurrentControlSet\Services\LanmanWorkstation\Parameters'
    $sv = 'HKLM:\SYSTEM\CurrentControlSet\Services\LanmanServer\Parameters'
    $lsa = 'HKLM:\SYSTEM\CurrentControlSet\Control\Lsa'
    Set-ItemProperty -Path $wk -Name 'AllowInsecureGuestAuth' -Value 1 -Type DWord -Force
    Set-ItemProperty -Path $wk -Name 'RequireSecuritySignature' -Value 0 -Type DWord -Force
    Set-ItemProperty -Path $sv -Name 'RequireSecuritySignature' -Value 0 -Type DWord -Force
    Set-ItemProperty -Path $lsa -Name 'LimitBlankPasswordUse' -Value 0 -Type DWord -Force
    Set-ItemProperty -Path $lsa -Name 'LmCompatibilityLevel' -Value 1 -Type DWord -Force
    OK 'Policy aggiornate'
} catch { W "policy di sicurezza: $($_.Exception.Message)" }
"##;
    } else {
        s += "Say '[8/8] Policy - saltato (non selezionato)'\r\n";
    }
    s
}

pub fn smb_remove(p: &SmbParams) -> String {
    let mut s = vars(&[("Share", &p.share), ("User", &p.user)]);
    s += r##"
Say '[1/6] Rimozione share SMB ...'
try { Remove-SmbShare -Name $Share -Force -ErrorAction Stop; OK 'rimossa' } catch { W "rimozione share: $($_.Exception.Message)" }
Say '[2/6] Rimozione utente ...'
try { Remove-LocalUser -Name $User -ErrorAction Stop; OK 'rimosso' } catch { W "rimozione utente: $($_.Exception.Message)" }
Say '[3/6] Cartella conservata (eliminare manualmente se necessario).'
"##;
    if p.smb1 {
        s += r##"
Say '[4/6] Disabilitazione SMB v1 ...'
try {
    Set-SmbServerConfiguration -EnableSMB1Protocol $false -Force -Confirm:$false
    Disable-WindowsOptionalFeature -Online -FeatureName SMB1Protocol -NoRestart -ErrorAction SilentlyContinue | Out-Null
    Set-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Services\LanmanServer\Parameters' -Name 'SMB1' -Value 0 -Type DWord -Force
    OK 'SMB v1 disabilitato (riavvio consigliato)'
} catch { W "disabilitazione SMB v1: $($_.Exception.Message)" }
"##;
    } else {
        s += "Say '[4/6] SMB v1 - saltato (non selezionato)'\r\n";
    }
    s += &format!(
        "Say '[5/6] Ripristino regola firewall ...'\r\ntry {{ Disable-NetFirewallRule -Group {} ; OK 'regole disabilitate' }} catch {{ W \"firewall: $($_.Exception.Message)\" }}\r\n",
        psq(FPS_GROUP)
    );
    if p.policy {
        s += r##"
Say '[6/6] Ripristino policy di sicurezza ...'
try {
    $lsa = 'HKLM:\SYSTEM\CurrentControlSet\Control\Lsa'
    $wk  = 'HKLM:\SYSTEM\CurrentControlSet\Services\LanmanWorkstation\Parameters'
    $sv  = 'HKLM:\SYSTEM\CurrentControlSet\Services\LanmanServer\Parameters'
    Set-ItemProperty -Path $wk  -Name 'AllowInsecureGuestAuth'   -Value 0 -Type DWord -Force
    Set-ItemProperty -Path $wk  -Name 'RequireSecuritySignature' -Value 0 -Type DWord -Force
    Set-ItemProperty -Path $sv  -Name 'RequireSecuritySignature' -Value 0 -Type DWord -Force
    Set-ItemProperty -Path $lsa -Name 'LimitBlankPasswordUse'    -Value 1 -Type DWord -Force
    Set-ItemProperty -Path $lsa -Name 'LmCompatibilityLevel'     -Value 3 -Type DWord -Force
    OK 'Policy ripristinate ai default Windows'
} catch { W "ripristino policy: $($_.Exception.Message)" }
"##;
    } else {
        s += "Say '[6/6] Policy - saltato (non selezionato)'\r\n";
    }
    s += "Say ''\r\nSay 'Rimozione completata.'\r\n";
    s
}

pub fn smb_status() -> String {
    r##"
Say '--- SHARE ATTIVE ---'
Say ((Get-SmbShare | Where-Object { $_.Name -notmatch '\$$' } | Format-Table Name,Path,Description -AutoSize | Out-String).Trim())
Say ''
Say '--- UTENTI LOCALI ---'
Say ((Get-LocalUser | Format-Table Name,Enabled,PasswordNeverExpires -AutoSize | Out-String).Trim())
Say ''
Say '--- SMB VERSIONI ---'
Say ((Get-SmbServerConfiguration | Select-Object EnableSMB1Protocol,EnableSMB2Protocol | Format-List | Out-String).Trim())
"##
    .to_string()
}

pub fn ftp_setup(p: &FtpParams) -> String {
    let port = p.port.to_string();
    let (a, b) = (p.pasv_start.to_string(), p.pasv_end.to_string());
    let mut s = vars(&[("Folder", &p.folder), ("User", &p.user), ("Password", &p.password), ("Port", &port), ("PasvStart", &a), ("PasvEnd", &b)]);
    s += r##"
Say '[1/7] Installazione IIS e FTP Server (potrebbe richiedere qualche minuto)...'
try {
    foreach ($f in @('IIS-WebServerRole','IIS-WebServer','IIS-FTPServer','IIS-FTPSvc','IIS-FTPExtensibility','IIS-ManagementConsole')) {
        Enable-WindowsOptionalFeature -Online -FeatureName $f -All -NoRestart -ErrorAction SilentlyContinue | Out-Null
    }
    OK 'feature IIS/FTP'
} catch { W "feature IIS/FTP: $($_.Exception.Message)" }
Say "[2/7] Cartella $Folder ..."
"##;
    s += ENSURE_FOLDER;
    s += "Say \"[3/7] Utente locale '$User' ...\"\r\n";
    s += &ensure_user("FTP Scan User", "Utente FTP Scan to Folder");
    s += "Say \"[4/7] Permessi NTFS (Modify) su $Folder ...\"\r\n";
    s += NTFS_MODIFY;
    s += r##"
Say "[5/7] Configurazione sito FTP (porta $Port) ..."
try {
    Import-Module WebAdministration
    if (Get-WebSite -Name 'ScanFTP' -ErrorAction SilentlyContinue) { Remove-Website -Name 'ScanFTP' }
    New-WebFtpSite -Name 'ScanFTP' -Port ([int]$Port) -PhysicalPath $Folder -Force | Out-Null
    Set-ItemProperty 'IIS:\Sites\ScanFTP' -Name ftpServer.security.authentication.basicAuthentication.enabled -Value $true
    Set-ItemProperty 'IIS:\Sites\ScanFTP' -Name ftpServer.security.authentication.anonymousAuthentication.enabled -Value $false
    Set-ItemProperty 'IIS:\Sites\ScanFTP' -Name ftpServer.security.ssl.controlChannelPolicy -Value 0
    Set-ItemProperty 'IIS:\Sites\ScanFTP' -Name ftpServer.security.ssl.dataChannelPolicy -Value 0
    Add-WebConfiguration '/system.ftpServer/security/authorization' -Value @{accessType='Allow'; users=$User; permissions='Read,Write'} -PSPath 'IIS:' -Location 'ScanFTP'
    Set-WebConfigurationProperty -PSPath 'MACHINE/WEBROOT/APPHOST' -Filter 'system.ftpServer/firewallSupport' -Name 'lowDataChannelPort' -Value ([int]$PasvStart)
    Set-WebConfigurationProperty -PSPath 'MACHINE/WEBROOT/APPHOST' -Filter 'system.ftpServer/firewallSupport' -Name 'highDataChannelPort' -Value ([int]$PasvEnd)
    Start-WebSite -Name 'ScanFTP' -ErrorAction SilentlyContinue
    OK "Sito FTP 'ScanFTP' creato e avviato"
} catch { W "sito FTP (potrebbe servire un riavvio di IIS): $($_.Exception.Message)" }
Say '[6/7] Avvio servizio FTPSVC ...'
try { Set-Service -Name 'FTPSVC' -StartupType Automatic; Start-Service -Name 'FTPSVC'; OK 'servizio FTPSVC avviato' } catch { W "servizio FTPSVC: $($_.Exception.Message)" }
Say "[7/7] Regole firewall (porta $Port + passivo $PasvStart-$PasvEnd) ..."
try {
    if (-not (Get-NetFirewallRule -DisplayName 'Scan FTP Control' -ErrorAction SilentlyContinue)) {
        New-NetFirewallRule -DisplayName 'Scan FTP Control' -Direction Inbound -LocalPort ([int]$Port) -Protocol TCP -Action Allow | Out-Null }
    if (-not (Get-NetFirewallRule -DisplayName 'Scan FTP Passive' -ErrorAction SilentlyContinue)) {
        New-NetFirewallRule -DisplayName 'Scan FTP Passive' -Direction Inbound -LocalPort "$PasvStart-$PasvEnd" -Protocol TCP -Action Allow | Out-Null }
    OK 'regole firewall'
} catch { W "firewall FTP: $($_.Exception.Message)" }
"##;
    s
}

pub fn ftp_remove(p: &FtpParams) -> String {
    let mut s = vars(&[("User", &p.user)]);
    s += r##"
Say "[1/4] Stop e rimozione sito FTP 'ScanFTP' ..."
try { Import-Module WebAdministration; Stop-WebSite -Name 'ScanFTP' -ErrorAction SilentlyContinue; Remove-Website -Name 'ScanFTP' -ErrorAction SilentlyContinue; OK 'rimosso' } catch { W "rimozione sito FTP: $($_.Exception.Message)" }
Say '[2/4] Rimozione utente locale ...'
try { Remove-LocalUser -Name $User -ErrorAction Stop; OK 'rimosso' } catch { W "rimozione utente: $($_.Exception.Message)" }
Say '[3/4] Rimozione regole firewall FTP ...'
try { Remove-NetFirewallRule -DisplayName 'Scan FTP Control' -ErrorAction SilentlyContinue; Remove-NetFirewallRule -DisplayName 'Scan FTP Passive' -ErrorAction SilentlyContinue; OK 'rimosse' } catch { W "firewall: $($_.Exception.Message)" }
Say '[4/4] Servizio FTPSVC impostato su Manual ...'
try { Set-Service -Name 'FTPSVC' -StartupType Manual; OK 'Manual' } catch { W "servizio FTPSVC: $($_.Exception.Message)" }
Say 'Rimozione FTP completata.'
"##;
    s
}

/// Regola firewall per il server FTP integrato (porta di controllo + passive).
pub fn builtin_firewall(port: u16, a: u16, b: u16, add: bool) -> Option<String> {
    let mut s = vars(&[("Port", &port.to_string()), ("Range", &format!("{a}-{b}"))]);
    if add {
        s += r##"
Say '[firewall] regole per il server FTP integrato ...'
try {
    Remove-NetFirewallRule -DisplayName 'Scan FTP integrato' -ErrorAction SilentlyContinue
    New-NetFirewallRule -DisplayName 'Scan FTP integrato' -Direction Inbound -LocalPort @("$Port", $Range) -Protocol TCP -Action Allow | Out-Null
    OK "porta $Port e $Range aperte"
} catch { W "firewall: $($_.Exception.Message)" }
"##;
    } else {
        s += "try { Remove-NetFirewallRule -DisplayName 'Scan FTP integrato' -ErrorAction Stop; OK 'regola firewall rimossa' } catch { W 'nessuna regola da rimuovere' }\r\n";
    }
    Some(s)
}
