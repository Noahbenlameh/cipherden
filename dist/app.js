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

// --- Screens ---------------------------------------------------------------

const lockScreen = document.getElementById("lock-screen");
const desktop = document.getElementById("desktop");
const zoneView = document.getElementById("zone-view");
const lockError = document.getElementById("lock-error");

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

function showLockScreen(message) {
  lockScreen.classList.remove("hidden");
  desktop.classList.add("hidden");
  zoneView.classList.add("hidden");
  lockError.textContent = message || "";
  currentZoneId = null;
  currentZoneKind = null;
}

function showDesktop() {
  lockScreen.classList.add("hidden");
  zoneView.classList.add("hidden");
  desktop.classList.remove("hidden");
  sessionSecondsLeft = AUTO_LOCK_SECONDS;
  currentZoneId = null;
  currentZoneKind = null;
  refreshZones();
}

function showZoneView(zone) {
  desktop.classList.add("hidden");
  lockScreen.classList.add("hidden");
  zoneView.classList.remove("hidden");
  currentZoneId = zone.id;
  currentZoneKind = zone.kind;
  document.getElementById("zone-view-title").textContent = `${zone.icon} ${zone.label}`;

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
    lockError.textContent = "Укажите путь к оболочке (кнопка 📂) или введите его вручную.";
    return;
  }
  try {
    await invoke("open_shell", { path, password });
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
    lockError.textContent = "Укажите, где создать оболочку (кнопка 📁) или введите путь вручную.";
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
      <span class="glyph">${zone.icon}</span>
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

async function refreshLedger() {
  try {
    const { transactions, total_cents } = await invoke("list_transactions_with_total", { zoneId: currentZoneId });
    renderLedger(transactions);
    ledgerTotalValue.textContent = formatCents(total_cents);
    ledgerTotalValue.classList.toggle("amount-negative", total_cents < 0);
    ledgerTotalValue.classList.toggle("amount-positive", total_cents >= 0);
  } catch (e) {
    setDesktopStatus("Раздел заблокирован по таймауту, введите пароль снова.");
    showDesktop();
  }
}

function renderLedger(transactions) {
  ledgerBody.innerHTML = "";
  for (const t of transactions) {
    const tr = document.createElement("tr");
    const amountClass = t.amount_cents < 0 ? "amount-negative" : "amount-positive";
    tr.innerHTML = `
      <td>${escapeHtml((t.date || "").slice(0, 16).replace("T", " "))}</td>
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

showLockScreen();
