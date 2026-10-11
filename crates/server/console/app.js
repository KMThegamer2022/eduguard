/* EduGuard web console — dependency-free SPA served by the server binary. */
"use strict";

const S = {
  token: localStorage.getItem("eg_token") || "",
  user: localStorage.getItem("eg_user") || "",
  route: (location.hash || "#/dashboard").slice(2),
  devices: [],
  stats: {},
  events: [],
  audit: [],
  selected: new Set(),
  policyTemplate: null,
  keywords: [],
  tokens: [],
  ws: null,
  poller: null,
};

const root = document.getElementById("root");

// ------------------------- utilities -------------------------
async function api(path, opts = {}) {
  const res = await fetch(path, {
    ...opts,
    headers: {
      Authorization: "Bearer " + S.token,
      ...(opts.body && !(opts.body instanceof FormData) ? { "Content-Type": "application/json" } : {}),
      ...(opts.headers || {}),
    },
  });
  if (res.status === 401 && !path.endsWith("/login")) {
    logout();
    throw new Error("unauthorized");
  }
  if (!res.ok) {
    let msg = res.statusText;
    try { msg = (await res.json()).error || msg; } catch (_) {}
    throw new Error(msg);
  }
  const ct = res.headers.get("content-type") || "";
  return ct.includes("json") ? res.json() : res;
}

function el(tag, attrs = {}, ...children) {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (k === "class") node.className = v;
    else if (k === "style") node.style.cssText = v;
    else if (k === "html") node.innerHTML = v;
    else if (k.startsWith("on")) node.addEventListener(k.slice(2), v);
    else if (v !== null && v !== undefined && v !== false) node.setAttribute(k, v === true ? "" : v);
  }
  for (const c of children.flat()) {
    if (c === null || c === undefined || c === false) continue;
    node.append(c.nodeType ? c : document.createTextNode(c));
  }
  return node;
}

function fmtTs(ts) {
  if (!ts) return "—";
  const d = new Date(ts * 1000);
  return d.toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit", second: "2-digit" });
}
function fmtDur(secs) {
  if (secs < 60) return secs + "s";
  if (secs < 3600) return Math.floor(secs / 60) + "m";
  if (secs < 86400) return Math.floor(secs / 3600) + "h";
  return Math.floor(secs / 86400) + "d";
}

function toast(title, detail, kind) {
  let wrap = document.querySelector(".toasts");
  if (!wrap) { wrap = el("div", { class: "toasts" }); document.body.append(wrap); }
  const t = el("div", { class: "toast " + (kind || "") }, el("div", { class: "t" }, title), detail ? el("div", { class: "d" }, detail) : null);
  wrap.append(t);
  setTimeout(() => t.remove(), 6000);
}

// Screen preview URL. CSS background-images / <img> can't send an
// Authorization header, so the session token rides in the query string
// (the server validates it against the session table like any credential).
function thumbUrl(id, ts) {
  return `/api/devices/${id}/thumbnail?ts=${ts || 0}&token=${encodeURIComponent(S.token)}`;
}

function modal(title, bodyNode, footNodes) {
  const bg = el("div", { class: "modal-bg", onclick: (e) => { if (e.target === bg) bg.remove(); } },
    el("div", { class: "modal" },
      el("h3", {}, title),
      el("div", { class: "body" }, bodyNode),
      el("div", { class: "foot" }, footNodes || [el("button", { class: "btn", onclick: () => bg.remove() }, "Close")])));
  document.body.append(bg);
  return bg;
}

function logout() {
  localStorage.removeItem("eg_token");
  localStorage.removeItem("eg_user");
  S.token = ""; S.user = "";
  if (S.ws) { try { S.ws.close(); } catch (_) {} }
  render();
}

const svg = (p, color) => `<svg viewBox="0 0 24 24" fill="none" stroke="${color || "currentColor"}" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" width="16" height="16">${p}</svg>`;

// ------------------------- login -------------------------
function loginView() {
  const user = el("input", { id: "u", placeholder: "admin", autocomplete: "username" });
  const pass = el("input", { id: "p", type: "password", placeholder: "••••••••", autocomplete: "current-password" });
  const err = el("div", { class: "muted", style: "color:var(--red);min-height:18px;font-size:12px" });
  const submit = async () => {
    try {
      const r = await api("/api/login", { method: "POST", body: JSON.stringify({ username: user.value, password: pass.value }) });
      S.token = r.token; S.user = r.username;
      localStorage.setItem("eg_token", r.token);
      localStorage.setItem("eg_user", r.username);
      render();
    } catch (e) { err.textContent = e.message; }
  };
  pass.addEventListener("keydown", (e) => { if (e.key === "Enter") submit(); });
  return el("div", { class: "login-wrap" },
    el("div", { class: "login-card" },
      el("div", { style: "display:flex;gap:10px;align-items:center;margin-bottom:14px" },
        el("div", { style: "width:34px;height:34px", html: `<svg viewBox="0 0 24 24" fill="none" stroke="#3fb950" stroke-width="2" width="34" height="34"><path d="M12 2l8 4v6c0 5-3.5 8.5-8 10-4.5-1.5-8-5-8-10V6l8-4z"/></svg>` }),
        el("div", {}, el("h1", {}, "EduGuard"), el("div", { class: "muted", style: "font-size:12px" }, "Safeguarding & classroom console"))),
      el("label", { class: "f" }, "Username"), user,
      el("label", { class: "f" }, "Password"), pass,
      el("div", { style: "margin-top:14px" }, el("button", { class: "btn primary", style: "width:100%", onclick: submit }, "Sign in")),
      err,
      el("div", { class: "muted", style: "font-size:11px;margin-top:14px" }, "On-premises deployment · first-run credentials are printed in the server log.")));
}

// ------------------------- shell -------------------------
const NAV = [
  ["dashboard", "Dashboard"],
  ["devices", "Devices"],
  ["alerts", "Alerts & Incidents"],
  ["activity", "Activity Log"],
  ["audit", "Staff Audit"],
  ["settings", "Settings"],
];

function shell(content) {
  const alertBadge = S.stats.alerts_24h || 0;
  const nav = NAV.map(([id, label]) =>
    el("button", {
      class: "navitem" + (S.route === id ? " active" : ""),
      onclick: () => { location.hash = "#/" + id; },
    }, label,
    id === "alerts" && alertBadge ? el("span", { class: "badge" }, String(alertBadge)) : null));
  return el("div", { class: "shell" },
    el("div", { class: "sidebar" },
      el("div", { class: "brand" },
        el("div", { html: `<svg viewBox="0 0 24 24" fill="none" stroke="#3fb950" stroke-width="2"><path d="M12 2l8 4v6c0 5-3.5 8.5-8 10-4.5-1.5-8-5-8-10V6l8-4z"/><path d="M9 12l2 2 4-4"/></svg>` }),
        el("div", {}, el("b", {}, "EduGuard"), el("small", {}, "console"))),
      nav,
      el("div", { class: "sidefoot" },
        el("div", {}, "Signed in as ", el("b", {}, S.user)),
        el("button", { class: "btn sm", style: "margin-top:8px", onclick: logout }, "Sign out"))),
    el("div", { class: "main" }, content));
}

// ------------------------- dashboard -------------------------
async function loadStats() { S.stats = await api("/api/stats"); }

function dashboardView() {
  const s = S.stats;
  const recent = S.events.filter((e) => e.kind === "alert" || e.kind === "raise_hand" || e.kind === "chat").slice(0, 8);
  return [
    el("div", { class: "cards" },
      card(s.devices_online ?? 0, "Devices online"),
      card(s.devices_total ?? 0, "Enrolled devices"),
      card(s.alerts_24h ?? 0, "Alerts (24h)"),
      card(s.devices_locked ?? 0, "Locked screens"),
      card(s.devices_blanked ?? 0, "Blanked screens")),
    el("div", { class: "panel" },
      el("h3", {}, "Latest safeguarding activity"),
      recent.length
        ? el("table", { class: "tbl" },
            el("tr", {}, el("th", {}, "Time"), el("th", {}, "Device"), el("th", {}, "Type"), el("th", {}, "Detail")),
            recent.map((e) => el("tr", {},
              el("td", { class: "mono" }, fmtTs(e.ts)),
              el("td", {}, e.device_name),
              el("td", {}, el("span", { class: "sev " + e.severity }, e.severity), " ", e.kind),
              el("td", {}, e.title))))
        : el("div", { class: "body muted" }, "No activity yet — enrol agents and they will appear here."),
    ),
  ];
}
const card = (v, l) => el("div", { class: "stat" }, el("div", { class: "v" }, String(v)), el("div", { class: "l" }, l));

// ------------------------- devices -------------------------
async function loadDevices() { S.devices = await api("/api/devices"); }

async function sendCommand(id, kind, extra = {}) {
  try {
    const r = await api(`/api/devices/${id}/command`, { method: "POST", body: JSON.stringify({ type: kind, ...extra }) });
    if (!r.delivered) toast("Queued (device offline)", `${kind} will run when the device reconnects`);
    return r;
  } catch (e) { toast("Command failed", e.message, "alert"); }
}

async function setPolicy(id, patch) {
  const dev = S.devices.find((d) => d.id === id);
  if (!dev) return;
  const next = { ...dev.policy, ...patch };
  try {
    await api(`/api/devices/${id}/policy`, { method: "PUT", body: JSON.stringify(next) });
    dev.policy = next;
    renderDevices();
  } catch (e) { toast("Policy failed", e.message, "alert"); }
}

function deviceCard(d) {
  const selected = S.selected.has(d.id);
  const thumb = el("div", {
    class: "thumb",
    "data-dev": d.id,
    onclick: () => deviceDetails(d),
    style: d.thumbnail_ts ? `background-image:url(${thumbUrl(d.id, d.thumbnail_ts)})` : "",
  }, d.thumbnail_ts ? "" : "no screenshot yet");
  return el("div", { class: "devcard" + (selected ? " sel" : "") },
    thumb,
    el("div", { class: "meta" },
      el("div", { class: "name" },
        el("span", {}, el("span", { class: "dot " + (d.online ? "on" : "off") }), d.name),
        el("input", {
          class: "check", type: "checkbox", checked: selected,
          onchange: (e) => { e.target.checked ? S.selected.add(d.id) : S.selected.delete(d.id); renderDevices(); },
        })),
      el("div", { class: "sub" }, `${d.os} ${d.os_version} · ${d.active_user || "no user"}`),
      el("div", { class: "meters" },
        el("span", {}, `CPU ${d.cpu_pct.toFixed(0)}%`),
        el("span", {}, `RAM ${(d.mem_used_mb / 1024).toFixed(1)}GB`),
        d.battery_pct !== null && d.battery_pct !== undefined ? el("span", {}, `🔋${d.battery_pct.toFixed(0)}%`) : null,
        d.screen_locked ? el("span", { style: "color:var(--yellow)" }, "locked") : null),
      el("div", { class: "btnrow" },
        el("button", { class: "btn sm" + (d.policy.locked ? " warn" : ""), onclick: () => setPolicy(d.id, { locked: !d.policy.locked }) }, d.policy.locked ? "Unlock" : "Lock"),
        el("button", { class: "btn sm" + (d.policy.blanked ? " warn" : ""), onclick: () => setPolicy(d.id, { blanked: !d.policy.blanked }) }, d.policy.blanked ? "Unblank" : "Blank"),
        el("button", { class: "btn sm", onclick: () => deviceDetails(d) }, "Details"))));
}

function deviceDetails(d) {
  const msg = el("input", { placeholder: "Message to student…" });
  const url = el("input", { placeholder: "https://…" });
  // Live screen view: refreshed every 2s while this modal is open.
  const live = el("div", {
    class: "thumb live",
    "data-dev": d.id,
    style: d.thumbnail_ts ? `background-image:url(${thumbUrl(d.id, d.thumbnail_ts)})` : "",
  }, d.thumbnail_ts ? "" : "requesting screen…");
  const bg = modal(`${d.name} — ${d.online ? "online" : "offline"}`,
    el("div", {},
      live,
      el("div", { class: "muted", style: "margin-bottom:10px" },
        `${d.os} ${d.os_version || "?"} (${d.arch || "unknown arch"}) · agent ${d.agent_version || "?"} · ${d.active_user || "no user"} · ${d.primary_ip || "no ip"} · last seen ${fmtTs(d.last_seen)}`),
      el("div", { class: "btnrow", style: "margin-bottom:12px" },
        el("button", { class: "btn sm", onclick: () => sendCommand(d.id, "screenshot", { max_width: 640 }) }, "Screenshot"),
        el("button", { class: "btn sm", onclick: () => sendCommand(d.id, "rescan_inventory") }, "Rescan inventory"),
        el("button", { class: "btn sm", onclick: () => setPolicy(d.id, { muted: !d.policy.muted }) }, d.policy.muted ? "Unmute" : "Mute"),
        el("button", { class: "btn sm", onclick: () => raiseHandDemo(d) }, "Raise hand ping"),
        el("button", { class: "btn sm", onclick: () => chatWith(d) }, "Chat…"),
        el("button", { class: "btn sm warn", onclick: () => confirmPower(d, "shutdown") }, "Shut down"),
        el("button", { class: "btn sm warn", onclick: () => confirmPower(d, "restart") }, "Restart"),
        el("button", { class: "btn sm warn", onclick: () => sendCommand(d.id, "logoff") }, "Log off")),
      el("label", { class: "f" }, "Show message on screen"), msg,
      el("button", { class: "btn sm", style: "margin:6px 0 12px", onclick: () => { sendCommand(d.id, "message", { text: msg.value, ttl_secs: 20 }); bg.remove(); } }, "Send message"),
      el("label", { class: "f" }, "Open URL on device"), url,
      el("button", { class: "btn sm", style: "margin:6px 0 12px", onclick: () => { sendCommand(d.id, "open_url", { url: url.value }); bg.remove(); } }, "Open URL"),
      el("label", { class: "f" }, "Send file to device"),
      el("input", { type: "file", onchange: (e) => pushFile(d.id, e.target.files[0]) }),
      el("div", { id: "inv-" + d.id, style: "margin-top:14px" })));
  // Live view: request a fresh capture every 2s while the modal is open; the
  // console WS "thumbnail" event repaints every [data-dev] node including this
  // one. Stops itself when the modal leaves the document.
  const refresh = async () => {
    if (!document.body.contains(bg)) { clearInterval(iv); return; }
    // S.devices is replaced by the periodic re-render; use the fresh row.
    const cur = S.devices.find((x) => x.id === d.id) || d;
    if (!cur.online) return;
    try { await api("/api/viewing", { method: "POST", body: JSON.stringify({ active: true }) }); } catch (_) {}
    await sendCommand(d.id, "screenshot", { max_width: 640 });
  };
  const iv = setInterval(refresh, 2000);
  refresh();
  // Honest feature-gating: if no capture lands, say why instead of spinning.
  setTimeout(() => {
    if (document.body.contains(live) && live.textContent === "requesting screen…") {
      live.textContent = d.online
        ? "no screen yet — agent may lack capture permission (macOS Screen Recording) or a graphical session"
        : "device offline — no live view";
    }
  }, 8000);
  api(`/api/devices/${d.id}/inventory`).then((inv) => {
    const host = bg.querySelector("#inv-" + d.id);
    if (!host) return;
    if (!inv || !inv.os) { host.append(el("div", { class: "muted" }, "No inventory reported yet.")); return; }
    host.append(
      el("h3", { style: "font-size:13px;margin:0 0 6px" }, "Inventory"),
      el("div", { class: "mono muted" }, `${inv.cpu || "?"} · ${inv.cpu_cores || "?"} cores · ${((inv.mem_total_mb || 0) / 1024).toFixed(0)} GB RAM`),
      el("table", { class: "tbl", style: "margin-top:6px" },
        el("tr", {}, el("th", {}, "Application"), el("th", {}, "Version")),
        (inv.installed_apps || []).slice(0, 40).map((a) => el("tr", {}, el("td", {}, a.name), el("td", { class: "mono" }, a.version)))));
  }).catch(() => {});
}

function raiseHandDemo(d) { toast("Raise-hand ping is student-initiated", "Students use the agent tray/overlay to raise hands"); }

function chatWith(d) {
  const input = el("input", { placeholder: "Message from teacher…" });
  const bg = modal("Chat — " + d.name, input, [
    el("button", { class: "btn", onclick: () => bg.remove() }, "Cancel"),
    el("button", { class: "btn primary", onclick: () => { sendCommand(d.id, "message", { text: input.value, ttl_secs: 30 }); bg.remove(); } }, "Send"),
  ]);
  input.focus();
}

function confirmPower(d, kind) {
  const bg = modal(`Really ${kind} ${d.name}?`, el("div", { class: "muted" }, "This affects a real machine. The action is recorded in the staff audit log."), [
    el("button", { class: "btn", onclick: () => bg.remove() }, "Cancel"),
    el("button", { class: "btn danger", onclick: () => { sendCommand(d.id, kind, { delay_secs: 5 }); bg.remove(); } }, "Yes, " + kind),
  ]);
}

async function pushFile(deviceId, file) {
  if (!file) return;
  const fd = new FormData();
  fd.append("device_id", deviceId);
  fd.append("filename", file.name);
  fd.append("file", file);
  try {
    const r = await api("/api/files", { method: "POST", body: fd });
    toast("File " + (r.delivered ? "sent" : "queued (offline)"), file.name);
  } catch (e) { toast("File send failed", e.message, "alert"); }
}

function devicesView() {
  const bulk = S.selected.size
    ? el("div", { class: "bulkbar" },
        el("span", { class: "muted" }, S.selected.size + " selected"),
        el("button", { class: "btn sm", onclick: () => bulkSet({ locked: true }) }, "Lock all"),
        el("button", { class: "btn sm", onclick: () => bulkSet({ locked: false }) }, "Unlock all"),
        el("button", { class: "btn sm", onclick: () => bulkSet({ blanked: true }) }, "Blank all"),
        el("button", { class: "btn sm", onclick: () => bulkSet({ blanked: false }) }, "Unblank all"),
        el("button", { class: "btn sm", onclick: () => bulkShot() }, "Screenshot all"),
        el("button", { class: "btn sm", onclick: () => { S.selected.clear(); renderDevices(); } }, "Clear"))
    : null;
  return [
    el("div", { class: "grid" }, S.devices.map(deviceCard)),
    bulk,
  ];
}

async function bulkSet(patch) {
  for (const id of S.selected) await setPolicy(id, patch);
  toast("Bulk policy applied", JSON.stringify(patch));
}
async function bulkShot() {
  for (const id of S.selected) await sendCommand(id, "screenshot", { max_width: 320 });
}

function renderDevices() {
  if (S.route !== "devices") return;
  const main = document.querySelector(".main");
  if (main) { main.innerHTML = ""; main.append(...devicesView()); }
}

// ------------------------- alerts -------------------------
async function loadEvents(kind, limit) {
  S.events = await api(`/api/events?limit=${limit || 200}` + (kind ? `&kind=${kind}` : ""));
}

function alertsView() {
  const rows = S.events.filter((e) => ["alert", "raise_hand", "chat"].includes(e.kind));
  return el("div", { class: "panel" },
    el("h3", {}, "Safeguarding feed"),
    rows.length
      ? el("table", { class: "tbl" },
          el("tr", {}, el("th", {}, "Time"), el("th", {}, "Device"), el("th", {}, "Severity"), el("th", {}, "Match / message"), el("th", {}, "Context"), el("th", {}, "")),
          rows.map((e) => {
            let ctx = "";
            try { const d = JSON.parse(e.detail); ctx = d.context || d.title || d.body || ""; } catch (_) { ctx = e.detail; }
            return el("tr", {},
              el("td", { class: "mono" }, fmtTs(e.ts)),
              el("td", {}, e.device_name),
              el("td", {}, el("span", { class: "sev " + e.severity }, e.severity)),
              el("td", {}, e.title),
              el("td", { class: "mono muted", style: "max-width:340px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap" }, ctx),
              el("td", {}, el("button", { class: "btn sm", onclick: async () => { await api(`/api/events/${e.id}/status`, { method: "PUT", body: JSON.stringify({ status: "acknowledged" }) }); toast("Incident acknowledged", e.title); loadEvents().then(render); } }, "Ack")));
          }))
      : el("div", { class: "body muted" }, "No alerts yet. Keyword detection reports here in real time."));
}

// ------------------------- activity -------------------------
function activityView() {
  const rows = S.events.filter((e) => e.kind === "activity");
  return el("div", { class: "panel" },
    el("h3", {}, "Usage logging — foreground applications"),
    rows.length
      ? el("table", { class: "tbl" },
          el("tr", {}, el("th", {}, "Time"), el("th", {}, "Device"), el("th", {}, "Application"), el("th", {}, "Window title")),
          rows.slice(0, 300).map((e) => {
            let app = e.title, title = "";
            try { const d = JSON.parse(e.detail); app = d.app; title = (d.title || "").slice(0, 120); } catch (_) {}
            return el("tr", {},
              el("td", { class: "mono" }, fmtTs(e.ts)),
              el("td", {}, e.device_name),
              el("td", {}, app),
              el("td", { class: "mono muted" }, title));
          }))
      : el("div", { class: "body muted" }, "No activity samples yet."));
}

// ------------------------- audit -------------------------
async function loadAudit() { S.audit = await api("/api/audit?limit=300"); }

function auditView() {
  return el("div", { class: "panel" },
    el("h3", {}, "Staff action audit trail"),
    S.audit.length
      ? el("table", { class: "tbl" },
          el("tr", {}, el("th", {}, "Time"), el("th", {}, "Staff"), el("th", {}, "Action"), el("th", {}, "Target"), el("th", {}, "Detail")),
          S.audit.map((a) => el("tr", {},
            el("td", { class: "mono" }, fmtTs(a.ts)),
            el("td", {}, a.actor),
            el("td", { class: "mono" }, a.action),
            el("td", {}, a.target),
            el("td", { class: "mono muted" }, a.detail))))
      : el("div", { class: "body muted" }, "Audit entries appear as staff act."));
}

// ------------------------- settings -------------------------
async function loadSettings() {
  [S.policyTemplate, S.keywords, S.tokens] = await Promise.all([
    api("/api/policy"), api("/api/keywords"), api("/api/enrollment-tokens"),
  ]);
}

function settingsView() {
  const p = S.policyTemplate || {};
  const school = el("input", { value: p.school_name || "" });
  const banner = el("input", { type: "checkbox", checked: !!p.monitor_banner, style: "width:auto" });
  const interval = el("input", { type: "number", value: p.activity_interval_secs ?? 30, min: 0, max: 3600 });
  const captureInd = el("input", { type: "checkbox", checked: !!p.capture_indicator, style: "width:auto" });

  const kwHost = el("div", {});
  const kwList = S.keywords.slice();
  const drawKw = () => {
    kwHost.innerHTML = "";
    kwList.forEach((k, i) => {
      const del = el("button", { class: "btn sm danger", onclick: () => { kwList.splice(i, 1); drawKw(); } }, "✕");
      kwHost.append(el("div", { class: "kwrow" },
        el("input", { value: k.keyword, oninput: (e) => (k.keyword = e.target.value) }),
        el("input", { value: k.category, oninput: (e) => (k.category = e.target.value) }),
        el("select", { onchange: (e) => (k.severity = e.target.value) },
          ["info", "low", "medium", "high", "critical"].map((s) => el("option", { value: s, selected: k.severity === s }, s))),
        el("input", { value: k.note || "", placeholder: "note", oninput: (e) => (k.note = e.target.value) }),
        del));
    });
  };
  drawKw();

  const tokHost = el("div", {});
  const drawTok = () => {
    tokHost.innerHTML = "";
    if (!S.tokens.length) tokHost.append(el("div", { class: "muted" }, "No enrollment tokens yet."));
    tokHost.append(el("table", { class: "tbl" },
      el("tr", {}, el("th", {}, "Token"), el("th", {}, "Label"), el("th", {}, "Used"), el("th", {}, "")),
      S.tokens.map((t) => el("tr", {},
        el("td", { class: "mono" }, t.token),
        el("td", {}, t.label),
        el("td", {}, String(t.used_count)),
        el("td", {}, el("button", { class: "btn sm danger", onclick: async () => { await api("/api/enrollment-tokens/" + t.token, { method: "DELETE" }); S.tokens = S.tokens.filter((x) => x.token !== t.token); drawTok(); } }, "Revoke"))))));
  };
  drawTok();

  return [
    el("div", { class: "panel" }, el("h3", {}, "Enrollment"),
      el("div", { class: "body" },
        tokHost,
        el("div", { class: "btnrow", style: "margin-top:10px" },
          el("button", { class: "btn primary", onclick: async () => {
            const r = await api("/api/enrollment-tokens", { method: "POST", body: JSON.stringify({ label: "lab" }) });
            S.tokens.unshift({ token: r.token, label: "lab", used_count: 0 });
            drawTok();
            toast("Enrollment token created", "Use with: eduguard-agent enroll --server <url> --token " + r.token);
          } }, "Create enrollment token")))),
    el("div", { class: "panel" }, el("h3", {}, "Default policy (new devices)"),
      el("div", { class: "body" },
        el("label", { class: "f" }, "School name (student-facing banners)"), school,
        el("div", { style: "display:flex;gap:18px;margin-top:12px;align-items:center" },
          el("label", {}, banner, " Show monitoring transparency banner"),
          el("label", {}, captureInd, " Show capture-evidence indicator")),
        el("label", { class: "f" }, "Foreground-app sampling interval (seconds, 0 = off)"), interval,
        el("div", { style: "margin-top:12px" },
          el("button", { class: "btn primary", onclick: async () => {
            const next = { ...S.policyTemplate, school_name: school.value, monitor_banner: banner.checked, capture_indicator: captureInd.checked, activity_interval_secs: Number(interval.value) };
            await api("/api/policy", { method: "PUT", body: JSON.stringify(next) });
            S.policyTemplate = next;
            toast("Policy template saved");
          } }, "Save defaults")))),
    el("div", { class: "panel" }, el("h3", {}, `Keyword library (${kwList.length} rules)`),
      el("div", { class: "body" },
        kwHost,
        el("div", { class: "btnrow", style: "margin-top:10px" },
          el("button", { class: "btn", onclick: () => { kwList.push({ keyword: "", category: "custom", severity: "medium", note: "" }); drawKw(); } }, "Add rule"),
          el("button", { class: "btn primary", onclick: async () => {
            await api("/api/keywords", { method: "PUT", body: JSON.stringify(kwList.filter((k) => k.keyword)) });
            S.keywords = kwList.filter((k) => k.keyword);
            toast("Keyword library saved", "Pushed to all online agents");
          } }, "Save library")))),
  ];
}

// ------------------------- render + polling -------------------------
async function render() {
  if (!S.token) { root.innerHTML = ""; root.append(loginView()); return; }
  try {
    await Promise.all([
      loadStats(),
      loadDevices(),
      loadEvents(null, 200),
      S.route === "audit" ? loadAudit() : Promise.resolve(),
      S.route === "settings" ? loadSettings() : Promise.resolve(),
    ]);
  } catch (e) { return; }
  let content;
  switch (S.route) {
    case "devices": content = devicesView(); break;
    case "alerts": content = alertsView(); break;
    case "activity": content = activityView(); break;
    case "audit": content = auditView(); break;
    case "settings": content = settingsView(); break;
    default: content = dashboardView();
  }
  root.innerHTML = "";
  root.append(shell(content));
}

function connectWs() {
  if (!S.token) return;
  try { if (S.ws) S.ws.close(); } catch (_) {}
  const proto = location.protocol === "https:" ? "wss:" : "ws:";
  const ws = new WebSocket(`${proto}//${location.host}/ws/console?token=${encodeURIComponent(S.token)}`);
  ws.onmessage = (evt) => {
    let msg; try { msg = JSON.parse(evt.data); } catch (_) { return; }
    if (msg.type === "alert") {
      const p = msg.payload;
      toast(`Safeguarding alert — ${p.device_name}`, p.title, "alert");
      if (S.route === "alerts" || S.route === "dashboard") render();
      else loadStats();
    } else if (msg.type === "thumbnail") {
      const { device_id, ts } = msg.payload;
      document.querySelectorAll(`[data-dev="${device_id}"]`).forEach((node) => {
        node.style.backgroundImage = `url(${thumbUrl(device_id, ts)})`;
        node.textContent = "";
      });
      const d = S.devices.find((x) => x.id === device_id);
      if (d) d.thumbnail_ts = ts;
    } else if (msg.type === "raise_hand") {
      toast(`✋ ${msg.payload.device_name} raised a hand`, msg.payload.note || "", "alert");
    } else if (msg.type === "policy") {
      const d = S.devices.find((x) => x.id === msg.payload.device_id);
      if (d) { d.policy = msg.payload.policy; if (S.route === "devices") renderDevices(); }
    }
  };
  ws.onclose = () => { if (S.token) setTimeout(connectWs, 4000); };
  S.ws = ws;
}

window.addEventListener("hashchange", () => {
  S.route = (location.hash || "#/dashboard").slice(2);
  render();
});

(async function boot() {
  render();
  if (!S.token) return;
  connectWs();
  // Request live thumbnails every 2s while the device grid is on screen.
  setInterval(async () => {
    if (document.hidden) return;
    if (S.route === "devices" && S.devices.length) {
      try { await api("/api/viewing", { method: "POST", body: JSON.stringify({ active: true }) }); } catch (_) {}
      for (const d of S.devices) {
        if (!d.online) continue;
        await sendCommand(d.id, "screenshot", { max_width: 320 });
      }
    }
  }, 2000);
  setInterval(() => { if (!document.hidden) render(); }, 8000);
})();
