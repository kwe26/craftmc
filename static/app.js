// CraftMC control UI
const $ = (sel, root=document) => root.querySelector(sel);
const $$ = (sel, root=document) => Array.from(root.querySelectorAll(sel));
const main = $('#main');
const toastEl = $('#toast');
const tasksDock = $('#tasks-dock');

function toast(msg, kind='') {
  toastEl.className = 'toast show ' + kind;
  toastEl.textContent = msg;
  clearTimeout(toast._t);
  toast._t = setTimeout(() => toastEl.className = 'toast', 2400);
}

async function api(path, opts={}) {
  const r = await fetch('/api' + path, {
    headers: opts.body && !(opts.body instanceof FormData) ? {'content-type':'application/json'} : {},
    ...opts,
  });
  if (r.status === 401) { location.href = '/login'; throw new Error('unauthorized'); }
  if (r.status === 503) { location.href = '/setup'; throw new Error('not setup'); }
  if (!r.ok) {
    let err = 'request failed';
    try { err = (await r.json()).error || err; } catch {}
    toast(err, 'err');
    throw new Error(err);
  }
  if (r.headers.get('content-type')?.includes('application/json')) return r.json();
  return r;
}

const escapeHtml = s => (s||'').replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const humanSize = n => { const u=['B','KB','MB','GB','TB']; let i=0; while(n>=1024 && i<u.length-1){n/=1024;i++;} return n.toFixed(n>=10||i===0?0:1)+' '+u[i]; };
const humanRate = bps => humanSize(bps) + '/s';

// ---------- View routing ----------
const views = {};
function nav(id) {
  $$('.nav-item').forEach(n => n.classList.toggle('active', n.dataset.view === id));
  if (views[id]) {
    main.classList.remove('animate-fade-in');
    void main.offsetWidth; // restart animation
    main.classList.add('animate-fade-in');
    views[id]();
  }
  location.hash = id;
}
$$('.nav-item').forEach(n => n.addEventListener('click', () => n.dataset.view && nav(n.dataset.view)));
$('#logoutBtn').addEventListener('click', async () => {
  await api('/logout', { method: 'POST' }).catch(()=>{});
  location.href = '/login';
});

// ---------- Live tasks + metrics over WebSocket ----------
const liveState = { tasks: new Map(), metrics: null, server: null, players: [], listeners: { metrics: new Set(), server: new Set(), players: new Set() } };

function onMetrics(fn) { liveState.listeners.metrics.add(fn); return () => liveState.listeners.metrics.delete(fn); }
function onServer(fn) { liveState.listeners.server.add(fn); return () => liveState.listeners.server.delete(fn); }
function onPlayers(fn) { liveState.listeners.players.add(fn); return () => liveState.listeners.players.delete(fn); }

function connectTasksWs() {
  const url = (location.protocol === 'https:' ? 'wss://' : 'ws://') + location.host + '/ws/tasks';
  const ws = new WebSocket(url);
  ws.onmessage = (ev) => {
    let m; try { m = JSON.parse(ev.data); } catch { return; }
    if (m.type === 'snapshot') {
      liveState.tasks.clear();
      for (const t of m.tasks) liveState.tasks.set(t.id, t);
      liveState.players = m.players || [];
      liveState.listeners.players.forEach(fn => fn(liveState.players));
      renderDock(); renderSidebarStatus();
    } else if (m.type === 'task') {
      liveState.tasks.set(m.task.id, m.task);
      renderDock();
    } else if (m.type === 'players') {
      liveState.players = m.players || [];
      liveState.listeners.players.forEach(fn => fn(liveState.players));
      renderSidebarStatus();
    } else if (m.type === 'metrics') {
      liveState.metrics = m.metrics;
      liveState.server = m.server;
      liveState.listeners.metrics.forEach(fn => fn(m.metrics));
      liveState.listeners.server.forEach(fn => fn(m.server));
      renderSidebarStatus();
    }
  };
  ws.onclose = () => setTimeout(connectTasksWs, 2000);
}

function renderSidebarStatus() {
  const s = liveState.server, m = liveState.metrics;
  if (!s || !m) return;
  const tagColor = ({stopped:'bg-edge text-subtle',starting:'bg-yellow-100 text-yellow-800',running:'bg-emerald-100 text-emerald-800',stopping:'bg-yellow-100 text-yellow-800',crashed:'bg-rose-100 text-rose-800'})[s.status] || 'bg-edge text-subtle';
  const dot = s.status === 'running' ? 'bg-emerald-500 animate-pulse-glow' : s.status === 'crashed' ? 'bg-rose-500' : 'bg-subtle';
  $('#sidebar-status').innerHTML = `
    <div class="flex items-center gap-2 mb-2">
      <span class="inline-block w-2 h-2 rounded-full ${dot}"></span>
      <span class="px-2 py-0.5 rounded-full text-[11px] ${tagColor}">${s.status}</span>
    </div>
    <div class="text-[11px] leading-relaxed">
      CPU ${m.cpu_percent.toFixed(0)}% · RAM ${m.mem_percent.toFixed(0)}%<br/>
      Java ${s.pid ?? '—'} · ${m.server_mem_mb ? m.server_mem_mb + ' MB' : '—'}<br/>
      Players: ${liveState.players.length}
    </div>`;
}

function renderDock() {
  const visible = [...liveState.tasks.values()]
    .sort((a,b) => new Date(b.started_at) - new Date(a.started_at))
    .filter(t => t.status === 'running' || (t.finished_at && (Date.now() - new Date(t.finished_at).getTime()) < 6000))
    .slice(0, 4);
  tasksDock.innerHTML = visible.map(taskCard).join('');
}

function taskCard(t) {
  const pct = t.total ? Math.min(100, Math.round((t.progress / t.total) * 100)) : null;
  const bar = pct !== null
    ? `<div class="h-1.5 bg-edge rounded-full overflow-hidden mt-2"><div class="h-full bg-accent rounded-full transition-all duration-300" style="width:${pct}%"></div></div>`
    : `<div class="h-1.5 bg-edge rounded-full overflow-hidden mt-2 relative"><div class="absolute inset-y-0 left-0 w-1/4 bg-accent/70 rounded-full animate-progress-indet"></div></div>`;
  const status =
    t.status === 'done' ? `<span class="text-emerald-700">✓ done</span>` :
    t.status === 'failed' ? `<span class="text-rose-700">✗ failed</span>` :
    `<span class="text-subtle">${pct !== null ? pct + '%' : 'working'}</span>`;
  const detail = t.total
    ? `${humanSize(t.progress)} / ${humanSize(t.total)}`
    : (t.progress ? humanSize(t.progress) : '');
  const ring = t.status === 'failed' ? 'ring-1 ring-rose-200' : t.status === 'done' ? 'ring-1 ring-emerald-200' : '';
  return `
    <div class="pointer-events-auto bg-surface border border-edge rounded-xl p-3 shadow-sm animate-slide-up ${ring}">
      <div class="flex items-center justify-between gap-2">
        <div class="text-[13px] font-medium truncate">${escapeHtml(t.label)}</div>
        <div class="text-[11px]">${status}</div>
      </div>
      <div class="text-[11px] text-subtle truncate">${escapeHtml(t.message || detail)}</div>
      ${t.status === 'running' ? bar : ''}
    </div>`;
}

// ---------- Dashboard ----------
views.dashboard = async () => {
  main.innerHTML = `
    <h1 class="font-serif text-3xl mb-1">Dashboard</h1>
    <p class="text-subtle mb-5">Overview, controls, and live system metrics.</p>

    <div class="grid grid-cols-1 md:grid-cols-3 gap-3 mb-4">
      ${metricCard('System CPU', 'sysCpu', '%', 'animate-fade-in')}
      ${metricCard('System RAM', 'sysMem', 'MB', 'animate-fade-in')}
      ${metricCard('Disk', 'disk', 'GB', 'animate-fade-in')}
    </div>
    <div class="grid grid-cols-1 md:grid-cols-3 gap-3 mb-4">
      ${metricCard('Server CPU', 'srvCpu', '%', 'animate-fade-in')}
      ${metricCard('Server RAM', 'srvMem', 'MB', 'animate-fade-in')}
      ${metricCard('Disk read / write', 'diskIO', '/s', 'animate-fade-in')}
    </div>

    <div class="bg-surface border border-edge rounded-xl p-4 animate-fade-in mb-4">
      <div class="flex items-center gap-2 flex-wrap">
        <button class="primary" id="btnStart">Start</button>
        <button id="btnStop">Stop</button>
        <button id="btnRestart">Restart</button>
        <button class="danger" id="btnKill">Kill</button>
        <span class="flex-1"></span>
        <button id="btnBackupNow">Backup worlds now</button>
      </div>
      <div id="status-row" class="mt-3 text-[13px] text-subtle"></div>
    </div>

    <div class="bg-surface border border-edge rounded-xl p-4 animate-fade-in">
      <div class="flex items-center justify-between mb-2">
        <div class="text-[11px] uppercase tracking-wider text-subtle">Online players</div>
        <div class="text-[11px] text-subtle"><span id="dashPlayerCount">0</span> online</div>
      </div>
      <div id="dashPlayers" class="flex flex-wrap gap-2 min-h-[28px]"></div>
    </div>`;

  const animate = (el) => { el.classList.remove('animate-fade-in'); void el.offsetWidth; el.classList.add('animate-fade-in'); };
  const button = (sel, fn, t) => $(sel).addEventListener('click', async () => { try { await fn(); toast(t,'ok'); animate($(sel)); } catch{} });
  button('#btnStart', () => api('/server/start',{method:'POST'}), 'Starting');
  button('#btnStop', () => api('/server/stop',{method:'POST'}), 'Stopping');
  button('#btnRestart', () => api('/server/restart',{method:'POST'}), 'Restarting');
  $('#btnKill').onclick = () => { if (confirm('Force-kill the server process?')) api('/server/kill',{method:'POST'}).catch(()=>{}); };
  $('#btnBackupNow').onclick = () => api('/backups',{method:'POST'}).then(()=>toast('Backup queued','ok')).catch(()=>{});

  // Sparkline state
  const histCpu = [], histMem = [], histSrv = [], histRead = [], histWrite = [];
  const off1 = onMetrics(m => {
    push(histCpu, m.cpu_percent);
    push(histMem, m.mem_percent);
    push(histSrv, m.server_cpu_percent || 0);
    push(histRead, m.disk_read_bps);
    push(histWrite, m.disk_write_bps);

    setMetric('sysCpu', m.cpu_percent.toFixed(1), '%', m.cpu_percent, 100, histCpu);
    setMetric('sysMem', `${m.mem_used_mb} / ${m.mem_total_mb}`, 'MB', m.mem_percent, 100, histMem);
    setMetric('disk', `${m.disk_used_gb.toFixed(1)} / ${m.disk_total_gb.toFixed(0)}`, 'GB', m.disk_total_gb ? (m.disk_used_gb/m.disk_total_gb*100) : 0, 100, []);
    setMetric('srvCpu', (m.server_cpu_percent ?? 0).toFixed(1), '%', m.server_cpu_percent ?? 0, 200, histSrv);
    setMetric('srvMem', m.server_mem_mb ?? '—', 'MB', m.server_mem_mb ?? 0, m.mem_total_mb || 1, []);
    const total = (m.disk_read_bps + m.disk_write_bps) || 1;
    $('#diskIO-value').innerHTML = `<span class="text-emerald-700">↓ ${humanRate(m.disk_read_bps)}</span> &nbsp; <span class="text-accent">↑ ${humanRate(m.disk_write_bps)}</span>`;
    drawSparkline($('#diskIO-spark'), histRead, histWrite);
  });
  const off2 = onServer(s => {
    const tag = ({stopped:'bg-edge text-subtle',starting:'bg-yellow-100 text-yellow-800',running:'bg-emerald-100 text-emerald-800',stopping:'bg-yellow-100 text-yellow-800',crashed:'bg-rose-100 text-rose-800'})[s.status] || '';
    $('#status-row').innerHTML = `
      <span class="px-2 py-0.5 rounded-full text-[11px] ${tag}">${s.status}</span>
      <span class="ml-3">PID ${s.pid ?? '—'}</span>
      <span class="ml-3">Uptime ${s.started_at ? formatUptime(Date.now() - new Date(s.started_at).getTime()) : '—'}</span>
      <span class="ml-3">Auto-restarts ${s.restart_count}</span>`;
  });
  const renderDashPlayers = (players) => {
    $('#dashPlayerCount').textContent = players.length;
    const host = $('#dashPlayers');
    if (!players.length) { host.innerHTML = `<div class="text-[12px] text-subtle italic">No one online.</div>`; return; }
    host.innerHTML = players.map(p => `
      <div class="flex items-center gap-2 bg-paper border border-edge rounded-full pl-1 pr-3 py-1 animate-slide-up">
        <img src="https://mc-heads.net/avatar/${encodeURIComponent(p.name)}/24" class="w-6 h-6 rounded-full" alt=""/>
        <span class="text-[13px]">${escapeHtml(p.name)}</span>
      </div>`).join('');
  };
  const off3 = onPlayers(renderDashPlayers);
  renderDashPlayers(liveState.players);

  // Force first paint with whatever we already have.
  if (liveState.metrics) liveState.listeners.metrics.forEach(fn => fn(liveState.metrics));
  if (liveState.server) liveState.listeners.server.forEach(fn => fn(liveState.server));
  views.dashboard._cleanup = () => { off1(); off2(); off3(); };
};

function metricCard(label, id, unit, anim='') {
  return `
    <div class="bg-surface border border-edge rounded-xl p-4 ${anim} hover:shadow-sm transition-shadow">
      <div class="flex items-center justify-between mb-1">
        <div class="text-[11px] uppercase tracking-wider text-subtle">${label}</div>
        <div class="text-[11px] text-subtle">${unit}</div>
      </div>
      <div class="font-serif text-2xl tabular-nums" id="${id}-value">—</div>
      <div class="h-1.5 bg-edge rounded-full overflow-hidden mt-3"><div class="h-full bg-accent rounded-full transition-[width] duration-500" style="width:0%" id="${id}-bar"></div></div>
      <svg id="${id}-spark" viewBox="0 0 120 24" class="w-full h-6 mt-2 text-accent"></svg>
    </div>`;
}

function setMetric(id, value, unit, num, max, history) {
  const v = $(`#${id}-value`); if (v) v.textContent = `${value}`;
  const bar = $(`#${id}-bar`);
  if (bar && max) bar.style.width = Math.min(100, (num / max) * 100) + '%';
  if (history && history.length) drawSparkline($(`#${id}-spark`), history);
}

function push(arr, v) { arr.push(Number(v) || 0); if (arr.length > 60) arr.shift(); }

function drawSparkline(svg, primary, secondary) {
  if (!svg) return;
  const W = 120, H = 24;
  const series = [primary, secondary || []];
  const all = [].concat(...series.filter(s => s.length));
  if (!all.length) { svg.innerHTML = ''; return; }
  const max = Math.max(...all, 1);
  const path = (s, color) => {
    if (!s.length) return '';
    const step = W / Math.max(1, s.length - 1);
    const pts = s.map((v,i) => `${(i*step).toFixed(1)},${(H - (v/max)*H).toFixed(1)}`).join(' ');
    return `<polyline fill="none" stroke="${color}" stroke-width="1.5" points="${pts}"/>`;
  };
  svg.innerHTML = `${path(primary, '#c96442')}${secondary ? path(secondary, '#6ea884') : ''}`;
}

function formatUptime(ms) {
  const s = Math.floor(ms/1000); const d = Math.floor(s/86400), h = Math.floor((s%86400)/3600), m = Math.floor((s%3600)/60);
  if (d) return `${d}d ${h}h`; if (h) return `${h}h ${m}m`; return `${m}m`;
}

// ---------- Console ----------
let term, fitAddon, ws;
views.console = async () => {
  if (views._consoleTimer) clearInterval(views._consoleTimer);
  main.innerHTML = `
    <h1 class="font-serif text-3xl mb-1">Console</h1>
    <p class="text-subtle mb-4">Live server output. Use the input below to send commands.</p>
    <div class="flex items-center gap-2 mb-3 flex-wrap">
      <button class="primary" id="cStart">Start</button>
      <button id="cStop">Stop</button>
      <button id="cRestart">Restart</button>
      <span id="cStatus" class="px-2 py-0.5 rounded-full text-[11px] bg-edge text-subtle transition-colors">—</span>
      <span class="flex-1"></span>
      <button id="cClear">Clear view</button>
    </div>
    <div id="term-host" class="rounded-xl"></div>
    <div class="flex gap-2 mt-2">
      <input id="cmdInput" class="flex-1 font-mono" placeholder="Enter server command (no leading slash)" autocomplete="off" />
      <button class="primary" id="cmdSend">Send</button>
    </div>`;
  $('#cStart').onclick = () => api('/server/start',{method:'POST'}).catch(()=>{});
  $('#cStop').onclick = () => api('/server/stop',{method:'POST'}).catch(()=>{});
  $('#cRestart').onclick = () => api('/server/restart',{method:'POST'}).catch(()=>{});
  $('#cClear').onclick = () => term && term.clear();

  if (term) { try { term.dispose(); } catch {} }
  term = new Terminal({
    fontFamily: 'ui-monospace, "Cascadia Mono", "JetBrains Mono", Consolas, monospace',
    fontSize: 13, theme: { background: '#1a1a1a', foreground: '#e8e4dc' },
    scrollback: 5000, convertEol: true, cursorBlink: false, disableStdin: true,
  });
  fitAddon = new FitAddon.FitAddon();
  term.loadAddon(fitAddon);
  term.open($('#term-host'));
  setTimeout(() => fitAddon.fit(), 50);
  window.addEventListener('resize', () => fitAddon && fitAddon.fit());

  const wsUrl = (location.protocol === 'https:' ? 'wss://' : 'ws://') + location.host + '/ws/console';
  ws = new WebSocket(wsUrl);
  ws.onmessage = (ev) => {
    let m; try { m = JSON.parse(ev.data); } catch { return; }
    if (m.type === 'snapshot' || m.type === 'lines') {
      const buf = m.lines.map(formatLine).join('');
      term.write(buf);
    } else if (m.type === 'lagged') {
      term.write(`\x1b[33m[manager] dropped ${m.skipped} buffered lines (slow client)\x1b[0m\r\n`);
    } else if (m.type === 'error') {
      term.write(`\x1b[31m[manager] ${m.msg}\x1b[0m\r\n`);
    }
  };
  ws.onclose = () => { term && term.write('\r\n\x1b[33m[manager] console disconnected\x1b[0m\r\n'); };

  $('#cmdSend').onclick = sendCmd;
  $('#cmdInput').addEventListener('keydown', e => { if (e.key === 'Enter') sendCmd(); });

  const setStatus = s => {
    const tag = ({stopped:'bg-edge text-subtle',starting:'bg-yellow-100 text-yellow-800',running:'bg-emerald-100 text-emerald-800',stopping:'bg-yellow-100 text-yellow-800',crashed:'bg-rose-100 text-rose-800'})[s.status] || 'bg-edge text-subtle';
    const el = $('#cStatus'); if (!el) return;
    el.className = `px-2 py-0.5 rounded-full text-[11px] transition-colors ${tag}`;
    el.textContent = s.status;
  };
  if (liveState.server) setStatus(liveState.server);
  views._consoleOff = onServer(setStatus);
};
function sendCmd() {
  const inp = $('#cmdInput'); if (!inp || !ws || ws.readyState !== 1) return;
  const cmd = inp.value.trim(); if (!cmd) return;
  ws.send(JSON.stringify({ type:'cmd', cmd }));
  inp.value = '';
}
function formatLine(l) {
  const ts = new Date(l.ts).toLocaleTimeString();
  const color = l.stream === 'stderr' ? '\x1b[31m' : (l.stream === 'system' ? '\x1b[36m' : '');
  const reset = color ? '\x1b[0m' : '';
  return `\x1b[90m${ts}\x1b[0m ${color}${l.line}${reset}\r\n`;
}

// ---------- Config ----------
views.config = async () => {
  const cfg = await api('/config');
  main.innerHTML = `
    <h1 class="font-serif text-3xl mb-1">Configuration</h1>
    <p class="text-subtle mb-5">Manager and JVM settings.</p>
    <div class="bg-surface border border-edge rounded-xl p-5 animate-slide-up">
      <div class="row">
        <div class="field"><label>Bind host</label><input id="host" value="${escapeHtml(cfg.host)}"/></div>
        <div class="field"><label>MOTD</label><input id="motd" value="${escapeHtml(cfg.motd)}"/></div>
      </div>
      <div class="row">
        <div class="field"><label>Min RAM MB</label><input id="min_ram_mb" type="number" value="${cfg.min_ram_mb}"/></div>
        <div class="field"><label>Max RAM MB</label><input id="max_ram_mb" type="number" value="${cfg.max_ram_mb}"/></div>
        <div class="field"><label>Java path</label><input id="java_path" value="${escapeHtml(cfg.java_path)}"/></div>
      </div>
      <div class="field"><label>JVM args (one per line)</label>
        <textarea id="jvm_args" rows="4">${escapeHtml((cfg.jvm_args||[]).join('\n'))}</textarea></div>
      <div class="row">
        <div class="field"><label>On crash</label>
          <select id="crash_action">
            ${['restart_with_backoff','restart','stop'].map(v => `<option value="${v}" ${cfg.crash_action===v?'selected':''}>${v.replace(/_/g,' ')}</option>`).join('')}
          </select></div>
        <div class="field"><label>Max auto-restarts</label><input id="auto_restart_max" type="number" value="${cfg.auto_restart_max}"/></div>
        <div class="field"><label>PaperMC URL</label><input id="paper_url" value="${escapeHtml(cfg.paper_url||'')}"/></div>
      </div>
      <hr class="my-4 border-edge"/>
      <div class="row">
        <div class="field"><label>Auto-backup</label>
          <select id="auto_backup_enabled">
            <option value="true" ${cfg.auto_backup_enabled?'selected':''}>Enabled</option>
            <option value="false" ${!cfg.auto_backup_enabled?'selected':''}>Disabled</option>
          </select></div>
        <div class="field"><label>Interval (minutes)</label><input id="auto_backup_interval_minutes" type="number" value="${cfg.auto_backup_interval_minutes}"/></div>
        <div class="field"><label>Keep latest N</label><input id="backup_keep" type="number" value="${cfg.backup_keep}"/></div>
      </div>
      <button class="primary" id="saveCfg">Save</button>
    </div>`;
  $('#saveCfg').onclick = async () => {
    const v = id => $('#'+id).value;
    const body = {
      host: v('host'), motd: v('motd'),
      min_ram_mb: parseInt(v('min_ram_mb')), max_ram_mb: parseInt(v('max_ram_mb')),
      java_path: v('java_path'),
      jvm_args: v('jvm_args').split('\n').map(s=>s.trim()).filter(Boolean),
      crash_action: v('crash_action'),
      auto_restart_max: parseInt(v('auto_restart_max')),
      paper_url: v('paper_url'),
      auto_backup_enabled: v('auto_backup_enabled') === 'true',
      auto_backup_interval_minutes: parseInt(v('auto_backup_interval_minutes')),
      backup_keep: parseInt(v('backup_keep')),
    };
    await api('/config', { method:'PUT', body: JSON.stringify(body) });
    toast('Saved', 'ok');
  };
};

// ---------- server.properties ----------
views.properties = async () => {
  const r = await api('/server-properties');
  main.innerHTML = `
    <h1 class="font-serif text-3xl mb-1">server.properties</h1>
    <p class="text-subtle mb-4">Edit Minecraft server.properties. Restart the server to apply changes.</p>
    <div class="bg-surface border border-edge rounded-xl p-4 animate-fade-in">
      <textarea id="props" rows="22" spellcheck="false">${escapeHtml(r.content||'')}</textarea>
      <div class="mt-2"><button class="primary" id="save">Save</button></div>
    </div>`;
  $('#save').onclick = async () => {
    await api('/server-properties', { method:'PUT', body: JSON.stringify({ content: $('#props').value }) });
    toast('Saved','ok');
  };
};

// ---------- Join messages ----------
views.messages = async () => {
  const r = await api('/server-properties');
  const props = parseProps(r.content || '');
  const motd = props['motd'] || '';
  let custom = '';
  try {
    const f = await api('/files/read?path=' + encodeURIComponent('craftmc-messages.txt'));
    custom = f.content || '';
  } catch {}
  main.innerHTML = `
    <h1 class="font-serif text-3xl mb-1">Join messages</h1>
    <p class="text-subtle mb-4">MOTD goes in <span class="kbd">server.properties</span>. Custom join/leave templates are saved as <span class="kbd">craftmc-messages.txt</span> in the server directory — wire them up to your join plugin (Essentials/CMI/etc).</p>
    <div class="bg-surface border border-edge rounded-xl p-4 animate-fade-in">
      <div class="field"><label>MOTD (server.properties)</label><input id="motd" value="${escapeHtml(motd)}"/></div>
      <div class="field"><label>Custom message file</label>
        <textarea id="custom" rows="12" spellcheck="false" placeholder="join: &amp;a{player} joined the realm&#10;leave: &amp;7{player} departed">${escapeHtml(custom)}</textarea></div>
      <button class="primary" id="save">Save</button>
    </div>`;
  $('#save').onclick = async () => {
    const newProps = setProp(r.content || '', 'motd', $('#motd').value);
    await api('/server-properties', { method:'PUT', body: JSON.stringify({ content: newProps }) });
    await api('/files/write', { method:'POST', body: JSON.stringify({ path: 'craftmc-messages.txt', content: $('#custom').value }) });
    toast('Saved','ok');
  };
};
function parseProps(text) {
  const out = {};
  text.split(/\r?\n/).forEach(l => {
    if (!l || l.startsWith('#')) return;
    const i = l.indexOf('='); if (i < 0) return;
    out[l.slice(0,i).trim()] = l.slice(i+1);
  });
  return out;
}
function setProp(text, key, val) {
  const lines = text.split(/\r?\n/);
  let found = false;
  for (let i=0;i<lines.length;i++) {
    if (lines[i].startsWith(key+'=')) { lines[i] = `${key}=${val}`; found=true; break; }
  }
  if (!found) lines.push(`${key}=${val}`);
  return lines.join('\n');
}

// ---------- File manager ----------
let fmPath = '';
views.files = async () => {
  main.innerHTML = `
    <h1 class="font-serif text-3xl mb-1">File manager</h1>
    <p class="text-subtle mb-4">Browse the server directory.</p>
    <div class="flex items-center gap-2 mb-3 flex-wrap">
      <button id="fmUp">Up</button>
      <button id="fmRoot">Root</button>
      <button id="fmRefresh">Refresh</button>
      <span class="flex-1"></span>
      <button id="fmMkdir">New folder</button>
      <label class="btn cursor-pointer" for="fmUpload">Upload</label>
      <input id="fmUpload" type="file" multiple class="hidden"/>
    </div>
    <div class="crumbs" id="fmCrumbs"></div>
    <div class="bg-surface border border-edge rounded-xl overflow-hidden animate-fade-in">
      <table><thead><tr><th>Name</th><th>Size</th><th>Modified</th><th></th></tr></thead><tbody id="fmRows"></tbody></table>
    </div>`;
  $('#fmUp').onclick = () => { fmPath = fmPath.split('/').filter(Boolean).slice(0,-1).join('/'); fmLoad(); };
  $('#fmRoot').onclick = () => { fmPath = ''; fmLoad(); };
  $('#fmRefresh').onclick = fmLoad;
  $('#fmMkdir').onclick = async () => {
    const n = prompt('Folder name'); if (!n) return;
    const path = (fmPath ? fmPath + '/' : '') + n;
    await api('/files/mkdir',{method:'POST', body: JSON.stringify({ path })});
    fmLoad();
  };
  $('#fmUpload').onchange = async (e) => {
    const fd = new FormData();
    for (const f of e.target.files) fd.append('file', f, f.name);
    await api('/files/upload?dir=' + encodeURIComponent(fmPath), { method:'POST', body: fd });
    e.target.value = '';
    fmLoad();
    toast('Uploaded','ok');
  };
  fmLoad();
};
async function fmLoad() {
  const data = await api('/files?path=' + encodeURIComponent(fmPath));
  const crumbs = $('#fmCrumbs');
  const parts = fmPath.split('/').filter(Boolean);
  crumbs.innerHTML = `<span data-p="">/</span>` + parts.map((p,i) => {
    const path = parts.slice(0,i+1).join('/');
    return `<span class="sep">/</span><span data-p="${escapeHtml(path)}">${escapeHtml(p)}</span>`;
  }).join('');
  $$('#fmCrumbs span[data-p]').forEach(s => s.onclick = () => { fmPath = s.dataset.p; fmLoad(); });
  const rows = $('#fmRows');
  rows.innerHTML = '';
  for (const e of data.entries) {
    const tr = document.createElement('tr');
    const sz = e.is_dir ? '' : humanSize(e.size);
    const mod = e.modified ? new Date(e.modified).toLocaleString() : '';
    tr.innerHTML = `
      <td>${e.is_dir ? '📁 ' : '📄 '}<span class="${e.is_dir?'dir':'file'}" style="cursor:${e.is_dir?'pointer':'default'};${e.is_dir?'color:var(--accent)':''}">${escapeHtml(e.name)}</span></td>
      <td class="muted">${sz}</td>
      <td class="muted">${mod}</td>
      <td style="text-align:right">
        ${!e.is_dir ? `<button data-act="edit">Edit</button>` : ''}
        ${!e.is_dir ? `<a class="btn" href="/api/files/download?path=${encodeURIComponent(e.path)}">Download</a>` : ''}
        <button class="danger" data-act="del">Delete</button>
      </td>`;
    if (e.is_dir) tr.querySelector('.dir').onclick = () => { fmPath = e.path.replace(/^\//,''); fmLoad(); };
    tr.querySelector('[data-act="del"]').onclick = async () => {
      if (!confirm(`Delete ${e.name}?`)) return;
      await api('/files/delete', { method:'POST', body: JSON.stringify({ path: e.path }) });
      fmLoad();
    };
    const editBtn = tr.querySelector('[data-act="edit"]');
    if (editBtn) editBtn.onclick = () => fmEdit(e.path);
    rows.appendChild(tr);
  }
}
async function fmEdit(path) {
  let r;
  try { r = await api('/files/read?path=' + encodeURIComponent(path)); } catch { return; }
  if (r.binary) { toast('Binary file — download instead','warn'); return; }
  main.innerHTML = `
    <h1 class="font-serif text-3xl mb-1">Edit: ${escapeHtml(path)}</h1>
    <div class="bg-surface border border-edge rounded-xl p-4 animate-fade-in">
      <textarea id="ed" rows="24" spellcheck="false">${escapeHtml(r.content||'')}</textarea>
      <div class="mt-2 flex gap-2">
        <button class="primary" id="save">Save</button>
        <button id="back">Back</button>
      </div>
    </div>`;
  $('#save').onclick = async () => {
    await api('/files/write', { method:'POST', body: JSON.stringify({ path, content: $('#ed').value }) });
    toast('Saved','ok');
  };
  $('#back').onclick = () => views.files();
}

// ---------- Plugins ----------
views.plugins = async () => {
  main.innerHTML = `
    <h1 class="font-serif text-3xl mb-1">Plugins</h1>
    <p class="text-subtle mb-4">Upload, install from URL, enable/disable, or remove plugin jars.</p>
    <div class="flex items-center gap-2 mb-3 flex-wrap">
      <label class="btn cursor-pointer" for="plUpload">Upload .jar</label>
      <input id="plUpload" type="file" accept=".jar" multiple class="hidden"/>
      <input id="plUrl" placeholder="Paste plugin URL (.jar)" class="!max-w-[420px]"/>
      <button id="plUrlBtn">Install URL</button>
      <label class="text-[12px] text-subtle"><input id="plReplace" type="checkbox" class="!w-auto"/> Replace if exists</label>
      <span class="flex-1"></span>
      <button id="plRefresh">Refresh</button>
    </div>
    <div class="bg-surface border border-edge rounded-xl overflow-hidden animate-fade-in">
      <table><thead><tr><th>Plugin</th><th>Size</th><th>Status</th><th></th></tr></thead><tbody id="plRows"></tbody></table>
    </div>`;
  $('#plUpload').onchange = async (e) => {
    const fd = new FormData();
    for (const f of e.target.files) fd.append('file', f, f.name);
    const replace = $('#plReplace').checked;
    await api('/plugins/upload?replace=' + replace, { method:'POST', body: fd });
    e.target.value=''; toast('Uploaded','ok'); plLoad();
  };
  $('#plUrlBtn').onclick = async () => {
    const url = $('#plUrl').value.trim(); if (!url) return;
    await api('/plugins/url', { method:'POST', body: JSON.stringify({ url, replace: $('#plReplace').checked }) });
    $('#plUrl').value=''; toast('Download started — see progress dock','ok');
  };
  $('#plRefresh').onclick = plLoad;
  // Refresh list automatically when an install_plugin task finishes.
  views._plOff = onTaskDone(t => { if (t.kind === 'install_plugin') plLoad(); });
  plLoad();
};
async function plLoad() {
  const r = await api('/plugins');
  const rows = $('#plRows'); rows.innerHTML = '';
  for (const p of r.plugins) {
    const tr = document.createElement('tr');
    tr.innerHTML = `
      <td>${escapeHtml(p.name)}</td>
      <td class="muted">${humanSize(p.size)}</td>
      <td><span class="px-2 py-0.5 rounded-full text-[11px] ${p.enabled?'bg-emerald-100 text-emerald-800':'bg-edge text-subtle'}">${p.enabled?'enabled':'disabled'}</span></td>
      <td style="text-align:right">
        <button data-act="toggle">${p.enabled?'Disable':'Enable'}</button>
        <button class="danger" data-act="del">Delete</button>
      </td>`;
    tr.querySelector('[data-act="toggle"]').onclick = async () => {
      await api('/plugins/toggle',{method:'POST', body: JSON.stringify({ name: p.name, enable: !p.enabled })});
      plLoad();
    };
    tr.querySelector('[data-act="del"]').onclick = async () => {
      if (!confirm(`Delete plugin ${p.name}?`)) return;
      await api('/plugins/delete',{method:'POST', body: JSON.stringify({ name: p.name })});
      plLoad();
    };
    rows.appendChild(tr);
  }
}

// Hook for completed tasks
const _doneHandlers = new Set();
function onTaskDone(fn) { _doneHandlers.add(fn); return () => _doneHandlers.delete(fn); }
const _seenDone = new Set();
setInterval(() => {
  for (const t of liveState.tasks.values()) {
    if (t.status === 'done' && !_seenDone.has(t.id)) {
      _seenDone.add(t.id);
      _doneHandlers.forEach(fn => fn(t));
    }
  }
}, 500);

// ---------- Plugin configs ----------
views['plugin-config'] = async () => {
  const r = await api('/plugins/configs');
  main.innerHTML = `
    <h1 class="font-serif text-3xl mb-1">Plugin configs</h1>
    <p class="text-subtle mb-4">Edit configuration files inside <span class="kbd">plugins/&lt;name&gt;/</span>.</p>
    <div class="grid gap-3" style="grid-template-columns:280px 1fr">
      <div class="bg-surface border border-edge rounded-xl overflow-auto max-h-[70vh] animate-fade-in" id="pcList"></div>
      <div id="pcEditor" class="bg-surface border border-edge rounded-xl p-4 animate-fade-in">Select a config to edit.</div>
    </div>`;
  const list = $('#pcList');
  for (const p of r.plugins) {
    const head = document.createElement('div');
    head.innerHTML = `<div class="item font-medium">${escapeHtml(p.plugin)}</div>`;
    list.appendChild(head);
    if (!p.configs.length) {
      const d = document.createElement('div');
      d.className='item muted'; d.style.fontSize='12px'; d.textContent='(no configs)';
      list.appendChild(d);
    }
    for (const f of p.configs) {
      const item = document.createElement('div');
      item.className = 'item'; item.style.paddingLeft = '24px';
      item.textContent = f;
      item.onclick = async () => {
        $$('#pcList .item').forEach(n => n.classList.remove('active'));
        item.classList.add('active');
        const c = await api(`/plugins/config?plugin=${encodeURIComponent(p.plugin)}&file=${encodeURIComponent(f)}`);
        $('#pcEditor').innerHTML = `
          <div class="muted mb-2">${escapeHtml(p.plugin)} / ${escapeHtml(f)}</div>
          <textarea id="pcText" rows="22" spellcheck="false">${escapeHtml(c.content||'')}</textarea>
          <div class="mt-2"><button class="primary" id="pcSave">Save</button></div>`;
        $('#pcSave').onclick = async () => {
          await api('/plugins/config', { method:'PUT', body: JSON.stringify({ plugin: p.plugin, file: f, content: $('#pcText').value }) });
          toast('Saved','ok');
        };
      };
      list.appendChild(item);
    }
  }
};

// ---------- Backups ----------
views.backups = async () => {
  main.innerHTML = `
    <h1 class="font-serif text-3xl mb-1">Backups</h1>
    <p class="text-subtle mb-4">Auto-backups run on the schedule set in Configuration. Worlds backed up: <span class="kbd">world</span>, <span class="kbd">world_nether</span>, <span class="kbd">world_the_end</span> (new layout: <span class="kbd">world/dimensions/minecraft/*</span>).</p>
    <div class="flex items-center gap-2 mb-3">
      <button class="primary" id="bkNow">Backup now</button>
      <span class="flex-1"></span>
      <button id="bkRefresh">Refresh</button>
    </div>
    <div class="bg-surface border border-edge rounded-xl overflow-hidden animate-fade-in">
      <table><thead><tr><th>Name</th><th>Size</th><th>Created</th><th></th></tr></thead><tbody id="bkRows"></tbody></table>
    </div>`;
  $('#bkNow').onclick = async () => { await api('/backups',{method:'POST'}); toast('Backup started','ok'); };
  $('#bkRefresh').onclick = bkLoad;
  bkLoad();
};
async function bkLoad() {
  const r = await api('/backups');
  const rows = $('#bkRows'); rows.innerHTML = '';
  for (const b of r.backups) {
    const tr = document.createElement('tr');
    tr.innerHTML = `
      <td>${escapeHtml(b.name)}</td>
      <td class="muted">${humanSize(b.size)}</td>
      <td class="muted">${new Date(b.created).toLocaleString()}</td>
      <td style="text-align:right">
        <a class="btn" href="/api/backups/${encodeURIComponent(b.name)}/download">Download</a>
        <button class="danger" data-act="del">Delete</button>
      </td>`;
    tr.querySelector('[data-act="del"]').onclick = async () => {
      if (!confirm('Delete backup?')) return;
      await api('/backups/' + encodeURIComponent(b.name), { method:'DELETE' });
      bkLoad();
    };
    rows.appendChild(tr);
  }
}

// ---------- Region editor ----------
views.region = async () => {
  main.innerHTML = `
    <h1 class="font-serif text-3xl mb-1">Region editor</h1>
    <p class="text-subtle mb-4">Inspect <span class="kbd">.mca</span> files. Stop the server before clearing chunks. Cleared chunks are zeroed in the location header — Minecraft regenerates them on next load.</p>
    <div class="row">
      <div class="field"><label>Dimension</label><select id="rgDim"></select></div>
      <div class="field"><label>Region file</label><select id="rgFile"></select></div>
    </div>
    <div id="rgChunks" class="bg-surface border border-edge rounded-xl p-4 mt-3 animate-fade-in">Pick a region file to view chunks.</div>`;
  const dims = (await api('/regions/dimensions')).dimensions;
  const dimSel = $('#rgDim');
  dimSel.innerHTML = dims.map(d => `<option value="${d.name}">${d.name}</option>`).join('') || '<option>(no worlds yet)</option>';
  dimSel.onchange = rgLoadFiles;
  if (dims.length) rgLoadFiles();
};
async function rgLoadFiles() {
  const dim = $('#rgDim').value;
  const r = await api('/regions/' + encodeURIComponent(dim));
  const sel = $('#rgFile');
  sel.innerHTML = r.files.map(f => {
    const just = f.path.split('/').pop();
    return `<option value="${escapeHtml(just)}">${escapeHtml(just)} (${f.chunk_count} chunks, ${humanSize(f.size)})</option>`;
  }).join('') || '<option>(none)</option>';
  sel.onchange = rgLoadChunks;
  if (r.files.length) rgLoadChunks();
}
async function rgLoadChunks() {
  const dim = $('#rgDim').value;
  const file = $('#rgFile').value;
  const r = await api(`/regions/${encodeURIComponent(dim)}/${encodeURIComponent(file)}/chunks`);
  const present = new Set(r.chunks.map(c => `${c.x},${c.z}`));
  let grid = '<div class="muted mb-2">Each cell is a chunk (32×32). Green = present. Click to clear.</div><div class="region-grid">';
  for (let z=0; z<32; z++) for (let x=0; x<32; x++) {
    const key = `${x},${z}`;
    grid += `<div class="region-cell ${present.has(key)?'exists':'empty'} transition-transform hover:scale-110" data-x="${x}" data-z="${z}" title="(${x},${z})"></div>`;
  }
  grid += '</div>';
  $('#rgChunks').innerHTML = grid;
  $$('#rgChunks .region-cell.exists').forEach(c => c.onclick = async () => {
    const x = +c.dataset.x, z = +c.dataset.z;
    if (!confirm(`Clear chunk (${x},${z}) in ${file}? Stop the server first.`)) return;
    try {
      await api(`/regions/${encodeURIComponent(dim)}/${encodeURIComponent(file)}/chunk/clear`, { method:'POST', body: JSON.stringify({ x, z }) });
      toast('Cleared','ok');
      rgLoadChunks();
    } catch {}
  });
}

// ---------- Logs ----------
views.logs = async () => {
  const r = await api('/logs');
  main.innerHTML = `
    <h1 class="font-serif text-3xl mb-1">Logs</h1>
    <p class="text-subtle mb-4">Persisted manager + server console logs.</p>
    <div class="bg-surface border border-edge rounded-xl overflow-hidden animate-fade-in">
      <table><thead><tr><th>Name</th><th>Size</th><th></th></tr></thead><tbody>
        ${r.logs.map(l => `<tr><td>${escapeHtml(l.name)}</td><td class="muted">${humanSize(l.size)}</td><td style="text-align:right"><a class="btn" href="/api/logs/${encodeURIComponent(l.name)}">Download</a></td></tr>`).join('')}
      </tbody></table>
    </div>`;
};

// ---------- Players ----------
views.players = async () => {
  main.innerHTML = `
    <h1 class="font-serif text-3xl mb-1">Players</h1>
    <p class="text-subtle mb-4">Live join/leave parsed from the server console. The list resets when the server stops.</p>
    <div class="flex items-center gap-2 mb-3">
      <span class="text-[12px] text-subtle"><span id="plCount">0</span> online</span>
      <span class="flex-1"></span>
      <input id="plBroadcast" placeholder="Broadcast message (sends as say)" class="!max-w-[420px]"/>
      <button id="plBroadcastBtn">Broadcast</button>
    </div>
    <div id="plList" class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-2"></div>`;

  $('#plBroadcastBtn').onclick = async () => {
    const t = $('#plBroadcast').value.trim(); if (!t) return;
    await api('/server/command', { method:'POST', body: JSON.stringify({ cmd: 'say ' + t }) }).catch(()=>{});
    $('#plBroadcast').value = '';
    toast('Broadcast sent','ok');
  };

  const render = (players) => {
    $('#plCount').textContent = players.length;
    const host = $('#plList');
    if (!players.length) {
      host.innerHTML = `<div class="bg-surface border border-edge rounded-xl p-6 text-center text-subtle italic animate-fade-in">No players online.</div>`;
      return;
    }
    host.innerHTML = players.map(p => `
      <div class="bg-surface border border-edge rounded-xl p-3 flex items-center gap-3 animate-slide-up hover:shadow-sm transition-shadow">
        <img src="https://mc-heads.net/avatar/${encodeURIComponent(p.name)}/40" class="w-10 h-10 rounded-md" alt=""/>
        <div class="flex-1 min-w-0">
          <div class="font-medium truncate">${escapeHtml(p.name)}</div>
          <div class="text-[11px] text-subtle">Joined ${new Date(p.joined_at).toLocaleTimeString()}</div>
        </div>
        <div class="flex flex-col gap-1">
          <button data-act="msg">DM</button>
          <button class="danger" data-act="kick">Kick</button>
        </div>
      </div>`).join('');
    $$('#plList [data-act="kick"]').forEach((b, i) => b.onclick = async () => {
      const name = players[i].name;
      const reason = prompt(`Kick ${name} — reason (optional)`, '') ?? '';
      try {
        await api('/players/kick', { method:'POST', body: JSON.stringify({ name, reason }) });
        toast(`Kicked ${name}`,'ok');
      } catch {}
    });
    $$('#plList [data-act="msg"]').forEach((b, i) => b.onclick = async () => {
      const name = players[i].name;
      const text = prompt(`Whisper ${name}:`, ''); if (!text) return;
      try {
        await api('/server/command', { method:'POST', body: JSON.stringify({ cmd: `tell ${name} ${text}` }) });
        toast('Sent','ok');
      } catch {}
    });
  };
  views._plOff2 = onPlayers(render);
  render(liveState.players);
};

// ---------- Marketplace ----------
views.marketplace = async () => {
  main.innerHTML = `
    <h1 class="font-serif text-3xl mb-1">Plugin marketplace</h1>
    <p class="text-subtle mb-4">Search Modrinth and Hangar (PaperMC). Installs go through the same task system as URL installs — track progress in the dock.</p>
    <div class="flex items-center gap-2 mb-3 flex-wrap">
      <input id="mkQ" placeholder="e.g. essentialsx, vault, dynmap" class="flex-1 min-w-[240px]"/>
      <select id="mkSrc" class="!w-auto">
        <option value="all">All sources</option>
        <option value="modrinth">Modrinth</option>
        <option value="hangar">Hangar</option>
      </select>
      <label class="text-[12px] text-subtle"><input id="mkReplace" type="checkbox" class="!w-auto"/> Replace if exists</label>
      <button class="primary" id="mkBtn">Search</button>
    </div>
    <div id="mkResults" class="grid grid-cols-1 md:grid-cols-2 gap-3"></div>`;

  const run = async () => {
    const q = $('#mkQ').value.trim(); if (!q) return;
    const src = $('#mkSrc').value;
    const host = $('#mkResults');
    host.innerHTML = Array.from({length:4}).map(() =>
      `<div class="bg-surface border border-edge rounded-xl p-4 h-28 relative overflow-hidden">
        <div class="absolute inset-0 animate-shimmer" style="background: linear-gradient(90deg, transparent, rgba(0,0,0,.05), transparent); background-size: 400px 100%;"></div>
      </div>`).join('');
    let r;
    try {
      r = await api(`/marketplace/search?q=${encodeURIComponent(q)}&source=${src}`);
    } catch { return; }
    if (!r.results.length) { host.innerHTML = `<div class="text-subtle italic">No results.</div>`; return; }
    host.innerHTML = r.results.map((p, i) => `
      <div class="bg-surface border border-edge rounded-xl p-4 flex gap-3 animate-slide-up hover:shadow-sm transition-shadow" style="animation-delay:${i*30}ms">
        <div class="shrink-0">
          ${p.icon_url
            ? `<img src="${escapeHtml(p.icon_url)}" class="w-14 h-14 rounded-md object-cover bg-edge" alt=""/>`
            : `<div class="w-14 h-14 rounded-md bg-edge flex items-center justify-center text-subtle text-xs">${p.source[0].toUpperCase()}</div>`}
        </div>
        <div class="flex-1 min-w-0">
          <div class="flex items-center gap-2">
            <a href="${escapeHtml(p.url)}" target="_blank" rel="noopener" class="font-medium truncate hover:text-accent">${escapeHtml(p.title)}</a>
            <span class="px-1.5 py-0.5 rounded text-[10px] uppercase tracking-wide ${p.source==='modrinth'?'bg-emerald-100 text-emerald-800':'bg-orange-100 text-orange-800'}">${p.source}</span>
          </div>
          <div class="text-[11px] text-subtle mb-1">by ${escapeHtml(p.author)} · ${p.downloads.toLocaleString()} downloads</div>
          <div class="text-[12px] text-subtle line-clamp-2">${escapeHtml(p.description||'')}</div>
          <div class="mt-2"><button class="primary" data-i="${i}">Install latest</button></div>
        </div>
      </div>`).join('');
    $$('#mkResults [data-i]').forEach(btn => btn.onclick = async () => {
      const i = +btn.dataset.i; const p = r.results[i];
      btn.disabled = true; btn.textContent = 'Queuing…';
      try {
        await api('/marketplace/install', { method:'POST', body: JSON.stringify({ source: p.source, slug: p.slug, replace: $('#mkReplace').checked }) });
        toast('Download started — see progress dock','ok');
        btn.textContent = 'Queued ✓';
      } catch {
        btn.disabled = false; btn.textContent = 'Install latest';
      }
    });
  };
  $('#mkBtn').onclick = run;
  $('#mkQ').addEventListener('keydown', e => { if (e.key === 'Enter') run(); });
  $('#mkQ').focus();
};

// ---------- Deal records (admin) ----------
views.deals = async () => {
  main.innerHTML = `
    <h1 class="font-serif text-3xl mb-1">Deal records</h1>
    <p class="text-subtle mb-4">All public deal records. Anyone can create one at <a href="/deals" class="text-accent" target="_blank">/deals</a>; signing happens in-game via the API.</p>
    <div class="flex items-center gap-2 mb-3">
      <a class="btn" href="/deals" target="_blank">Open public page</a>
      <span class="flex-1"></span>
      <button id="dlRefresh">Refresh</button>
    </div>
    <div class="bg-surface border border-edge rounded-xl overflow-hidden animate-fade-in">
      <table><thead><tr><th>Title</th><th>ID</th><th>Status</th><th>Parties</th><th>Created</th><th></th></tr></thead><tbody id="dlRows"></tbody></table>
    </div>`;
  $('#dlRefresh').onclick = dlLoad;
  dlLoad();
};
async function dlLoad() {
  const r = await api('/deals');
  const rows = $('#dlRows'); rows.innerHTML = '';
  for (const d of r.deals) {
    const status = d.status;
    const cls = ({signed:'bg-emerald-100 text-emerald-800',partial:'bg-yellow-100 text-yellow-800',rejected:'bg-rose-100 text-rose-800',pending:'bg-edge text-subtle'})[status] || 'bg-edge text-subtle';
    const tr = document.createElement('tr');
    tr.innerHTML = `
      <td class="font-medium">${escapeHtml(d.title)}</td>
      <td class="muted font-mono">${escapeHtml(d.id)}</td>
      <td><span class="px-2 py-0.5 rounded-full text-[11px] ${cls}">${status}</span></td>
      <td class="muted">${escapeHtml(d.parties.join(', '))}</td>
      <td class="muted">${new Date(d.created_at).toLocaleString()}</td>
      <td style="text-align:right">
        <a class="btn" href="/deals/${encodeURIComponent(d.id)}" target="_blank">View</a>
        <button class="danger" data-act="del">Delete</button>
      </td>`;
    tr.querySelector('[data-act="del"]').onclick = async () => {
      if (!confirm(`Delete deal ${d.id}?`)) return;
      await api('/deals/' + encodeURIComponent(d.id), { method:'DELETE' });
      dlLoad();
    };
    rows.appendChild(tr);
  }
  if (!r.deals.length) rows.innerHTML = `<tr><td colspan="6" class="muted italic" style="padding:18px">No deals yet.</td></tr>`;
}

// ---------- API docs ----------
views['api-docs'] = async () => {
  const origin = location.origin;
  const ex = (url) => `<div class="bg-paper border border-edge rounded-md px-3 py-2 font-mono text-[12px] flex items-center justify-between gap-2 mt-2">
    <span class="truncate">${escapeHtml(url)}</span>
    <button data-copy="${escapeHtml(url)}">Copy</button></div>`;
  const endpoint = (method, path, desc, examples=[]) => `
    <div class="bg-surface border border-edge rounded-xl p-4 animate-fade-in">
      <div class="flex items-center gap-2 mb-1">
        <span class="px-2 py-0.5 rounded text-[11px] font-mono ${method==='GET'?'bg-emerald-100 text-emerald-800':'bg-orange-100 text-orange-800'}">${method}</span>
        <span class="font-mono text-[13px]">${escapeHtml(path)}</span>
      </div>
      <div class="text-[13px] text-subtle">${desc}</div>
      ${examples.map(e => ex(origin + e)).join('')}
    </div>`;
  main.innerHTML = `
    <h1 class="font-serif text-3xl mb-1">API documentation</h1>
    <p class="text-subtle mb-4">Public Deal Sign API for in-game plugins. All <span class="kbd">/mcsapi/*</span> endpoints are unauthenticated and return JSON.</p>

    <div class="space-y-3">
      ${endpoint('GET', '/mcsapi/record/list', 'List all deal records.', ['/mcsapi/record/list'])}
      ${endpoint('GET', '/mcsapi/record/view?dealId=&lt;id&gt;', 'Fetch a single record with signatures, rejections, and current status.', ['/mcsapi/record/view?dealId=DEAL_ID'])}
      ${endpoint('POST', '/mcsapi/record/create', 'Create a new unsigned deal. Body: <code>{ "title": "...", "body": "...", "parties": ["Alice","Bob"], "by": "optional creator" }</code>.', [])}
      ${endpoint('GET', '/mcsapi/record/create', 'Convenience GET form for creating a deal.', ['/mcsapi/record/create?title=Trade&parties=Alice,Bob&body=Terms'])}
      ${endpoint('GET', '/mcsapi/record/approve?name=&lt;user&gt;&amp;dealId=&lt;id&gt;', 'Sign / approve a deal as the given player. Errors if the player is not a party, has already signed, or has rejected.', ['/mcsapi/record/approve?name=Alice&dealId=DEAL_ID'])}
      ${endpoint('GET', '/mcsapi/record/reject?name=&lt;user&gt;&amp;dealId=&lt;id&gt;&amp;reason=...', 'Reject a deal. The whole record becomes <span class="kbd">rejected</span>.', ['/mcsapi/record/reject?name=Alice&dealId=DEAL_ID&reason=changed%20mind'])}
    </div>

    <h2 class="font-serif text-2xl mt-8 mb-2">Status values</h2>
    <ul class="list-disc pl-6 text-[13px] text-subtle space-y-1">
      <li><span class="kbd">pending</span> — no one has signed yet.</li>
      <li><span class="kbd">partial</span> — some parties have signed; others haven't.</li>
      <li><span class="kbd">signed</span> — every listed party has signed.</li>
      <li><span class="kbd">rejected</span> — at least one party rejected.</li>
    </ul>

    <h2 class="font-serif text-2xl mt-8 mb-2">Plugin integration</h2>
    <div class="bg-surface border border-edge rounded-xl p-4">
      <p class="text-[13px] mb-2">A reference PaperMC plugin lives in the source tree under <span class="kbd">mcs_plugin/</span>. It exposes:</p>
      <ul class="list-disc pl-6 text-[13px] text-subtle space-y-1">
        <li><span class="kbd">/sign &lt;dealId&gt;</span> — calls <span class="kbd">/mcsapi/record/approve</span> with the player's username.</li>
        <li><span class="kbd">/reject &lt;dealId&gt; [reason...]</span> — calls <span class="kbd">/mcsapi/record/reject</span>.</li>
        <li><span class="kbd">/deal &lt;dealId&gt;</span> — calls <span class="kbd">/mcsapi/record/view</span> and prints the title, terms, and parties.</li>
        <li><span class="kbd">/deals</span> — lists pending/partial deals where the player is a party.</li>
      </ul>
      <p class="text-[13px] text-subtle mt-2">Configure the manager URL in <span class="kbd">plugins/DealSign/config.yml</span> after the first run.</p>
    </div>`;

  $$('#main [data-copy]').forEach(b => b.onclick = () => {
    navigator.clipboard.writeText(b.dataset.copy).then(() => toast('Copied','ok'));
  });
};

// ---------- Boot ----------
(async () => {
  try { await api('/me'); } catch { return; }
  connectTasksWs();
  const initial = location.hash.replace('#','') || 'dashboard';
  nav(views[initial] ? initial : 'dashboard');
})();
