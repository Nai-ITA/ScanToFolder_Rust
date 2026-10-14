// Scan to Folder 2.0 — pagina (Tauri). Tutto il lavoro lo fa il programma Rust.
'use strict';
const T = window.__TAURI__;
const invoke = (cmd, args) => T.core.invoke(cmd, args);
const $ = (s) => document.querySelector(s);
const $$ = (s) => [...document.querySelectorAll(s)];
let INFO = {};

// ── log ──
const logEl = $('#log');
function log(t) {
  logEl.textContent += t + '\n';
  logEl.scrollTop = logEl.scrollHeight;
}
T.event.listen('log', (e) => log(e.payload));
$('#log-clear').onclick = () => { logEl.textContent = ''; };

// ── tema ──
try { const t = localStorage.getItem('theme'); if (t) document.documentElement.dataset.theme = t; } catch { /* nulla */ }
$('#theme').onclick = () => {
  const cur = document.documentElement.dataset.theme
    || (matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light');
  const next = cur === 'dark' ? 'light' : 'dark';
  document.documentElement.dataset.theme = next;
  try { localStorage.setItem('theme', next); } catch { /* nulla */ }
};

// ── schede ──
for (const b of $$('.tab')) {
  b.onclick = () => {
    if (b.disabled) return;
    for (const x of $$('.tab')) x.classList.toggle('active', x === b);
    for (const p of $$('.pane')) p.hidden = p.id !== `pane-${b.dataset.tab}`;
  };
}

// ── conferma ──
function confirmBox(text) {
  return new Promise((resolve) => {
    $('#modal-text').textContent = text;
    $('#modal').hidden = false;
    const done = (v) => { $('#modal').hidden = true; resolve(v); };
    $('#modal-yes').onclick = () => done(true);
    $('#modal-no').onclick = () => done(false);
  });
}

// ── parametri ──
const num = (id, def) => { const n = parseInt($(id).value, 10); return Number.isFinite(n) ? n : def; };
const smb = () => ({
  folder: $('#smb-folder').value.trim(), share: $('#smb-share').value.trim(),
  user: $('#smb-user').value.trim(), password: $('#smb-pass').value,
  smb1: $('#smb-smb1').checked, policy: $('#smb-policy').checked,
});
const ftp = () => ({
  folder: $('#ftp-folder').value.trim(), user: $('#ftp-user').value.trim(), password: $('#ftp-pass').value,
  port: num('#ftp-port', 21), pasv_start: num('#ftp-pa', 49152), pasv_end: num('#ftp-pb', 49162),
});
const bi = () => ({
  folder: $('#bi-folder').value.trim(), user: $('#bi-user').value.trim(), password: $('#bi-pass').value,
  port: num('#bi-port', INFO.default_builtin_port || 2121), pasv_start: num('#bi-pa', 49152), pasv_end: num('#bi-pb', 49162),
});

$('#smb-smb1').onchange = () => { $('#smb1-warn').hidden = !$('#smb-smb1').checked; };

// ── azioni ──
async function busy(fn) {
  const btns = $$('button[data-act]');
  btns.forEach((b) => { b.disabled = true; });
  try { await fn(); } catch (e) { log(`[ERRORE] ${e}`); } finally {
    btns.forEach((b) => { b.disabled = false; });
    await refresh();
  }
}

const ACTIONS = {
  'smb-setup': () => busy(() => invoke('smb_setup', { p: smb() })),
  'smb-remove': async () => {
    const p = smb();
    if (await confirmBox(`Rimuovere l'utente '${p.user}' e la condivisione '${p.share}'? La cartella resta.`)) await busy(() => invoke('smb_remove', { p }));
  },
  'smb-status': () => busy(() => invoke('smb_status')),
  'ftp-setup': () => busy(() => invoke('ftp_setup', { p: ftp() })),
  'ftp-remove': async () => {
    const p = ftp();
    if (await confirmBox(`Rimuovere l'utente FTP '${p.user}' e il server FTP di sistema?`)) await busy(() => invoke('ftp_remove', { p }));
  },
  'bi-start': () => busy(async () => {
    await invoke('builtin_start', { p: bi() });
    $('#bi-pass').value = '';
    $('#bi-pass').placeholder = 'salvata (lascia vuoto per non cambiarla)';
  }),
  'bi-stop': () => busy(() => invoke('builtin_stop')),
  'bi-fw': () => busy(() => invoke('builtin_firewall', { p: bi(), add: true })),
};
for (const b of $$('button[data-act]')) b.onclick = () => ACTIONS[b.dataset.act]();

$('#bi-auto').onchange = async (e) => {
  try { e.target.checked = await invoke('autostart_set', { on: e.target.checked }); log(e.target.checked ? 'avvio automatico attivato' : 'avvio automatico disattivato'); } catch (err) { log(`[ERRORE] avvio automatico: ${err}`); }
};
$('#nai').onclick = (e) => { e.preventDefault(); invoke('open_url', { url: 'https://www.naict.it' }); };
$('#quit').onclick = async () => {
  if (INFO.builtin_running && !await confirmBox('Uscendo si ferma anche il server FTP integrato. Continuare?')) return;
  invoke('quit');
};

// ── stato ──
async function refresh() {
  INFO = await invoke('info');
  const running = INFO.builtin_running;
  $('#bi-dot').classList.toggle('on', running);
  $('#bi-state').textContent = running
    ? `server attivo · ${INFO.ip}:${INFO.builtin ? INFO.builtin.port : ''}`
    : 'server fermo';
  $('#bi-auto').checked = INFO.autostart;
}

async function init() {
  await refresh();
  const osName = { windows: 'Windows', linux: 'Linux', macos: 'macOS' }[INFO.os] || INFO.os;
  $('#sysinfo').textContent = `${osName} · ${INFO.host} · ${INFO.ip}`;
  $('#ver').textContent = `v${INFO.version}`;
  for (const id of ['#smb-folder', '#ftp-folder', '#bi-folder']) $(id).value = INFO.default_folder;
  $('#bi-port').value = INFO.default_builtin_port;
  if (INFO.os === 'windows') $('#ftp-port').value = 21;
  $('#row-smb1').hidden = !INFO.smb1;
  $('#row-policy').hidden = !INFO.policy;
  $('#bi-fw').hidden = !INFO.builtin_firewall;
  if (INFO.ftp_system) {
    $('#ftp-note').textContent = `Usa il server FTP del sistema: ${INFO.ftp_system_name}. Viene installato e configurato automaticamente e funziona anche a programma chiuso.`;
  } else {
    $('#ftp-note').textContent = 'Su questo sistema non c\'è un server FTP di sistema: usa la scheda "FTP integrato".';
    $('#ftp-form').hidden = true;
    $('#ftp-actions').hidden = true;
  }
  if (INFO.builtin) {
    const b = INFO.builtin;
    $('#bi-folder').value = b.folder; $('#bi-user').value = b.user; $('#bi-port').value = b.port;
    $('#bi-pa').value = b.pasv_start; $('#bi-pb').value = b.pasv_end;
    if (b.has_password) $('#bi-pass').placeholder = 'salvata (lascia vuoto per non cambiarla)';
  } else {
    $('#bi-pass').value = 'scan';
  }
  if (!INFO.admin) {
    const how = INFO.os === 'windows' ? 'Windows chiederà l\'autorizzazione (UAC)'
      : 'il sistema chiederà la password di amministratore';
    $('#banner').textContent = `Le operazioni su utenti, condivisioni e servizi richiedono i privilegi di amministratore: ${how} a ogni operazione. Il server FTP integrato non ne ha bisogno.`;
    $('#banner').hidden = false;
  }
  log(INFO.admin ? '✔ avviato con privilegi di amministratore' : 'pronto');
  if (INFO.builtin_running) log(`server FTP integrato attivo sulla porta ${INFO.builtin.port} (cartella ${INFO.builtin.folder})`);
}
init().catch((e) => log(`[ERRORE] avvio: ${e}`));
