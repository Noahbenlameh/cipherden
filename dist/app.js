const invoke = window.__TAURI__.core.invoke;
const dialog = window.__TAURI__.dialog;

// Tauri's macOS webview does not implement the native window.prompt()
// dialog (it silently returns null without ever showing anything), so we
// can't rely on window.prompt/confirm anywhere in this app. These two
// helpers are our own in-app replacements, styled to match the rest of
// the UI.
function showPromptModal(title, { password = false, placeholder = "" } = {}) {
  return new Promise((resolve) => {
    const modalEl = document.getElementById("prompt-modal");
    const titleEl = document.getElementById("prompt-modal-title");
    const inputEl = document.getElementById("prompt-modal-input");
    const okBtn = document.getElementById("prompt-modal-ok");
    const cancelBtn = document.getElementById("prompt-modal-cancel");

    titleEl.textContent = title;
    inputEl.type = password ? "password" : "text";
    inputEl.placeholder = placeholder;
    inputEl.value = "";
    modalEl.classList.remove("hidden");
    setTimeout(() => inputEl.focus(), 0);

    function cleanup(result) {
      modalEl.classList.add("hidden");
      okBtn.removeEventListener("click", onOk);
      cancelBtn.removeEventListener("click", onCancel);
      inputEl.removeEventListener("keydown", onKeydown);
      resolve(result);
    }
    function onOk() { cleanup(inputEl.value.trim() || null); }
    function onCancel() { cleanup(null); }
    function onKeydown(e) {
      if (e.key === "Enter") onOk();
      if (e.key === "Escape") onCancel();
    }
    okBtn.addEventListener("click", onOk);
    cancelBtn.addEventListener("click", onCancel);
    inputEl.addEventListener("keydown", onKeydown);
  });
}

function showConfirmModal(message) {
  return new Promise((resolve) => {
    const modalEl = document.getElementById("confirm-modal");
    const messageEl = document.getElementById("confirm-modal-message");
    const okBtn = document.getElementById("confirm-modal-ok");
    const cancelBtn = document.getElementById("confirm-modal-cancel");

    messageEl.textContent = message;
    modalEl.classList.remove("hidden");

    function cleanup(result) {
      modalEl.classList.add("hidden");
      okBtn.removeEventListener("click", onOk);
      cancelBtn.removeEventListener("click", onCancel);
      resolve(result);
    }
    function onOk() { cleanup(true); }
    function onCancel() { cleanup(false); }
    okBtn.addEventListener("click", onOk);
    cancelBtn.addEventListener("click", onCancel);
  });
}

function escapeHtml(s) {
  return String(s ?? "").replace(/[&<>"']/g, (c) => ({
    "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
  }[c]));
}

// --- Theme (Cyberpunk / Signal) ---------------------------------------------
//
// Cyberpunk is the original theme and is left byte-for-byte behaviour-wise:
// every themed code path below only activates additional behaviour when
// Signal is selected, it never changes what Cyberpunk does. The choice is a
// pure UI preference stored in localStorage — no vault data, no IPC command,
// no network access is involved.

function isSignalTheme() {
  return document.documentElement.dataset.theme === "signal";
}

const ZONE_EMOJI = { accounts: "🔑", files: "🗂", seeds: "🌱", ledger: "💰" };

// Custom line-art glyphs for the Signal theme: each keeps a small "node" dot
// (the same particle motif as the ambient network) that gets a slow pulse
// via the .cd-node CSS animation, so the zone grid reads as quietly alive.
const ZONE_ICON_SVGS = {
  accounts:
    '<svg class="zone-icon-svg zi-accounts" viewBox="0 0 28 28" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round">' +
    '<circle cx="9" cy="14" r="5.5"/><circle class="cd-node" cx="9" cy="14" r="1.3" fill="currentColor" stroke="none"/>' +
    '<path d="M14 14h9M19 14v3M22.5 14v2"/></svg>',
  files:
    '<svg class="zone-icon-svg zi-files" viewBox="0 0 28 28" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round" stroke-linecap="round">' +
    '<path d="M3.5 8.5c0-1 .8-1.8 1.8-1.8h5l2 2.4h10.4c1 0 1.8.8 1.8 1.8v9c0 1-.8 1.8-1.8 1.8H5.3c-1 0-1.8-.8-1.8-1.8V8.5z"/>' +
    '<circle class="cd-node" cx="21" cy="9" r="1.3" fill="currentColor" stroke="none"/></svg>',
  seeds:
    '<svg class="zone-icon-svg zi-seeds" viewBox="0 0 28 28" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round">' +
    '<path d="M14 22V13M14 13c0-3.5 2.5-6 6-6M14 13c0-3.5-2.5-6-6-6"/>' +
    '<circle cx="14" cy="22" r="1.4" fill="currentColor" stroke="none"/>' +
    '<circle class="cd-node" cx="20" cy="7" r="1.6" fill="currentColor" stroke="none"/>' +
    '<circle class="cd-node" cx="8" cy="7" r="1.6" fill="currentColor" stroke="none"/></svg>',
  ledger:
    '<svg class="zone-icon-svg zi-ledger" viewBox="0 0 28 28" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">' +
    '<path d="M14 5v16M6 9h16M6 9l-2.5 6M6 9l2.5 6M22 9l-2.5 6M22 9l2.5 6M18.5 21h-9"/>' +
    '<circle class="cd-node" cx="14" cy="5" r="1.4" fill="currentColor" stroke="none"/></svg>',
};

function zoneGlyphMarkup(kind) {
  if (isSignalTheme()) return ZONE_ICON_SVGS[kind] || "";
  return escapeHtml(ZONE_EMOJI[kind] || "");
}

// The "System" tile isn't a real zone (no password, no stored data), so it
// gets its own glyph rather than one keyed off a zone kind.
const SYSTEM_ICON_SVG =
  '<svg class="zone-icon-svg zi-system" viewBox="0 0 28 28" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round" stroke-linecap="round">' +
  '<path d="M14 3.5l8.5 3.2v6.8c0 6-3.6 9.6-8.5 11.5-4.9-1.9-8.5-5.5-8.5-11.5V6.7L14 3.5z"/>' +
  '<path d="M9.5 14.5l2.6 2.7 6.4-6.4"/>' +
  '<circle class="cd-node" cx="14" cy="3.5" r="1.3" fill="currentColor" stroke="none"/></svg>';

function systemGlyphMarkup() {
  return isSignalTheme() ? SYSTEM_ICON_SVG : "🛡";
}

function applyKindChoiceIcons() {
  document.querySelectorAll(".kind-choice").forEach((btn) => {
    const iconEl = btn.querySelector(".kc-icon");
    if (iconEl) iconEl.innerHTML = zoneGlyphMarkup(btn.dataset.kind);
  });
}

function updateThemeToggleUI() {
  const current = document.documentElement.dataset.theme;
  document.querySelectorAll(".theme-toggle-btn").forEach((btn) => {
    btn.classList.toggle("active", btn.dataset.themeChoice === current);
  });
}

function applyTheme(theme) {
  document.documentElement.dataset.theme = theme;
  try { localStorage.setItem("cipherden-theme", theme); } catch (e) { /* ignore */ }
  updateThemeToggleUI();
  applyKindChoiceIcons();
  setAmbientCanvasEnabled(theme === "signal");
}

document.querySelectorAll(".theme-toggle-btn").forEach((btn) => {
  btn.addEventListener("click", () => applyTheme(btn.dataset.themeChoice));
});

// --- Signal theme: ambient particle-network background ---------------------
//
// One persistent, low-cost canvas behind every screen (z-index 0, same slot
// Cyberpunk's body::before/::after ambient grid+glow already used). Only
// animates while the Signal theme is active; Cyberpunk never starts it.

const ambientCanvas = document.getElementById("ambient-canvas");
const ambientCtx = ambientCanvas.getContext("2d");
const AMBIENT_PARTICLE_COUNT = 60;
let ambientParticles = [];
let ambientRunning = false;
let ambientRafId = null;
let ambientFormationStart = null;

function ambientResize() {
  ambientCanvas.width = window.innerWidth;
  ambientCanvas.height = window.innerHeight;
}
window.addEventListener("resize", ambientResize);

function ambientInitParticles() {
  ambientResize();
  ambientParticles = [];
  for (let i = 0; i < AMBIENT_PARTICLE_COUNT; i++) {
    ambientParticles.push({
      x: Math.random() * ambientCanvas.width,
      y: Math.random() * ambientCanvas.height,
      vx: (Math.random() - 0.5) * 0.18,
      vy: (Math.random() - 0.5) * 0.18,
    });
  }
}

// A point along the perimeter of a diamond (rotated square) centered at
// (cx, cy) with "radius" r — used to briefly assemble the particle field
// into the ◈ brand mark right after a successful unlock.
function ambientDiamondPoint(i, n, cx, cy, r) {
  const t = i / n;
  const pts = [[cx, cy - r], [cx + r, cy], [cx, cy + r], [cx - r, cy]];
  const seg = Math.floor(t * 4) % 4;
  const localT = t * 4 - Math.floor(t * 4);
  const a = pts[seg], b = pts[(seg + 1) % 4];
  return [a[0] + (b[0] - a[0]) * localT, a[1] + (b[1] - a[1]) * localT];
}

const AMBIENT_FORM_MS = 1500;
const AMBIENT_HOLD_MS = 700;
// Recomputed per-trigger to match the diamond's actual size (see
// triggerSignalFormation) — a flat distance here was much larger than the
// gap between adjacent points on a compact diamond, so nearly every particle
// connected to nearly every other one: a dense tangled scribble instead of a
// clean outline, which read as "crooked" rather than a diamond.
let ambientFormationMaxD = 30;
let ambientFormationCenter = null;
let ambientDriftBlendStart = null;
const AMBIENT_DRIFT_MAXD = 110;
const AMBIENT_DRIFT_BLEND_MS = 900;

// Called once, right after a successful Shell unlock — never on ordinary
// navigation (back-to-desktop, zone open/close), so it reads as a deliberate
// "your vault just connected" moment rather than a repeated tic.
function triggerSignalFormation() {
  if (!isSignalTheme() || ambientParticles.length === 0) return;
  const cx = ambientCanvas.width / 2;
  const cy = ambientCanvas.height / 2;
  const r = Math.min(180, Math.min(ambientCanvas.width, ambientCanvas.height) * 0.16);
  ambientParticles.forEach((p, i) => {
    const [tx, ty] = ambientDiamondPoint(i, ambientParticles.length, cx, cy, r);
    p.tx = tx;
    p.ty = ty;
    // Snapshot the start position so every particle interpolates on the same
    // clock (see ambientFrame's "form" phase) — moving at a rate proportional
    // to remaining distance instead made particles that started close to
    // their target snap into place almost instantly while distant ones were
    // still visibly in transit, so one side of the diamond looked finished
    // and the other looked "crooked"/unfinished for most of the assembly.
    p.formStartX = p.x;
    p.formStartY = p.y;
  });
  // Gap between adjacent points along the diamond's perimeter, times a small
  // margin — connects each particle to its real neighbors on the outline
  // only, not across the whole shape.
  const perimeter = 4 * r * Math.SQRT2;
  const gap = perimeter / ambientParticles.length;
  // 1.6x only linked strict outline neighbors — a thin, flat-looking line.
  // 4.5x adds a light "woven/faceted" cross-texture near each edge and
  // corner (verified across several r values with a static render before
  // picking this) while staying nowhere near the old flat-130px tangle.
  ambientFormationMaxD = gap * 4.5;
  ambientFormationCenter = { cx, cy };
  ambientFormationStart = performance.now();
}

// The formation only reads as a "wow" moment if it plays somewhere
// unobstructed. Before it just ran behind the still-opaque lock card and
// then behind the desktop's zone panel — mostly invisible. This fades the
// lock card out of the way first, so the particle network has the full
// screen to itself for the two seconds the assemble→hold plays out, and
// only then reveals the desktop (right as it starts dispersing into the
// ambient drift, which is fine to have partially obscured).
async function playUnlockFlourish() {
  if (!isSignalTheme()) return;
  const card = document.querySelector("#lock-screen .lock-card");
  triggerSignalFormation();
  if (card) card.classList.add("card-fade-out");
  await new Promise((resolve) => setTimeout(resolve, AMBIENT_FORM_MS + AMBIENT_HOLD_MS - 200));
}

function ambientFrame(ts) {
  const FORM_MS = AMBIENT_FORM_MS, HOLD_MS = AMBIENT_HOLD_MS;
  let phase = "drift";
  let elapsed = 0;
  if (ambientFormationStart != null) {
    elapsed = ts - ambientFormationStart;
    if (elapsed < FORM_MS) phase = "form";
    else if (elapsed < FORM_MS + HOLD_MS) phase = "hold";
    else {
      ambientFormationStart = null;
      ambientDriftBlendStart = ts;
      // Ambient drift is deliberately slow/calm, so left alone the held
      // diamond would just sit there nearly motionless for seconds — a
      // static leftover shape, not a dispersal. A one-time radial outward
      // kick (decayed away in the drift branch below) makes it visibly
      // scatter apart instead — evenly in every direction since the
      // diamond's own points are already evenly spread around its
      // perimeter, just moving out from center. Sized to cover a good
      // portion of the screen (not a precise edge-to-edge fill — the
      // window isn't square, so "reach the edges in sync" would need a
      // per-direction speed anyway, and a graceful uneven settle reads
      // better than a screensaver-style burst).
      if (ambientFormationCenter) {
        const { cx: fcx, cy: fcy } = ambientFormationCenter;
        for (const p of ambientParticles) {
          const dx = p.x - fcx, dy = p.y - fcy;
          const dist = Math.sqrt(dx * dx + dy * dy) || 1;
          const burst = 7 + Math.random() * 4;
          p.burstVx = (dx / dist) * burst;
          p.burstVy = (dy / dist) * burst;
        }
        ambientFormationCenter = null;
      }
    }
  }
  // Ease-out cubic, shared by every particle so they all arrive together
  // regardless of how far each one started from its target (see the
  // formStartX/Y comment in triggerSignalFormation).
  const formT = Math.min(1, elapsed / FORM_MS);
  const formEase = 1 - Math.pow(1 - formT, 3);
  ambientCtx.clearRect(0, 0, ambientCanvas.width, ambientCanvas.height);
  for (const p of ambientParticles) {
    if (phase === "form" && p.tx != null) {
      p.x = p.formStartX + (p.tx - p.formStartX) * formEase;
      p.y = p.formStartY + (p.ty - p.formStartY) * formEase;
    } else if (phase === "hold") {
      // hold formation
    } else {
      if (p.burstVx || p.burstVy) {
        p.x += p.vx + p.burstVx;
        p.y += p.vy + p.burstVy;
        p.burstVx *= 0.97;
        p.burstVy *= 0.97;
        if (Math.abs(p.burstVx) < 0.02) p.burstVx = 0;
        if (Math.abs(p.burstVy) < 0.02) p.burstVy = 0;
      } else {
        p.x += p.vx;
        p.y += p.vy;
      }
      if (p.x < 0 || p.x > ambientCanvas.width) p.vx *= -1;
      if (p.y < 0 || p.y > ambientCanvas.height) p.vy *= -1;
    }
  }
  // Right when dispersal starts, the particles are still physically as
  // close together as they were while held — jumping straight to drift's
  // much larger neighbor radius on a still-tight cluster reads as a sudden
  // "pop" of extra connections. Ease the radius up over the same window the
  // burst takes to actually spread them apart, so density and spacing
  // change together instead of the threshold jumping ahead of the motion.
  let maxD;
  if (phase !== "drift") {
    maxD = ambientFormationMaxD;
  } else if (ambientDriftBlendStart != null) {
    const blendElapsed = ts - ambientDriftBlendStart;
    if (blendElapsed < AMBIENT_DRIFT_BLEND_MS) {
      const bt = blendElapsed / AMBIENT_DRIFT_BLEND_MS;
      const bEase = 1 - Math.pow(1 - bt, 2);
      maxD = ambientFormationMaxD + (AMBIENT_DRIFT_MAXD - ambientFormationMaxD) * bEase;
    } else {
      maxD = AMBIENT_DRIFT_MAXD;
      ambientDriftBlendStart = null;
    }
  } else {
    maxD = AMBIENT_DRIFT_MAXD;
  }
  // The assembled/held diamond is the one deliberate "wow" beat, so its
  // lines and dots get a brighter peak than the calm ambient drift.
  const lineAlpha = phase === "drift" ? 0.09 : 0.22;
  const dotAlpha = phase === "drift" ? 0.55 : 0.85;
  const dotRadius = phase === "drift" ? 1.4 : 1.8;
  ambientCtx.lineWidth = 1;
  for (let i = 0; i < ambientParticles.length; i++) {
    for (let j = i + 1; j < ambientParticles.length; j++) {
      const dx = ambientParticles[i].x - ambientParticles[j].x;
      const dy = ambientParticles[i].y - ambientParticles[j].y;
      const d = Math.sqrt(dx * dx + dy * dy);
      if (d < maxD) {
        ambientCtx.strokeStyle = `rgba(201,145,90,${lineAlpha * (1 - d / maxD)})`;
        ambientCtx.beginPath();
        ambientCtx.moveTo(ambientParticles[i].x, ambientParticles[i].y);
        ambientCtx.lineTo(ambientParticles[j].x, ambientParticles[j].y);
        ambientCtx.stroke();
      }
    }
  }
  for (const p of ambientParticles) {
    ambientCtx.beginPath();
    ambientCtx.arc(p.x, p.y, dotRadius, 0, Math.PI * 2);
    ambientCtx.fillStyle = `rgba(230,200,160,${dotAlpha})`;
    ambientCtx.fill();
  }
  if (ambientRunning) ambientRafId = requestAnimationFrame(ambientFrame);
}

function setAmbientCanvasEnabled(enabled) {
  if (enabled && !ambientRunning) {
    ambientRunning = true;
    if (ambientParticles.length === 0) ambientInitParticles();
    ambientCanvas.classList.remove("hidden");
    ambientRafId = requestAnimationFrame(ambientFrame);
  } else if (!enabled && ambientRunning) {
    ambientRunning = false;
    if (ambientRafId) cancelAnimationFrame(ambientRafId);
    ambientCanvas.classList.add("hidden");
    ambientCtx.clearRect(0, 0, ambientCanvas.width, ambientCanvas.height);
  }
}

// --- Screens ---------------------------------------------------------------

const lockScreen = document.getElementById("lock-screen");
const desktop = document.getElementById("desktop");
const zoneView = document.getElementById("zone-view");
const systemView = document.getElementById("system-view");
const emergencyExitBtn = document.getElementById("btn-emergency-exit");
const lockError = document.getElementById("lock-error");
let systemStatusInterval = null;

function stopSystemStatusPolling() {
  if (systemStatusInterval) {
    clearInterval(systemStatusInterval);
    systemStatusInterval = null;
  }
}

const AUTO_LOCK_SECONDS = 5 * 60; // must match src-tauri's AUTO_LOCK_TIMEOUT
let sessionSecondsLeft = AUTO_LOCK_SECONDS;
let currentZoneId = null;
let currentZoneKind = null;

function resetSessionTimer() {
  sessionSecondsLeft = AUTO_LOCK_SECONDS;
}
["click", "keydown"].forEach((evt) => document.addEventListener(evt, resetSessionTimer));

setInterval(() => {
  if (lockScreen.classList.contains("hidden") === false) return;
  sessionSecondsLeft = Math.max(0, sessionSecondsLeft - 1);
  const m = Math.floor(sessionSecondsLeft / 60).toString().padStart(2, "0");
  const s = (sessionSecondsLeft % 60).toString().padStart(2, "0");
  const text = `${m}:${s}`;
  const shellTimer = document.getElementById("shell-session-timer");
  const zoneTimer = document.getElementById("zone-session-timer");
  if (shellTimer) shellTimer.textContent = text;
  if (zoneTimer) zoneTimer.textContent = text;
}, 1000);

// Cyberpunk switches screens with an instant class toggle, unchanged from
// before. Signal fades the old screen out, then fades the new one in — a
// screen is never mid-transition and mid-layout at once, so there's no
// overlap/jump risk from having two full-height sections in flow together.
function switchScreen(hideEls, showEl) {
  hideEls = Array.isArray(hideEls) ? hideEls : [hideEls];
  emergencyExitBtn.classList.toggle("hidden", showEl === lockScreen);
  if (!isSignalTheme()) {
    hideEls.forEach((el) => el.classList.add("hidden"));
    showEl.classList.remove("hidden");
    return;
  }
  const toFadeOut = hideEls.filter((el) => !el.classList.contains("hidden"));
  toFadeOut.forEach((el) => el.classList.add("screen-fade-out"));
  setTimeout(() => {
    toFadeOut.forEach((el) => { el.classList.add("hidden"); el.classList.remove("screen-fade-out"); });
    showEl.classList.remove("hidden");
    showEl.classList.add("screen-fade-in-start");
    void showEl.offsetWidth; // force reflow so the entrance transition runs
    requestAnimationFrame(() => showEl.classList.remove("screen-fade-in-start"));
  }, toFadeOut.length ? 260 : 0);
}

function showLockScreen(message) {
  switchScreen([desktop, zoneView, systemView], lockScreen);
  stopSystemStatusPolling();
  const card = document.querySelector("#lock-screen .lock-card");
  if (card) card.classList.remove("card-fade-out");
  // The password fields are the one thing on this screen that must never
  // survive a re-lock — leaving the typed password sitting in the DOM
  // would let anyone at the machine just click "Открыть" again without
  // knowing it. The path field is deliberately left alone (that's the
  // known-shells convenience, not a secret).
  document.getElementById("shell-password").value = "";
  document.getElementById("shell-recovery-password").value = "";
  lockError.textContent = message || "";
  currentZoneId = null;
  currentZoneKind = null;
}

function showDesktop() {
  switchScreen([lockScreen, zoneView, systemView], desktop);
  stopSystemStatusPolling();
  sessionSecondsLeft = AUTO_LOCK_SECONDS;
  currentZoneId = null;
  currentZoneKind = null;
  refreshZones();
}

function showSystemView() {
  switchScreen([desktop, lockScreen, zoneView], systemView);
  refreshSystemStatus();
  stopSystemStatusPolling();
  systemStatusInterval = setInterval(refreshSystemStatus, 3000);
}

document.getElementById("btn-system-back").addEventListener("click", showDesktop);

function formatBytes(bytes) {
  if (bytes == null) return "—";
  const units = ["Б", "КБ", "МБ", "ГБ", "ТБ"];
  let value = bytes;
  let i = 0;
  while (value >= 1024 && i < units.length - 1) {
    value /= 1024;
    i++;
  }
  return `${value.toFixed(i === 0 ? 0 : 1)} ${units[i]}`;
}

// Read-only: every value here comes straight from a real measurement
// (get_system_status) or the already-existing zone list — nothing on this
// screen is fabricated, and nothing on it is a control.
async function refreshSystemStatus() {
  try {
    const [zones, status] = await Promise.all([
      invoke("list_zones"),
      invoke("get_system_status"),
    ]);
    const unlockedFlags = await Promise.all(
      zones.map((z) => invoke("is_zone_unlocked", { zoneId: z.id }))
    );

    const zonesListEl = document.getElementById("sys-zones-list");
    zonesListEl.innerHTML = zones.length
      ? zones
          .map(
            (z, i) => `
        <div class="sys-zone-row">
          <div class="sys-zone-row-top">
            <span class="sys-zone-glyph">${zoneGlyphMarkup(z.kind)}</span>
            <span class="sys-zone-label">${escapeHtml(z.label)}</span>
            <span class="sys-zone-state ${unlockedFlags[i] ? "sys-state-unlocked" : "sys-state-locked"}">${unlockedFlags[i] ? "Разблокирован" : "Заблокирован"}</span>
          </div>
          <div class="sys-zone-crypto">AES-256-GCM · Argon2id</div>
        </div>`
          )
          .join("")
      : `<div class="hint">Разделов пока нет.</div>`;

    document.getElementById("sys-shell-size").textContent = formatBytes(status.shell_file_bytes);
    document.getElementById("sys-disk-free").textContent =
      status.disk_free_bytes != null && status.disk_total_bytes != null
        ? `${formatBytes(status.disk_free_bytes)} из ${formatBytes(status.disk_total_bytes)}`
        : "—";
    const bar = document.getElementById("sys-disk-bar");
    if (status.disk_free_bytes != null && status.disk_total_bytes) {
      const usedPct = 100 - (status.disk_free_bytes / status.disk_total_bytes) * 100;
      bar.style.width = `${Math.max(0, Math.min(100, usedPct)).toFixed(1)}%`;
    }

    document.getElementById("sys-ram").textContent = formatBytes(status.process_ram_bytes);
    document.getElementById("sys-cpu").textContent = `${status.process_cpu_percent.toFixed(1)}%`;

    document.getElementById("sys-attempts-total").textContent = String(status.failed_attempts_total);
    const recentEl = document.getElementById("sys-attempts-recent");
    recentEl.textContent = String(status.failed_attempts_recent);
    recentEl.classList.toggle("amount-negative", status.failed_attempts_recent > 0);
    recentEl.classList.toggle("amount-positive", status.failed_attempts_recent === 0);

    document.getElementById("sys-kdf").textContent =
      `Argon2id, ${Math.round(status.argon2_m_cost_kib / 1024)} МиБ, t=${status.argon2_t_cost}, p=${status.argon2_p_cost}`;
    document.getElementById("sys-autolock").textContent = `${Math.round(status.auto_lock_seconds / 60)} мин бездействия`;

    const dot = document.getElementById("system-indicator-dot");
    const label = document.getElementById("system-indicator-label");
    if (status.failed_attempts_recent > 0) {
      dot.classList.add("dot-warn");
      label.textContent = "Есть недавние неудачные попытки входа";
    } else {
      dot.classList.remove("dot-warn");
      label.textContent = "Всё в норме";
    }
  } catch (e) {
    setDesktopStatus("Не удалось получить статус системы: " + e);
  }
}

function showZoneView(zone) {
  switchScreen([desktop, lockScreen, systemView], zoneView);
  currentZoneId = zone.id;
  currentZoneKind = zone.kind;
  document.getElementById("zone-view-title").innerHTML = `${zoneGlyphMarkup(zone.kind)} <span>${escapeHtml(zone.label)}</span>`;

  document.getElementById("accounts-content").classList.toggle("hidden", zone.kind !== "accounts");
  document.getElementById("files-content").classList.toggle("hidden", zone.kind !== "files");
  document.getElementById("seeds-content").classList.toggle("hidden", zone.kind !== "seeds");
  document.getElementById("ledger-content").classList.toggle("hidden", zone.kind !== "ledger");
  document.getElementById("search").classList.toggle("hidden", zone.kind !== "accounts");

  if (zone.kind === "accounts") {
    refreshEntries();
  } else if (zone.kind === "files") {
    filesPath = [];
    refreshFiles();
  } else if (zone.kind === "seeds") {
    refreshSeeds();
  } else if (zone.kind === "ledger") {
    refreshLedger();
  }
}

document.getElementById("btn-zone-back").addEventListener("click", showDesktop);

document.getElementById("btn-shell-lock").addEventListener("click", async () => {
  await invoke("lock_shell");
  showLockScreen();
});

// Emergency quick-exit: no confirmation dialog by design -- the whole
// point is a single instant action for the moment the drive needs to come
// out right now. invoke() fires the request but the backend calls
// std::process::exit(0) before any reply can come back, so the app window
// just disappears; nothing after this call is expected to run.
function triggerEmergencyExit() {
  if (emergencyExitBtn.classList.contains("hidden")) return;
  invoke("emergency_exit").catch(() => {});
}
emergencyExitBtn.addEventListener("click", triggerEmergencyExit);
window.addEventListener("keydown", (e) => {
  if (e.ctrlKey && e.shiftKey && (e.key === "X" || e.key === "x")) {
    e.preventDefault();
    triggerEmergencyExit();
  }
});

document.getElementById("btn-zone-lock").addEventListener("click", async () => {
  if (currentZoneId != null) await invoke("lock_zone", { zoneId: currentZoneId });
  showDesktop();
});

// Poll for server-enforced auto-lock so the UI reflects it even with no
// user-initiated command in flight.
setInterval(async () => {
  if (lockScreen.classList.contains("hidden") === false) return;
  try {
    const shellUnlocked = await invoke("is_shell_unlocked");
    if (!shellUnlocked) {
      showLockScreen("Оболочка автоматически заблокирована по таймауту бездействия.");
      return;
    }
  } catch {
    return;
  }
  if (currentZoneId != null) {
    const zoneUnlocked = await invoke("is_zone_unlocked", { zoneId: currentZoneId });
    if (!zoneUnlocked) {
      setDesktopStatus("Раздел автоматически заблокирован по таймауту бездействия.");
      showDesktop();
    }
  }
}, 10000);

// --- Shell unlock ------------------------------------------------------

const WARN_WITH_RECOVERY = "Оболочка скрывает сам факт существования разделов (аккаунты, файлы...) — без пароля не видно даже их списка. Резервный пароль НЕ открывает оболочку напрямую — он нужен только чтобы задать новый основной пароль, если вы его забыли (кнопка «🔑 Смена пароля» на рабочем столе; тем же способом можно сбросить и сам резервный, зная основной). Но забыты оба — и восстановить доступ невозможно. Сделайте резервную копию файла оболочки на другом носителе.";
const WARN_STRICT = "⚠ Жёсткий режим: один-единственный пароль, без резервного. Забыли его — доступ к оболочке и ко всему внутри потерян НАВСЕГДА, восстановить нечем. Резервный пароль можно будет добавить позже через «🔑 Смена пароля», но только если вы ещё помните этот единственный пароль. Сделайте резервную копию файла оболочки на другом носителе.";

const recoveryCheckbox = document.getElementById("shell-add-recovery");
const recoveryField = document.getElementById("shell-recovery-field");
const lockWarn = document.getElementById("lock-warn");

function updateRecoveryUi() {
  const addRecovery = recoveryCheckbox.checked;
  recoveryField.classList.toggle("hidden", !addRecovery);
  lockWarn.textContent = addRecovery ? WARN_WITH_RECOVERY : WARN_STRICT;
}
recoveryCheckbox.addEventListener("change", updateRecoveryUi);
updateRecoveryUi();

// --- Known shells (lock screen convenience list) ---------------------------
//
// Purely a local UX shortcut: remembers paths of shells this computer has
// opened/created before, so a returning user with several independent
// shells (one per drive, say) doesn't have to re-browse every time. Stored
// in this browser profile's localStorage only — never synced, never sent
// anywhere, and holds nothing but the file path + a timestamp (no password,
// no shell contents). Still real metadata about this person's vaults
// living on THIS computer, not the portable drive, so it's opt-out: each
// entry has a one-click "forget" control.
const KNOWN_SHELLS_KEY = "cipherden-known-shells";
const KNOWN_SHELLS_MAX = 8;

function loadKnownShells() {
  try {
    const raw = localStorage.getItem(KNOWN_SHELLS_KEY);
    const list = raw ? JSON.parse(raw) : [];
    return Array.isArray(list) ? list : [];
  } catch (e) {
    return [];
  }
}

function saveKnownShells(list) {
  try {
    localStorage.setItem(KNOWN_SHELLS_KEY, JSON.stringify(list));
  } catch (e) {
    /* ignore — worst case, the list just doesn't persist */
  }
}

// "shell.vault" is the suggested default filename everywhere in this app,
// so most vaults end up literally sharing that filename — the containing
// folder name is what actually tells them apart, so that's the label.
function shellDisplayName(path) {
  const parts = String(path).replace(/\\/g, "/").split("/").filter(Boolean);
  if (parts.length >= 2) return parts[parts.length - 2];
  return parts[parts.length - 1] || path;
}

function rememberShell(path) {
  if (!path) return;
  let list = loadKnownShells().filter((s) => s.path !== path);
  list.unshift({ path, lastUsedAt: Date.now() });
  saveKnownShells(list.slice(0, KNOWN_SHELLS_MAX));
  renderKnownShells();
}

function forgetShell(path) {
  saveKnownShells(loadKnownShells().filter((s) => s.path !== path));
  renderKnownShells();
}

function renderKnownShells() {
  const list = loadKnownShells();
  const container = document.getElementById("known-shells");
  if (list.length === 0) {
    container.classList.add("hidden");
    container.innerHTML = "";
    return;
  }
  container.classList.remove("hidden");
  container.innerHTML = list
    .map(
      (s) => `
    <div class="known-shell-row" data-path="${escapeHtml(s.path)}">
      <span class="known-shell-icon">◈</span>
      <div class="known-shell-info">
        <span class="known-shell-name">${escapeHtml(shellDisplayName(s.path))}</span>
        <span class="known-shell-path" title="${escapeHtml(s.path)}">${escapeHtml(s.path)}</span>
      </div>
      <button type="button" class="known-shell-remove" title="Убрать из списка">✕</button>
    </div>`
    )
    .join("");

  container.querySelectorAll(".known-shell-row").forEach((row) => {
    row.addEventListener("click", () => {
      document.getElementById("shell-path").value = row.dataset.path;
      document.getElementById("shell-password").focus();
    });
    row.querySelector(".known-shell-remove").addEventListener("click", (e) => {
      e.stopPropagation();
      forgetShell(row.dataset.path);
    });
  });
}

renderKnownShells();

document.getElementById("btn-shell-browse-open").addEventListener("click", async () => {
  const path = await dialog.open({
    title: "Выберите файл оболочки",
    filters: [{ name: "CIPHERDEN vault", extensions: ["vault"] }],
  });
  if (path) document.getElementById("shell-path").value = path;
});

document.getElementById("btn-shell-browse-new").addEventListener("click", async () => {
  const dir = await dialog.open({ directory: true, title: "Куда сохранить новую оболочку" });
  if (!dir) return;
  const sep = dir.includes("\\") && !dir.includes("/") ? "\\" : "/";
  document.getElementById("shell-path").value = `${dir}${sep}shell.vault`;
});

document.getElementById("btn-shell-open").addEventListener("click", async () => {
  const path = document.getElementById("shell-path").value.trim();
  const password = document.getElementById("shell-password").value;
  if (!path) {
    lockError.textContent = "Укажите путь к оболочке (кнопка «Открыть файл») или введите его вручную.";
    return;
  }
  try {
    await invoke("open_shell", { path, password });
    rememberShell(path);
    await playUnlockFlourish();
    showDesktop();
  } catch (e) {
    lockError.textContent = String(e);
  }
});

document.getElementById("btn-shell-create").addEventListener("click", async () => {
  const path = document.getElementById("shell-path").value.trim();
  const primaryPassword = document.getElementById("shell-password").value;
  const addRecovery = recoveryCheckbox.checked;
  const recoveryPassword = addRecovery ? document.getElementById("shell-recovery-password").value : null;

  if (!path) {
    lockError.textContent = "Укажите, где создать оболочку (кнопка «Новое хранилище») или введите путь вручную.";
    return;
  }
  if (primaryPassword.length < 8) {
    lockError.textContent = "Пароль должен быть не короче 8 символов.";
    return;
  }
  if (addRecovery) {
    if (recoveryPassword.length < 8) {
      lockError.textContent = "Резервный пароль должен быть не короче 8 символов.";
      return;
    }
    if (primaryPassword === recoveryPassword) {
      lockError.textContent = "Основной и резервный пароли должны отличаться.";
      return;
    }
  }
  try {
    await invoke("create_shell", { path, primaryPassword, recoveryPassword });
    rememberShell(path);
    await playUnlockFlourish();
    showDesktop();
  } catch (e) {
    lockError.textContent = String(e);
  }
});

// --- Desktop: zone grid --------------------------------------------------

const zonesGrid = document.getElementById("zones-grid");

function setDesktopStatus(text) {
  const el = document.getElementById("desktop-status");
  el.textContent = text;
  if (text) setTimeout(() => { if (el.textContent === text) el.textContent = ""; }, 3500);
}

async function refreshZones() {
  const zones = await invoke("list_zones");
  document.getElementById("zone-count").textContent =
    `${zones.length} ${zones.length === 1 ? "раздел" : "раздела/ов"}`;

  const unlockedFlags = await Promise.all(
    zones.map((z) => invoke("is_zone_unlocked", { zoneId: z.id }))
  );

  zonesGrid.innerHTML = "";
  zones.forEach((zone, i) => {
    const unlocked = unlockedFlags[i];
    const el = document.createElement("div");
    el.className = "file-icon" + (unlocked ? "" : " zone-locked");
    el.innerHTML = `
      <span class="glyph">${zoneGlyphMarkup(zone.kind)}</span>
      ${unlocked ? "" : '<span class="lock-badge">🔒</span>'}
      <span class="label">${escapeHtml(zone.label)}</span>
      <div class="icon-actions">
        <button type="button" class="ghost export-zone-btn" title="Экспортировать как отдельный файл">⇩</button>
        <button type="button" class="danger delete-zone-btn" title="Удалить раздел">✕</button>
      </div>
    `;
    el.addEventListener("click", () => openZoneFromDesktop(zone, unlocked));
    el.querySelector(".export-zone-btn").addEventListener("click", async (e) => {
      e.stopPropagation();
      await exportZoneStandalone(zone);
    });
    el.querySelector(".delete-zone-btn").addEventListener("click", async (e) => {
      e.stopPropagation();
      if (!(await showConfirmModal(`Удалить раздел «${zone.label}» вместе со всем его содержимым? Это необратимо.`))) return;
      try {
        await invoke("delete_zone", { zoneId: zone.id });
        refreshZones();
      } catch (err) {
        setDesktopStatus("Ошибка: " + err);
      }
    });
    zonesGrid.appendChild(el);
  });

  // Always present, never deletable: a read-only status dashboard, not a
  // real zone — no password, no stored data of its own.
  const systemEl = document.createElement("div");
  systemEl.className = "file-icon system-tile";
  systemEl.innerHTML = `
    <span class="glyph">${systemGlyphMarkup()}</span>
    <span class="label">Системная</span>
  `;
  systemEl.addEventListener("click", showSystemView);
  zonesGrid.appendChild(systemEl);

  if (zones.length === 0) {
    const hint = document.createElement("div");
    hint.className = "files-empty-hint";
    hint.textContent = "Разделов пока нет — нажмите «+ Новый раздел», чтобы создать первый.";
    zonesGrid.appendChild(hint);
  }
}

async function exportZoneStandalone(zone) {
  const proceed = await showConfirmModal(
    `Экспортировать раздел «${zone.label}» отдельным файлом? Он будет открываться тем же паролем раздела даже без оболочки — но с этого момента сам факт существования раздела больше не скрыт: файл будет виден на диске.`
  );
  if (!proceed) return;

  const zonePassword = await showPromptModal(`Пароль раздела «${zone.label}»`, { password: true });
  if (zonePassword == null) return;

  const destPath = await dialog.save({
    title: "Куда сохранить отдельный файл раздела",
    defaultPath: `${zone.label}.vault`,
    filters: [{ name: "CIPHERDEN vault", extensions: ["vault"] }],
  });
  if (!destPath) return;

  try {
    await invoke("export_zone_standalone", { zoneId: zone.id, zonePassword, destPath });
    setDesktopStatus(`Раздел экспортирован: ${destPath}`);
  } catch (e) {
    setDesktopStatus("Ошибка экспорта: " + e);
  }
}

async function openZoneFromDesktop(zone, alreadyUnlocked) {
  if (alreadyUnlocked) {
    showZoneView(zone);
    return;
  }
  const password = await showPromptModal(`Пароль раздела «${zone.label}»`, { password: true });
  if (password == null) return;
  try {
    const kind = await invoke("open_zone", { zoneId: zone.id, zonePassword: password });
    showZoneView({ ...zone, kind });
  } catch (e) {
    setDesktopStatus("Ошибка: " + e);
  }
}

// --- New zone modal ------------------------------------------------------

let newZoneKind = "accounts";

document.querySelectorAll(".kind-choice").forEach((btn) => {
  btn.addEventListener("click", () => {
    document.querySelectorAll(".kind-choice").forEach((b) => b.classList.remove("active"));
    btn.classList.add("active");
    newZoneKind = btn.dataset.kind;
  });
});

document.getElementById("btn-zone-add").addEventListener("click", () => {
  newZoneKind = "accounts";
  document.querySelectorAll(".kind-choice").forEach((b) => b.classList.toggle("active", b.dataset.kind === "accounts"));
  document.getElementById("nz-label").value = "";
  document.getElementById("nz-password").value = "";
  document.getElementById("new-zone-error").textContent = "";
  document.getElementById("new-zone-modal").classList.remove("hidden");
});

document.getElementById("btn-new-zone-cancel").addEventListener("click", () => {
  document.getElementById("new-zone-modal").classList.add("hidden");
});

document.getElementById("btn-new-zone-create").addEventListener("click", async () => {
  const label = document.getElementById("nz-label").value.trim();
  const password = document.getElementById("nz-password").value;
  const errorEl = document.getElementById("new-zone-error");
  if (!label) { errorEl.textContent = "Введите название раздела."; return; }
  if (password.length < 8) { errorEl.textContent = "Пароль раздела должен быть не короче 8 символов."; return; }

  const zoneIcons = { accounts: "🔑", files: "🗂", seeds: "🌱", ledger: "💰" };
  const icon = zoneIcons[newZoneKind];
  try {
    await invoke("create_zone", { kind: newZoneKind, label, icon, zonePassword: password });
    document.getElementById("new-zone-modal").classList.add("hidden");
    refreshZones();
  } catch (e) {
    errorEl.textContent = String(e);
  }
});

// ============================================================================
// Accounts zone
// ============================================================================

const status = document.getElementById("status");
const entriesBody = document.getElementById("entries-body");
const modal = document.getElementById("entry-modal");
const categoryFilter = document.getElementById("category-filter");

let editingId = null;
let allEntries = [];
let selectedIds = new Set();
let currentlyRendered = [];

function setStatus(text) {
  status.textContent = text;
  if (text) setTimeout(() => { if (status.textContent === text) status.textContent = ""; }, 3000);
}

async function refreshEntries() {
  try {
    allEntries = await invoke("list_entries", { zoneId: currentZoneId });
    renderEntryCount();
    updateCategoryOptions();
    applyFilters();
  } catch (e) {
    setDesktopStatus("Раздел заблокирован по таймауту, введите пароль снова.");
    showDesktop();
  }
}

function renderEntryCount() {
  // No dedicated counter element in the zone header currently; kept as a
  // no-op hook in case one is added later.
}

function updateCategoryOptions() {
  const selected = categoryFilter.value;
  const categories = [...new Set(allEntries.map((e) => e.category).filter(Boolean))].sort();
  categoryFilter.innerHTML = '<option value="">Все категории</option>' +
    categories.map((c) => `<option value="${escapeHtml(c)}">${escapeHtml(c)}</option>`).join("");
  if (categories.includes(selected)) categoryFilter.value = selected;
}

function applyFilters() {
  const query = document.getElementById("search").value.trim().toLowerCase();
  const category = categoryFilter.value;
  const filtered = allEntries.filter((e) => {
    if (category && e.category !== category) return false;
    if (!query) return true;
    return [e.title, e.username, e.url, e.category].some((f) =>
      (f || "").toLowerCase().includes(query)
    );
  });
  renderEntries(filtered);
}

function renderEntries(entries) {
  currentlyRendered = entries;
  entriesBody.innerHTML = "";
  const visibleIds = new Set(entries.map((e) => e.id));
  for (const id of [...selectedIds]) {
    if (!visibleIds.has(id)) selectedIds.delete(id);
  }

  for (const e of entries) {
    const tr = document.createElement("tr");
    tr.innerHTML = `
      <td><input type="checkbox" class="row-select" data-id="${e.id}" ${selectedIds.has(e.id) ? "checked" : ""} /></td>
      <td>${escapeHtml(e.title)}</td>
      <td>${escapeHtml(e.username)}</td>
      <td>${escapeHtml(e.url)}</td>
      <td>${escapeHtml(e.category)}</td>
      <td>${escapeHtml((e.updated_at || "").slice(0, 16).replace("T", " "))}</td>
      <td><button data-id="${e.id}" class="secondary open-entry">Открыть</button></td>
    `;
    entriesBody.appendChild(tr);
  }
  document.querySelectorAll(".open-entry").forEach((btn) => {
    btn.addEventListener("click", () => openEntryModal(parseInt(btn.dataset.id, 10), entries));
  });
  document.querySelectorAll(".row-select").forEach((cb) => {
    cb.addEventListener("change", () => {
      const id = parseInt(cb.dataset.id, 10);
      if (cb.checked) selectedIds.add(id);
      else selectedIds.delete(id);
    });
  });
  document.getElementById("select-all").checked =
    entries.length > 0 && entries.every((e) => selectedIds.has(e.id));
}

function openEntryModal(id, entries) {
  editingId = id;
  const entry = entries.find((e) => e.id === id);
  document.getElementById("entry-modal-title").textContent = "Редактировать запись";
  document.getElementById("entry-id").value = id;
  document.getElementById("f-title").value = entry.title;
  document.getElementById("f-username").value = entry.username;
  document.getElementById("f-password").value = entry.password;
  document.getElementById("f-url").value = entry.url;
  document.getElementById("f-category").value = entry.category;
  document.getElementById("f-notes").value = entry.notes;
  document.getElementById("btn-delete-entry").classList.remove("hidden");
  modal.classList.remove("hidden");
}

function openNewEntryModal() {
  editingId = null;
  document.getElementById("entry-modal-title").textContent = "Новая запись";
  for (const id of ["f-title", "f-username", "f-password", "f-url", "f-category", "f-notes"]) {
    document.getElementById(id).value = "";
  }
  document.getElementById("btn-delete-entry").classList.add("hidden");
  modal.classList.remove("hidden");
}

function closeModal() {
  modal.classList.add("hidden");
}

function currentEntryForm() {
  return {
    title: document.getElementById("f-title").value.trim(),
    username: document.getElementById("f-username").value.trim(),
    password: document.getElementById("f-password").value,
    url: document.getElementById("f-url").value.trim(),
    category: document.getElementById("f-category").value.trim(),
    notes: document.getElementById("f-notes").value,
  };
}

document.getElementById("search").addEventListener("input", applyFilters);
categoryFilter.addEventListener("change", applyFilters);

document.getElementById("btn-add").addEventListener("click", openNewEntryModal);
document.getElementById("btn-cancel-entry").addEventListener("click", closeModal);

document.getElementById("btn-generate").addEventListener("click", async () => {
  const { password } = await invoke("generate_password", { length: 20, useSymbols: true });
  document.getElementById("f-password").value = password;
});

document.getElementById("btn-copy-password").addEventListener("click", async () => {
  const password = document.getElementById("f-password").value;
  if (!password) return;
  await invoke("copy_to_clipboard_with_autoclear", { text: password });
  setStatus("Пароль скопирован, автоочистка буфера через 20 сек.");
});

document.getElementById("btn-save-entry").addEventListener("click", async () => {
  const entry = currentEntryForm();
  if (!entry.title) return;
  try {
    if (editingId != null) {
      await invoke("update_entry", { zoneId: currentZoneId, id: editingId, entry });
    } else {
      await invoke("add_entry", { zoneId: currentZoneId, entry });
    }
    closeModal();
    refreshEntries();
  } catch (e) {
    setStatus("Ошибка: " + e);
  }
});

document.getElementById("btn-delete-entry").addEventListener("click", async () => {
  if (editingId == null) return;
  await invoke("delete_entry", { zoneId: currentZoneId, id: editingId });
  closeModal();
  refreshEntries();
});

document.getElementById("select-all").addEventListener("change", (e) => {
  if (e.target.checked) currentlyRendered.forEach((entry) => selectedIds.add(entry.id));
  else currentlyRendered.forEach((entry) => selectedIds.delete(entry.id));
  renderEntries(currentlyRendered);
});

document.getElementById("btn-export-selected").addEventListener("click", async () => {
  if (selectedIds.size === 0) {
    setStatus("Сначала отметьте галочками строки для экспорта.");
    return;
  }
  const destPath = await dialog.save({
    title: "Куда сохранить CSV",
    defaultPath: "export.csv",
    filters: [{ name: "CSV", extensions: ["csv"] }],
  });
  if (!destPath) return;
  try {
    const written = await invoke("export_csv", { zoneId: currentZoneId, ids: [...selectedIds], destPath });
    setStatus(`Экспортировано строк: ${written}`);
  } catch (e) {
    setStatus("Ошибка экспорта: " + e);
  }
});

document.getElementById("btn-import-csv").addEventListener("click", async () => {
  const path = await dialog.open({
    title: "Выберите CSV-файл для импорта",
    filters: [{ name: "CSV", extensions: ["csv"] }],
  });
  if (!path) return;
  try {
    const count = await invoke("import_csv", { zoneId: currentZoneId, path });
    setStatus(`Импортировано записей: ${count}`);
    refreshEntries();
  } catch (e) {
    setStatus("Ошибка импорта CSV: " + e);
  }
});

document.getElementById("btn-import-kdbx").addEventListener("click", async () => {
  const path = await dialog.open({
    title: "Выберите файл KeePass",
    filters: [{ name: "KeePass", extensions: ["kdbx"] }],
  });
  if (!path) return;
  const kdbxPassword = await showPromptModal("Пароль от базы KeePass (не пароль раздела CIPHERDEN)", { password: true });
  if (kdbxPassword == null) return;
  try {
    const count = await invoke("import_kdbx", { zoneId: currentZoneId, path, kdbxPassword });
    setStatus(`Импортировано записей: ${count}`);
    refreshEntries();
  } catch (e) {
    setStatus("Ошибка импорта KeePass: " + e);
  }
});

// ============================================================================
// Files zone: folders, icon-grid "desktop", drag-and-drop, in-app preview
// ============================================================================

const filesBreadcrumb = document.getElementById("files-breadcrumb");
const filesGrid = document.getElementById("files-grid");
const filesStatus = document.getElementById("files-status");

let filesPath = [];

function currentFolderId() {
  return filesPath.length ? filesPath[filesPath.length - 1].id : null;
}
function parentFolderId() {
  return filesPath.length >= 2 ? filesPath[filesPath.length - 2].id : null;
}

function setFilesStatus(text) {
  filesStatus.textContent = text;
  if (text) setTimeout(() => { if (filesStatus.textContent === text) filesStatus.textContent = ""; }, 3000);
}

function humanSize(bytes) {
  if (bytes < 1024) return `${bytes} Б`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} КБ`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} МБ`;
}

function fileGlyph(name) {
  const ext = (name.split(".").pop() || "").toLowerCase();
  if (["jpg", "jpeg", "png", "gif", "webp", "bmp", "svg"].includes(ext)) return "🖼";
  if (["pdf", "doc", "docx", "txt", "md", "rtf"].includes(ext)) return "📄";
  if (["zip", "rar", "7z", "tar", "gz"].includes(ext)) return "🗜";
  if (["mp3", "wav", "flac", "ogg"].includes(ext)) return "🎵";
  if (["mp4", "mov", "avi", "mkv"].includes(ext)) return "🎞";
  return "📦";
}

function renderBreadcrumb() {
  const parts = [`<span class="crumb${filesPath.length === 0 ? " current" : ""}" data-index="-1">⌂ Корень</span>`];
  filesPath.forEach((f, i) => {
    const isLast = i === filesPath.length - 1;
    parts.push('<span class="sep">/</span>');
    parts.push(`<span class="crumb${isLast ? " current" : ""}" data-index="${i}">${escapeHtml(f.name)}</span>`);
  });
  filesBreadcrumb.innerHTML = parts.join("");
  filesBreadcrumb.querySelectorAll(".crumb[data-index]").forEach((el) => {
    el.addEventListener("click", () => {
      const idx = parseInt(el.dataset.index, 10);
      if (idx === filesPath.length - 1) return;
      filesPath = idx < 0 ? [] : filesPath.slice(0, idx + 1);
      refreshFiles();
    });
  });
}

async function refreshFiles() {
  try {
    const folderId = currentFolderId();
    const [folders, files] = await Promise.all([
      invoke("list_folders", { zoneId: currentZoneId, parentId: folderId }),
      invoke("list_files", { zoneId: currentZoneId, folderId }),
    ]);
    renderBreadcrumb();
    renderFilesGrid(folders, files);
  } catch (e) {
    setDesktopStatus("Раздел заблокирован по таймауту, введите пароль снова.");
    showDesktop();
  }
}

function renderFilesGrid(folders, files) {
  filesGrid.innerHTML = "";
  if (filesPath.length > 0) filesGrid.appendChild(makeUpTile());
  for (const f of folders) filesGrid.appendChild(makeFolderTile(f));
  for (const f of files) filesGrid.appendChild(makeFileTile(f));

  if (folders.length === 0 && files.length === 0 && filesPath.length === 0) {
    const hint = document.createElement("div");
    hint.className = "files-empty-hint";
    hint.textContent = "Пусто. Добавьте файлы через «+ Добавить файлы» или создайте папку.";
    filesGrid.appendChild(hint);
  }
}

function makeUpTile() {
  const el = document.createElement("div");
  el.className = "file-icon";
  el.innerHTML = `<span class="glyph">⬆</span><span class="label">..</span>`;
  el.addEventListener("dblclick", () => {
    filesPath.pop();
    refreshFiles();
  });
  setDropTarget(el, async (dragged) => {
    const targetParent = parentFolderId();
    if (dragged.type === "file") await invoke("move_file", { zoneId: currentZoneId, id: dragged.id, folderId: targetParent });
    else await invoke("move_folder", { zoneId: currentZoneId, id: dragged.id, newParentId: targetParent });
  });
  return el;
}

function makeFolderTile(folder) {
  const el = document.createElement("div");
  el.className = "file-icon";
  el.draggable = true;
  el.innerHTML = `
    ${folder.pinned ? '<span class="pin-badge">📌</span>' : ""}
    <span class="glyph">📁</span>
    <span class="label">${escapeHtml(folder.name)}</span>
    <div class="icon-actions">
      <button type="button" class="ghost pin-toggle" title="Закрепить">📌</button>
      <button type="button" class="danger delete-btn" title="Удалить">✕</button>
    </div>
  `;
  el.addEventListener("dblclick", () => {
    filesPath.push({ id: folder.id, name: folder.name });
    refreshFiles();
  });
  el.querySelector(".pin-toggle").addEventListener("click", async (e) => {
    e.stopPropagation();
    await invoke("set_folder_pinned", { zoneId: currentZoneId, id: folder.id, pinned: !folder.pinned });
    refreshFiles();
  });
  el.querySelector(".delete-btn").addEventListener("click", async (e) => {
    e.stopPropagation();
    if (!(await showConfirmModal(`Удалить папку «${folder.name}»? Она должна быть пустой.`))) return;
    try {
      await invoke("delete_folder", { zoneId: currentZoneId, id: folder.id });
      refreshFiles();
    } catch (err) {
      setFilesStatus("Ошибка: " + err);
    }
  });

  setDragSource(el, { type: "folder", id: folder.id });
  setDropTarget(el, async (dragged) => {
    if (dragged.type === "folder" && dragged.id === folder.id) return;
    try {
      if (dragged.type === "file") await invoke("move_file", { zoneId: currentZoneId, id: dragged.id, folderId: folder.id });
      else await invoke("move_folder", { zoneId: currentZoneId, id: dragged.id, newParentId: folder.id });
    } catch (err) {
      setFilesStatus("Ошибка перемещения: " + err);
    }
  });
  return el;
}

function makeFileTile(file) {
  const el = document.createElement("div");
  el.className = "file-icon";
  el.draggable = true;
  el.innerHTML = `
    ${file.pinned ? '<span class="pin-badge">📌</span>' : ""}
    <span class="glyph">${fileGlyph(file.name)}</span>
    <span class="label">${escapeHtml(file.name)}</span>
    <div class="icon-actions">
      <button type="button" class="ghost pin-toggle" title="Закрепить">📌</button>
      <button type="button" class="ghost extract-btn" title="Извлечь">⇩</button>
      <button type="button" class="danger delete-btn" title="Удалить">✕</button>
    </div>
  `;
  el.title = `${file.name} — ${humanSize(file.size)}`;
  el.addEventListener("dblclick", () => openPreview(file));
  el.querySelector(".pin-toggle").addEventListener("click", async (e) => {
    e.stopPropagation();
    await invoke("set_file_pinned", { zoneId: currentZoneId, id: file.id, pinned: !file.pinned });
    refreshFiles();
  });
  el.querySelector(".extract-btn").addEventListener("click", (e) => {
    e.stopPropagation();
    extractFile(file);
  });
  el.querySelector(".delete-btn").addEventListener("click", async (e) => {
    e.stopPropagation();
    if (!(await showConfirmModal(`Удалить файл «${file.name}»?`))) return;
    await invoke("delete_file", { zoneId: currentZoneId, id: file.id });
    refreshFiles();
  });

  setDragSource(el, { type: "file", id: file.id });
  return el;
}

function setDragSource(el, payload) {
  el.addEventListener("dragstart", (e) => {
    e.dataTransfer.setData("application/json", JSON.stringify(payload));
    e.dataTransfer.effectAllowed = "move";
    setTimeout(() => el.classList.add("dragging"), 0);
  });
  el.addEventListener("dragend", () => el.classList.remove("dragging"));
}

function setDropTarget(el, onDrop) {
  el.addEventListener("dragover", (e) => {
    e.preventDefault();
    el.classList.add("drag-over");
  });
  el.addEventListener("dragleave", () => el.classList.remove("drag-over"));
  el.addEventListener("drop", async (e) => {
    e.preventDefault();
    el.classList.remove("drag-over");
    const raw = e.dataTransfer.getData("application/json");
    if (!raw) return;
    const dragged = JSON.parse(raw);
    await onDrop(dragged);
    refreshFiles();
  });
}

async function extractFile(file) {
  const destPath = await dialog.save({ title: "Куда сохранить файл", defaultPath: file.name });
  if (!destPath) return;
  try {
    await invoke("extract_file", { zoneId: currentZoneId, id: file.id, destPath });
    setFilesStatus("Файл сохранён: " + destPath);
  } catch (e) {
    setFilesStatus("Ошибка: " + e);
  }
}

let currentPreviewObjectUrl = null;

function base64ToBytes(b64) {
  const binary = atob(b64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

function closePreview() {
  document.getElementById("preview-modal").classList.add("hidden");
  if (currentPreviewObjectUrl) {
    URL.revokeObjectURL(currentPreviewObjectUrl);
    currentPreviewObjectUrl = null;
  }
  document.getElementById("preview-body").innerHTML = "";
}

async function openPreview(file) {
  document.getElementById("preview-title").textContent = file.name;
  document.getElementById("preview-body").innerHTML = '<p class="hint">Загрузка...</p>';
  document.getElementById("preview-modal").classList.remove("hidden");
  document.getElementById("preview-extract").onclick = () => extractFile(file);

  let preview;
  try {
    preview = await invoke("read_file_preview", { zoneId: currentZoneId, id: file.id });
  } catch (e) {
    document.getElementById("preview-body").innerHTML = `<p class="error">Ошибка: ${escapeHtml(String(e))}</p>`;
    return;
  }

  const bytes = base64ToBytes(preview.data_base64);
  const blob = new Blob([bytes], { type: preview.mime });
  currentPreviewObjectUrl = URL.createObjectURL(blob);

  const body = document.getElementById("preview-body");
  if (preview.mime.startsWith("image/")) {
    body.innerHTML = `<img src="${currentPreviewObjectUrl}" class="preview-image" alt="${escapeHtml(file.name)}" />`;
  } else if (preview.mime === "application/pdf") {
    body.innerHTML = `<iframe src="${currentPreviewObjectUrl}" class="preview-frame" title="${escapeHtml(file.name)}"></iframe>`;
  } else if (preview.mime.startsWith("text/") || preview.mime === "application/json") {
    const text = new TextDecoder("utf-8").decode(bytes);
    body.innerHTML = '<pre class="preview-text"></pre>';
    body.querySelector("pre").textContent = text;
  } else if (preview.mime.startsWith("audio/")) {
    body.innerHTML = `<audio controls src="${currentPreviewObjectUrl}" class="preview-media"></audio>`;
  } else if (preview.mime.startsWith("video/")) {
    body.innerHTML = `<video controls src="${currentPreviewObjectUrl}" class="preview-media"></video>`;
  } else {
    body.innerHTML = `<p class="hint">Предпросмотр недоступен для этого типа файла. Нажмите «⇩ Извлечь», чтобы сохранить его на диск и открыть обычной программой.</p>`;
  }
}

document.getElementById("preview-close").addEventListener("click", closePreview);

document.getElementById("btn-folder-add").addEventListener("click", async () => {
  const name = await showPromptModal("Название новой папки", { placeholder: "Например: Документы" });
  if (!name) return;
  try {
    await invoke("create_folder", { zoneId: currentZoneId, parentId: currentFolderId(), name });
    refreshFiles();
  } catch (e) {
    setFilesStatus("Ошибка: " + e);
  }
});

document.getElementById("btn-file-add").addEventListener("click", async () => {
  const paths = await dialog.open({ multiple: true, title: "Выберите файлы для добавления" });
  if (!paths) return;
  const list = Array.isArray(paths) ? paths : [paths];
  try {
    const added = await invoke("add_files", { zoneId: currentZoneId, folderId: currentFolderId(), paths: list });
    setFilesStatus(`Добавлено файлов: ${added}`);
    refreshFiles();
  } catch (e) {
    setFilesStatus("Ошибка добавления: " + e);
  }
});

document.getElementById("btn-shell-backup").addEventListener("click", async () => {
  const destDir = await dialog.open({ directory: true, title: "Куда сохранить резервную копию оболочки" });
  if (!destDir) return;
  try {
    const path = await invoke("export_shell_backup", { destDir });
    setDesktopStatus("Резервная копия сохранена: " + path);
  } catch (e) {
    setDesktopStatus("Ошибка резервного копирования: " + e);
  }
});

// --- Change Shell password (primary <-> recovery, either resets the other) ---

let changePasswordSlot = "primary";

document.querySelectorAll(".slot-choice").forEach((btn) => {
  btn.addEventListener("click", () => {
    document.querySelectorAll(".slot-choice").forEach((b) => b.classList.remove("active"));
    btn.classList.add("active");
    changePasswordSlot = btn.dataset.slot;
  });
});

document.getElementById("btn-shell-password").addEventListener("click", () => {
  changePasswordSlot = "primary";
  document.querySelectorAll(".slot-choice").forEach((b) => b.classList.toggle("active", b.dataset.slot === "primary"));
  document.getElementById("cp-known").value = "";
  document.getElementById("cp-new").value = "";
  document.getElementById("change-password-error").textContent = "";
  document.getElementById("change-password-modal").classList.remove("hidden");
});

document.getElementById("btn-change-password-cancel").addEventListener("click", () => {
  document.getElementById("change-password-modal").classList.add("hidden");
});

document.getElementById("btn-change-password-submit").addEventListener("click", async () => {
  const knownPassword = document.getElementById("cp-known").value;
  const newPassword = document.getElementById("cp-new").value;
  const errorEl = document.getElementById("change-password-error");
  if (newPassword.length < 8) { errorEl.textContent = "Новый пароль должен быть не короче 8 символов."; return; }
  if (newPassword === knownPassword) { errorEl.textContent = "Новый пароль должен отличаться от того, которым вы входите."; return; }

  const path = document.getElementById("shell-path").value.trim();
  try {
    await invoke("change_shell_password", { path, knownPassword, slot: changePasswordSlot, newPassword });
    document.getElementById("change-password-modal").classList.add("hidden");
    setDesktopStatus("Пароль изменён.");
  } catch (e) {
    errorEl.textContent = String(e);
  }
});

// ============================================================================
// Seeds zone
// ============================================================================

const seedsBody = document.getElementById("seeds-body");
const seedModal = document.getElementById("seed-modal");
const seedsStatus = document.getElementById("seeds-status");

let editingSeedId = null;
let allSeeds = [];

function setSeedsStatus(text) {
  seedsStatus.textContent = text;
  if (text) setTimeout(() => { if (seedsStatus.textContent === text) seedsStatus.textContent = ""; }, 3000);
}

async function refreshSeeds() {
  try {
    allSeeds = await invoke("list_seeds", { zoneId: currentZoneId });
    renderSeeds(allSeeds);
  } catch (e) {
    setDesktopStatus("Раздел заблокирован по таймауту, введите пароль снова.");
    showDesktop();
  }
}

function renderSeeds(seeds) {
  seedsBody.innerHTML = "";
  for (const s of seeds) {
    const tr = document.createElement("tr");
    tr.innerHTML = `
      <td>${escapeHtml(s.label)}</td>
      <td>${escapeHtml(s.network)}</td>
      <td class="mono">${escapeHtml(s.derivation_path)}</td>
      <td>${escapeHtml((s.updated_at || "").slice(0, 16).replace("T", " "))}</td>
      <td><button data-id="${s.id}" class="secondary open-seed">Открыть</button></td>
    `;
    seedsBody.appendChild(tr);
  }
  document.querySelectorAll(".open-seed").forEach((btn) => {
    btn.addEventListener("click", () => openSeedModal(parseInt(btn.dataset.id, 10)));
  });

  if (seeds.length === 0) {
    const tr = document.createElement("tr");
    tr.innerHTML = `<td colspan="5" class="hint">Записей пока нет — нажмите «+ Новая запись».</td>`;
    seedsBody.appendChild(tr);
  }
}

function openSeedModal(id) {
  editingSeedId = id;
  const seed = allSeeds.find((s) => s.id === id);
  document.getElementById("seed-modal-title").textContent = "Редактировать запись";
  document.getElementById("seed-id").value = id;
  document.getElementById("sf-label").value = seed.label;
  document.getElementById("sf-network").value = seed.network;
  document.getElementById("sf-derivation").value = seed.derivation_path;
  document.getElementById("sf-phrase").value = seed.seed_phrase;
  document.getElementById("sf-notes").value = seed.notes;
  document.getElementById("btn-delete-seed").classList.remove("hidden");
  seedModal.classList.remove("hidden");
}

document.getElementById("btn-seed-add").addEventListener("click", () => {
  editingSeedId = null;
  document.getElementById("seed-modal-title").textContent = "Новая запись";
  for (const id of ["sf-label", "sf-network", "sf-derivation", "sf-phrase", "sf-notes"]) {
    document.getElementById(id).value = "";
  }
  document.getElementById("btn-delete-seed").classList.add("hidden");
  seedModal.classList.remove("hidden");
});

document.getElementById("btn-cancel-seed").addEventListener("click", () => {
  seedModal.classList.add("hidden");
});

document.getElementById("btn-save-seed").addEventListener("click", async () => {
  const entry = {
    label: document.getElementById("sf-label").value.trim(),
    network: document.getElementById("sf-network").value.trim(),
    derivation_path: document.getElementById("sf-derivation").value.trim(),
    seed_phrase: document.getElementById("sf-phrase").value.trim(),
    notes: document.getElementById("sf-notes").value,
  };
  if (!entry.label) return;
  try {
    if (editingSeedId != null) {
      await invoke("update_seed", { zoneId: currentZoneId, id: editingSeedId, entry });
    } else {
      await invoke("add_seed", { zoneId: currentZoneId, entry });
    }
    seedModal.classList.add("hidden");
    refreshSeeds();
  } catch (e) {
    setSeedsStatus("Ошибка: " + e);
  }
});

document.getElementById("btn-delete-seed").addEventListener("click", async () => {
  if (editingSeedId == null) return;
  if (!(await showConfirmModal("Удалить эту запись?"))) return;
  await invoke("delete_seed", { zoneId: currentZoneId, id: editingSeedId });
  seedModal.classList.add("hidden");
  refreshSeeds();
});

// ============================================================================
// Ledger (Balance) zone
// ============================================================================

const ledgerBody = document.getElementById("ledger-body");
const txModal = document.getElementById("tx-modal");
const ledgerStatus = document.getElementById("ledger-status");
const ledgerTotalValue = document.getElementById("ledger-total-value");

let editingTxId = null;

function setLedgerStatus(text) {
  ledgerStatus.textContent = text;
  if (text) setTimeout(() => { if (ledgerStatus.textContent === text) ledgerStatus.textContent = ""; }, 3000);
}

// Parses "-500,22" / "+700,96" / "1234.5" into signed integer cents.
// Accepts either comma or dot as the decimal separator; the sign must be
// typed explicitly (a bare number with no sign is treated as positive).
function parseAmountToCents(raw) {
  const normalized = raw.trim().replace(",", ".").replace(/\s+/g, "");
  if (!/^[+-]?\d+(\.\d{1,2})?$/.test(normalized)) return null;
  const value = parseFloat(normalized);
  if (!Number.isFinite(value)) return null;
  return Math.round(value * 100);
}

function formatCents(cents) {
  const sign = cents < 0 ? "-" : "+";
  const abs = Math.abs(cents);
  const whole = Math.floor(abs / 100);
  const frac = (abs % 100).toString().padStart(2, "0");
  return `${sign}${whole.toLocaleString("ru-RU")},${frac}`;
}

// Formats an ISO date/datetime string as "дд.мм.гг" (2-digit year).
function formatDateShort(iso) {
  const datePart = (iso || "").slice(0, 10);
  const [y, m, d] = datePart.split("-");
  if (!y || !m || !d) return escapeHtml(iso || "");
  return `${d}.${m}.${y.slice(-2)}`;
}

async function refreshLedger() {
  try {
    const { transactions, total_cents } = await invoke("list_transactions_with_total", { zoneId: currentZoneId });
    renderLedger(transactions);
    renderLedgerChart(transactions, total_cents);
    ledgerTotalValue.textContent = formatCents(total_cents);
    ledgerTotalValue.classList.toggle("amount-negative", total_cents < 0);
    ledgerTotalValue.classList.toggle("amount-positive", total_cents >= 0);
  } catch (e) {
    setDesktopStatus("Раздел заблокирован по таймауту, введите пароль снова.");
    showDesktop();
  }
}

const ledgerChartEl = document.getElementById("ledger-chart");

function renderLedgerChart(transactions, totalCents) {
  let incomeCents = 0;
  let outcomeCents = 0;
  for (const t of transactions) {
    if (t.amount_cents >= 0) incomeCents += t.amount_cents;
    else outcomeCents += -t.amount_cents;
  }
  const maxCents = Math.max(incomeCents, outcomeCents, 1);
  const incomePct = (incomeCents / maxCents) * 100;
  const outcomePct = (outcomeCents / maxCents) * 100;
  const totalClass = totalCents < 0 ? "amount-negative" : "amount-positive";
  ledgerChartEl.innerHTML = `
    <div class="lc-headline">
      <div class="lc-headline-label">Сейчас всего денег</div>
      <div class="lc-headline-value ${totalClass}">${formatCents(totalCents)}</div>
    </div>
    <div class="lc-bars">
      <div class="lc-bar-row">
        <div class="lc-bar-label">Приход всего</div>
        <div class="lc-bar-track"><div class="lc-bar-fill lc-fill-income" style="width:${incomePct}%"></div></div>
        <div class="lc-bar-value amount-positive">${formatCents(incomeCents)}</div>
      </div>
      <div class="lc-bar-row">
        <div class="lc-bar-label">Уход всего</div>
        <div class="lc-bar-track"><div class="lc-bar-fill lc-fill-outcome" style="width:${outcomePct}%"></div></div>
        <div class="lc-bar-value amount-negative">${formatCents(-outcomeCents)}</div>
      </div>
    </div>
  `;
}

function renderLedger(transactions) {
  ledgerBody.innerHTML = "";
  for (const t of transactions) {
    const tr = document.createElement("tr");
    const amountClass = t.amount_cents < 0 ? "amount-negative" : "amount-positive";
    tr.innerHTML = `
      <td>${formatDateShort(t.date)}</td>
      <td class="mono ${amountClass}">${formatCents(t.amount_cents)}</td>
      <td>${escapeHtml(t.comment)}</td>
      <td><button data-id="${t.id}" class="secondary open-tx">Открыть</button></td>
    `;
    ledgerBody.appendChild(tr);
  }
  document.querySelectorAll(".open-tx").forEach((btn) => {
    btn.addEventListener("click", () => {
      const id = parseInt(btn.dataset.id, 10);
      const tx = transactions.find((t) => t.id === id);
      openTxModal(tx);
    });
  });

  if (transactions.length === 0) {
    const tr = document.createElement("tr");
    tr.innerHTML = `<td colspan="4" class="hint">Записей пока нет — нажмите «+ Новая запись».</td>`;
    ledgerBody.appendChild(tr);
  }
}

function openTxModal(tx) {
  editingTxId = tx.id;
  document.getElementById("tx-modal-title").textContent = "Редактировать запись";
  document.getElementById("tx-id").value = tx.id;
  document.getElementById("tf-amount").value = formatCents(tx.amount_cents);
  document.getElementById("tf-comment").value = tx.comment;
  const hint = document.getElementById("tx-date-hint");
  hint.textContent = `Дата записи: ${(tx.date || "").slice(0, 16).replace("T", " ")} (не меняется при редактировании)`;
  hint.classList.remove("hidden");
  document.getElementById("tx-modal-error").textContent = "";
  document.getElementById("btn-delete-tx").classList.remove("hidden");
  txModal.classList.remove("hidden");
}

document.getElementById("btn-tx-add").addEventListener("click", () => {
  editingTxId = null;
  document.getElementById("tx-modal-title").textContent = "Новая запись";
  document.getElementById("tf-amount").value = "";
  document.getElementById("tf-comment").value = "";
  document.getElementById("tx-date-hint").classList.add("hidden");
  document.getElementById("tx-modal-error").textContent = "";
  document.getElementById("btn-delete-tx").classList.add("hidden");
  txModal.classList.remove("hidden");
});

document.getElementById("btn-cancel-tx").addEventListener("click", () => {
  txModal.classList.add("hidden");
});

document.getElementById("btn-save-tx").addEventListener("click", async () => {
  const errorEl = document.getElementById("tx-modal-error");
  const amountRaw = document.getElementById("tf-amount").value;
  const comment = document.getElementById("tf-comment").value.trim();
  const amountCents = parseAmountToCents(amountRaw);
  if (amountCents == null) {
    errorEl.textContent = "Введите сумму в формате -500,22 или +700,96.";
    return;
  }
  try {
    if (editingTxId != null) {
      await invoke("update_transaction", { zoneId: currentZoneId, id: editingTxId, amountCents, comment });
    } else {
      await invoke("add_transaction", { zoneId: currentZoneId, amountCents, comment });
    }
    txModal.classList.add("hidden");
    refreshLedger();
  } catch (e) {
    errorEl.textContent = "Ошибка: " + e;
  }
});

document.getElementById("btn-delete-tx").addEventListener("click", async () => {
  if (editingTxId == null) return;
  if (!(await showConfirmModal("Удалить эту запись?"))) return;
  await invoke("delete_transaction", { zoneId: currentZoneId, id: editingTxId });
  txModal.classList.add("hidden");
  refreshLedger();
});

updateThemeToggleUI();
applyKindChoiceIcons();
setAmbientCanvasEnabled(isSignalTheme());
showLockScreen();
