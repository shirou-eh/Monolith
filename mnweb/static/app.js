// Monolith OS — control panel front-end (vanilla, no build step)
const $ = (sel) => document.querySelector(sel);
const $$ = (sel) => Array.from(document.querySelectorAll(sel));

const cpuHistory = []; // last N samples (0..100)
const HISTORY_LEN = 60;

const PAGE_META = {
  overview:   { title: "Overview",   sub: "Live system snapshot · refreshes every 5s" },
  services:   { title: "Services",   sub: "Systemd units loaded on this host" },
  containers: { title: "Containers", sub: "Docker / Podman unified view" },
  disks:      { title: "Disks",      sub: "Block devices and SMART health" },
  cluster:    { title: "Cluster",    sub: "Multi-node etcd / k3s configuration" },
  templates:  { title: "Templates",  sub: "One-command application stacks" },
  logs:       { title: "Logs",       sub: "Recent journalctl tail" },
};

async function api(path) {
  const r = await fetch(path, { headers: { Accept: "application/json" } });
  if (!r.ok) throw new Error(`${path} → ${r.status}`);
  return r.json();
}

function setActiveTab(name) {
  if (!PAGE_META[name]) name = "overview";
  $$(".nav-item").forEach((a) => a.classList.toggle("is-active", a.dataset.tab === name));
  $$("section.tab").forEach((s) => s.classList.toggle("is-active", s.id === `tab-${name}`));
  const meta = PAGE_META[name];
  $("#page-title").textContent = meta.title;
  $("#page-sub").textContent = meta.sub;
  if (location.hash !== `#${name}`) history.replaceState(null, "", `#${name}`);
  const fn = tabLoaders[name];
  if (fn) fn();
}

document.addEventListener("DOMContentLoaded", () => {
  $$(".nav-item").forEach((a) =>
    a.addEventListener("click", (e) => {
      e.preventDefault();
      setActiveTab(a.dataset.tab);
    })
  );
  // Honor initial hash
  const initial = (location.hash || "#overview").replace(/^#/, "");
  setActiveTab(initial);

  // Filter results in current visible table when typing in search.
  const search = $("#search-input");
  if (search) {
    search.addEventListener("input", (e) => {
      const q = e.target.value.trim().toLowerCase();
      $$("section.tab.is-active table tbody tr").forEach((tr) => {
        if (!q) { tr.style.display = ""; return; }
        tr.style.display = tr.textContent.toLowerCase().includes(q) ? "" : "none";
      });
    });
  }

  refreshOverview();
  setInterval(refreshOverview, 5000);
  // Quick-glance counters (services / containers / disks / templates)
  // change rarely, so refresh them six times slower than the live
  // metrics. Saves ~80 % of the otherwise constant API chatter.
  refreshQuickGlance();
  setInterval(refreshQuickGlance, 30000);
});

function fmtBytes(bytes) {
  if (bytes === undefined || bytes === null) return "—";
  if (!bytes) return "0 B";
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  let v = bytes, u = 0;
  while (v >= 1024 && u < units.length - 1) { v /= 1024; u++; }
  return `${v.toFixed(v < 10 ? 2 : 1)} ${units[u]}`;
}

function fmtUptime(secs) {
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  const m = Math.floor((secs % 3600) / 60);
  if (d > 0) return `${d}d ${h}h ${m}m`;
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m`;
}

function setMeter(el, pct) {
  el.classList.remove("warn", "bad");
  if (pct > 90) el.classList.add("bad");
  else if (pct > 75) el.classList.add("warn");
  el.querySelector("span").style.width = `${Math.min(100, Math.max(0, pct)).toFixed(1)}%`;
}

function setHealth(state, label) {
  const pill = $("#health-pill");
  if (!pill) return;
  pill.querySelector(".dot").className = `dot dot-${state}`;
  pill.querySelector(".health-label").textContent = label;
}

function pushSpark(value) {
  cpuHistory.push(value);
  if (cpuHistory.length > HISTORY_LEN) cpuHistory.shift();
  renderSpark();
}

function renderSpark() {
  const svg = $("#cpu-spark");
  if (!svg) return;
  const W = 200, H = 50, P = 1;
  if (cpuHistory.length === 0) return;
  const n = cpuHistory.length;
  const stepX = (W - P * 2) / Math.max(1, HISTORY_LEN - 1);
  const startX = P + (HISTORY_LEN - n) * stepX;
  const points = cpuHistory.map((v, i) => {
    const x = startX + i * stepX;
    const y = P + (1 - Math.min(100, Math.max(0, v)) / 100) * (H - P * 2);
    return [x, y];
  });
  const linePath = points.map((p, i) => `${i === 0 ? "M" : "L"}${p[0].toFixed(2)},${p[1].toFixed(2)}`).join(" ");
  const fillPath = `${linePath} L${points[points.length - 1][0].toFixed(2)},${H} L${points[0][0].toFixed(2)},${H} Z`;
  svg.querySelector(".spark-line").setAttribute("d", linePath);
  svg.querySelector(".spark-fill").setAttribute("d", fillPath);
}

async function refreshOverview() {
  try {
    const data = await api("/api/overview");
    const dl = $("#sysinfo");
    dl.innerHTML = "";
    const rows = [
      ["Hostname",     data.hostname],
      ["OS",           data.os],
      ["Kernel",       data.kernel],
      ["Architecture", data.arch],
      ["CPU",          `${data.cpu.brand} · ${data.cpu.cores} cores`],
      ["RAM",          `${fmtBytes(data.memory.total)}`],
    ];
    rows.forEach(([k, v]) => {
      const dt = document.createElement("dt");
      const dd = document.createElement("dd");
      dt.textContent = k;
      dd.textContent = v;
      dl.appendChild(dt); dl.appendChild(dd);
    });

    const cpuPct = data.cpu.usage_pct || 0;
    $("#cpu-big").textContent = cpuPct.toFixed(1);
    $("#cpu-sub").textContent = `${data.cpu.cores} cores · ${data.cpu.brand}`;
    setMeter($("#cpu-meter"), cpuPct);
    pushSpark(cpuPct);

    const memPct =
      data.memory.total > 0
        ? (data.memory.used / data.memory.total) * 100
        : 0;
    $("#mem-big").textContent = memPct.toFixed(1);
    setMeter($("#mem-meter"), memPct);
    $("#mem-caption").textContent = `${fmtBytes(data.memory.used)} of ${fmtBytes(data.memory.total)} used`;
    $("#mem-sub").textContent = "RAM utilization";

    const swapPct = data.memory.swap_total > 0
      ? (data.memory.swap_used / data.memory.swap_total) * 100
      : 0;
    setMeter($("#swap-meter"), swapPct);
    $("#swap-caption").textContent = data.memory.swap_total > 0
      ? `${fmtBytes(data.memory.swap_used)} of ${fmtBytes(data.memory.swap_total)} swap`
      : "no swap configured";

    $("#uptime-big").textContent = fmtUptime(data.uptime_seconds);
    $("#hostname-caption").textContent = data.hostname;
    $("#load1").textContent  = (data.load_avg[0] || 0).toFixed(2);
    $("#load5").textContent  = (data.load_avg[1] || 0).toFixed(2);
    $("#load15").textContent = (data.load_avg[2] || 0).toFixed(2);

    $("#user-host").textContent = data.hostname;
    $("#version-tag").textContent = `mnweb v${data.version}`;
    $("#codename").textContent = `v${data.version} · ${data.codename}`;

    // Health pill
    if (cpuPct > 92 || memPct > 92) setHealth("bad", "Resource pressure");
    else if (cpuPct > 80 || memPct > 80) setHealth("warn", "Elevated load");
    else setHealth("good", "All systems nominal");
  } catch (e) {
    setHealth("bad", "API unreachable");
    console.error(e);
  }
}

async function refreshQuickGlance() {
  // Best-effort fan-out to populate the quick-glance counters on Overview.
  const set = (id, v, sub) => {
    const el = $(id);
    if (el) el.textContent = v;
    if (sub) {
      const subEl = $(`${id}-sub`);
      if (subEl) subEl.textContent = sub;
    }
  };
  Promise.allSettled([
    api("/api/services"),
    api("/api/containers"),
    api("/api/disks"),
    api("/api/templates"),
  ]).then(([svcs, ctrs, disks, tpls]) => {
    if (svcs.status === "fulfilled") {
      const arr = svcs.value;
      const active = arr.filter((s) => s.active === "active").length;
      set("#qg-services", arr.length, `${active} active`);
    } else {
      set("#qg-services", "—", "unavailable");
    }
    if (ctrs.status === "fulfilled") {
      const arr = ctrs.value;
      const running = arr.filter((c) => /running|up/i.test(c.status)).length;
      set("#qg-containers", arr.length, `${running} running`);
    } else {
      set("#qg-containers", "—", "unavailable");
    }
    if (disks.status === "fulfilled") {
      set("#qg-disks", disks.value.length, "attached");
    } else {
      set("#qg-disks", "—", "unavailable");
    }
    if (tpls.status === "fulfilled") {
      set("#qg-templates", tpls.value.length, "available");
    } else {
      set("#qg-templates", "—", "unavailable");
    }
  });
}

const tabLoaders = {
  overview: refreshOverview,
  services: async () => {
    const tbody = $("#services-table tbody");
    tbody.innerHTML = `<tr class="skeleton-row"><td colspan="3"><span class="skeleton"></span></td></tr>`;
    try {
      const services = await api("/api/services");
      if (!services.length) {
        tbody.innerHTML = `<tr><td colspan='3' class="empty">No services reporting yet.</td></tr>`;
        return;
      }
      tbody.innerHTML = services
        .map((s) => `<tr>
          <td>${escapeHtml(s.name)}</td>
          <td>${tag(s.load)}</td>
          <td>${tag(s.active, s.active === "active" ? "good" : "warn")}</td>
        </tr>`)
        .join("");
    } catch (e) {
      tbody.innerHTML = `<tr><td colspan='3' class="empty">${escapeHtml(e.message)}</td></tr>`;
    }
  },
  containers: async () => {
    const tbody = $("#containers-table tbody");
    tbody.innerHTML = `<tr class="skeleton-row"><td colspan="3"><span class="skeleton"></span></td></tr>`;
    try {
      const cs = await api("/api/containers");
      tbody.innerHTML = cs.length
        ? cs
            .map((c) => {
              const cls = /running|up/i.test(c.status) ? "good" : /paused|exited|dead/i.test(c.status) ? "bad" : "warn";
              return `<tr>
                <td>${escapeHtml(c.name)}</td>
                <td>${escapeHtml(c.image)}</td>
                <td>${tag(c.status, cls)}</td>
              </tr>`;
            })
            .join("")
        : `<tr><td colspan='3' class="empty">No containers running.</td></tr>`;
    } catch (e) {
      tbody.innerHTML = `<tr><td colspan='3' class="empty">${escapeHtml(e.message)}</td></tr>`;
    }
  },
  disks: async () => {
    const tbody = $("#disks-table tbody");
    tbody.innerHTML = `<tr class="skeleton-row"><td colspan="4"><span class="skeleton"></span></td></tr>`;
    try {
      const disks = await api("/api/disks");
      tbody.innerHTML = disks.length
        ? disks
            .map((d) => `<tr>
              <td>${escapeHtml(d.name)}</td>
              <td>${fmtBytes(d.size)}</td>
              <td>${escapeHtml(d.mount || "—")}</td>
              <td>${escapeHtml(d.fstype || "—")}</td>
            </tr>`)
            .join("")
        : `<tr><td colspan='4' class="empty">No disks detected.</td></tr>`;
    } catch (e) {
      tbody.innerHTML = `<tr><td colspan='4' class="empty">${escapeHtml(e.message)}</td></tr>`;
    }
  },
  cluster: async () => {
    const pre = $("#cluster-info");
    try {
      const c = await api("/api/cluster");
      pre.textContent = c.config || "Not in a cluster.";
    } catch (e) {
      pre.textContent = e.message;
    }
  },
  templates: async () => {
    const wrap = $("#templates-list");
    try {
      const ts = await api("/api/templates");
      if (!ts.length) {
        wrap.innerHTML = `<div class="caption">No templates indexed.</div>`;
        return;
      }
      wrap.innerHTML = ts
        .map((t) => `<div class="template">
          <div class="template-name">
            ${escapeHtml(t.name)}
            <span class="tag template-cat">${escapeHtml(t.category)}</span>
          </div>
          <div class="template-desc">${escapeHtml(t.description || "")}</div>
        </div>`)
        .join("");
    } catch (e) {
      wrap.innerHTML = `<div class="caption">${escapeHtml(e.message)}</div>`;
    }
  },
  logs: async () => {
    const pre = $("#logs-pre");
    try {
      const l = await api("/api/logs");
      pre.textContent = (l.lines || []).join("\n") || "(no log lines)";
    } catch (e) {
      pre.textContent = e.message;
    }
  },
};

function tag(text, cls) {
  return `<span class="tag${cls ? " tag-" + cls : ""}">${escapeHtml(text)}</span>`;
}

function escapeHtml(s) {
  if (s === null || s === undefined) return "";
  return String(s)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}
