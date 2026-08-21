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

const lockScreen = document.getElementById("lock-screen");
const dashboard = document.getElementById("dashboard");
const lockError = document.getElementById("lock-error");
const status = document.getElementById("status");
const entriesBody = document.getElementById("entries-body");
const modal = document.getElementById("entry-modal");
const categoryFilter = document.getElementById("category-filter");

let editingId = null;
let allEntries = [];
let selectedIds = new Set();

const AUTO_LOCK_SECONDS = 5 * 60; // must match src-tauri's AUTO_LOCK_TIMEOUT
let sessionSecondsLeft = AUTO_LOCK_SECONDS;

function showDashboard() {
  lockScreen.classList.add("hidden");
  dashboard.classList.remove("hidden");
  sessionSecondsLeft = AUTO_LOCK_SECONDS;
  refreshEntries();
}

function showLockScreen(message) {
  dashboard.classList.add("hidden");
  lockScreen.classList.remove("hidden");
  lockError.textContent = message || "";
}

function resetSessionTimer() {
  sessionSecondsLeft = AUTO_LOCK_SECONDS;
}

function renderEntryCount() {
  const el = document.getElementById("entry-count");
  const n = allEntries.length;
  const word = n % 10 === 1 && n % 100 !== 11 ? "запись" : n % 10 >= 2 && n % 10 <= 4 && (n % 100 < 10 || n % 100 >= 20) ? "записи" : "записей";
  el.textContent = `${n} ${word}`;
}

// Purely cosmetic HUD countdown mirroring the server-enforced auto-lock
// timeout; the real enforcement happens in src-tauri regardless of this tab
// staying accurate. Resets on any user interaction with the dashboard.
setInterval(() => {
  if (dashboard.classList.contains("hidden")) return;
  sessionSecondsLeft = Math.max(0, sessionSecondsLeft - 1);
  const m = Math.floor(sessionSecondsLeft / 60).toString().padStart(2, "0");
  const s = (sessionSecondsLeft % 60).toString().padStart(2, "0");
  document.getElementById("session-timer").textContent = `${m}:${s}`;
}, 1000);
["click", "keydown"].forEach((evt) => document.addEventListener(evt, resetSessionTimer));

function setStatus(text) {
  status.textContent = text;
  if (text) setTimeout(() => { if (status.textContent === text) status.textContent = ""; }, 3000);
}

async function refreshEntries() {
  try {
    allEntries = await invoke("list_entries");
    renderEntryCount();
    updateCategoryOptions();
    applyFilters();
  } catch (e) {
    showLockScreen("Хранилище заблокировано по таймауту, введите пароль снова.");
  }
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

let currentlyRendered = [];

function renderEntries(entries) {
  currentlyRendered = entries;
  entriesBody.innerHTML = "";
  const visibleIds = new Set(entries.map((e) => e.id));
  // Drop selections that scrolled out of the current filter, so "select all"
  // + export never silently includes rows the user can no longer see.
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

function escapeHtml(s) {
  return String(s ?? "").replace(/[&<>"']/g, (c) => ({
    "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
  }[c]));
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

document.getElementById("btn-open").addEventListener("click", async () => {
  const path = document.getElementById("vault-path").value.trim();
  const password = document.getElementById("master-password").value;
  try {
    await invoke("open_vault", { path, password });
    showDashboard();
  } catch (e) {
    lockError.textContent = String(e);
  }
});

document.getElementById("btn-create").addEventListener("click", async () => {
  const path = document.getElementById("vault-path").value.trim();
  const password = document.getElementById("master-password").value;
  if (password.length < 8) {
    lockError.textContent = "Мастер-пароль должен быть не короче 8 символов.";
    return;
  }
  try {
    await invoke("create_vault", { path, password });
    showDashboard();
  } catch (e) {
    lockError.textContent = String(e);
  }
});

document.getElementById("btn-lock").addEventListener("click", async () => {
  await invoke("lock_vault");
  showLockScreen();
});

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
      await invoke("update_entry", { id: editingId, entry });
    } else {
      await invoke("add_entry", { entry });
    }
    closeModal();
    refreshEntries();
  } catch (e) {
    setStatus("Ошибка: " + e);
  }
});

document.getElementById("btn-delete-entry").addEventListener("click", async () => {
  if (editingId == null) return;
  await invoke("delete_entry", { id: editingId });
  closeModal();
  refreshEntries();
});

document.getElementById("btn-backup").addEventListener("click", async () => {
  const destDir = await dialog.open({ directory: true, title: "Куда сохранить резервную копию" });
  if (!destDir) return;
  try {
    const path = await invoke("export_backup", { destDir });
    setStatus("Резервная копия сохранена: " + path);
  } catch (e) {
    setStatus("Ошибка резервного копирования: " + e);
  }
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
    const written = await invoke("export_csv", { ids: [...selectedIds], destPath });
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
    const count = await invoke("import_csv", { path });
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
  const kdbxPassword = await showPromptModal("Пароль от базы KeePass (не ваш мастер-пароль CIPHERDEN)", { password: true });
  if (kdbxPassword == null) return;
  try {
    const count = await invoke("import_kdbx", { path, kdbxPassword });
    setStatus(`Импортировано записей: ${count}`);
    refreshEntries();
  } catch (e) {
    setStatus("Ошибка импорта KeePass: " + e);
  }
});

// Poll for server-enforced auto-lock so the UI reflects it even with no
// user-initiated command in flight.
setInterval(async () => {
  if (dashboard.classList.contains("hidden")) return;
  const unlocked = await invoke("is_unlocked");
  if (!unlocked) showLockScreen("Хранилище автоматически заблокировано по таймауту бездействия.");
}, 10000);

// --- Zone switching (Accounts / Files) -----------------------------------

document.querySelectorAll(".zone-tab").forEach((tab) => {
  tab.addEventListener("click", () => {
    document.querySelectorAll(".zone-tab").forEach((t) => t.classList.remove("active"));
    tab.classList.add("active");
    const zone = tab.dataset.zone;
    document.getElementById("zone-accounts").classList.toggle("hidden", zone !== "accounts");
    document.getElementById("zone-files").classList.toggle("hidden", zone !== "files");
    if (zone === "files") refreshFilesIfUnlocked();
  });
});

// --- Files zone: folders, icon-grid "desktop", drag-and-drop --------------

const filesLock = document.getElementById("files-lock");
const filesContent = document.getElementById("files-content");
const filesLockError = document.getElementById("files-lock-error");
const filesGrid = document.getElementById("files-grid");
const filesBreadcrumb = document.getElementById("files-breadcrumb");
const filesStatus = document.getElementById("files-status");

// path = the chain of folders from root to the current folder (root itself
// is not in this array). Empty path = at the root.
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

function showFilesUnlocked() {
  filesLock.classList.add("hidden");
  filesContent.classList.remove("hidden");
  filesPath = [];
  refreshFiles();
}

function showFilesLocked(message) {
  filesContent.classList.add("hidden");
  filesLock.classList.remove("hidden");
  filesLockError.textContent = message || "";
}

async function refreshFilesIfUnlocked() {
  try {
    if (await invoke("is_file_vault_unlocked")) showFilesUnlocked();
  } catch {
    // ignore — stays on the lock form
  }
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
      invoke("list_folders", { parentId: folderId }),
      invoke("list_files", { folderId }),
    ]);
    renderBreadcrumb();
    renderFilesGrid(folders, files);
  } catch (e) {
    showFilesLocked("Хранилище файлов заблокировано по таймауту, введите пароль снова.");
  }
}

function renderFilesGrid(folders, files) {
  filesGrid.innerHTML = "";

  if (filesPath.length > 0) {
    filesGrid.appendChild(makeUpTile());
  }
  for (const f of folders) filesGrid.appendChild(makeFolderTile(f));
  for (const f of files) filesGrid.appendChild(makeFileTile(f));

  if (folders.length === 0 && files.length === 0 && filesPath.length === 0) {
    const hint = document.createElement("div");
    hint.className = "files-empty-hint";
    hint.textContent = "Пусто. Перетащите файлы сюда через «+ Добавить файлы» или создайте папку.";
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
    if (dragged.type === "file") await invoke("move_file", { id: dragged.id, folderId: targetParent });
    else await invoke("move_folder", { id: dragged.id, newParentId: targetParent });
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
    await invoke("set_folder_pinned", { id: folder.id, pinned: !folder.pinned });
    refreshFiles();
  });
  el.querySelector(".delete-btn").addEventListener("click", async (e) => {
    e.stopPropagation();
    if (!(await showConfirmModal(`Удалить папку «${folder.name}»? Она должна быть пустой.`))) return;
    try {
      await invoke("delete_folder", { id: folder.id });
      refreshFiles();
    } catch (err) {
      setFilesStatus("Ошибка: " + err);
    }
  });

  setDragSource(el, { type: "folder", id: folder.id });
  setDropTarget(el, async (dragged) => {
    if (dragged.type === "folder" && dragged.id === folder.id) return;
    try {
      if (dragged.type === "file") await invoke("move_file", { id: dragged.id, folderId: folder.id });
      else await invoke("move_folder", { id: dragged.id, newParentId: folder.id });
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
    await invoke("set_file_pinned", { id: file.id, pinned: !file.pinned });
    refreshFiles();
  });
  el.querySelector(".extract-btn").addEventListener("click", (e) => {
    e.stopPropagation();
    extractFile(file);
  });
  el.querySelector(".delete-btn").addEventListener("click", async (e) => {
    e.stopPropagation();
    if (!(await showConfirmModal(`Удалить файл «${file.name}»?`))) return;
    await invoke("delete_file", { id: file.id });
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
    await invoke("extract_file", { id: file.id, destPath });
    setFilesStatus("Файл сохранён: " + destPath);
  } catch (e) {
    setFilesStatus("Ошибка: " + e);
  }
}

// --- In-app preview: view a file's contents without ever writing a
// decrypted copy to the host disk. Bytes come back base64-encoded over IPC
// and are turned into a blob: URL entirely in memory.

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
    preview = await invoke("read_file_preview", { id: file.id });
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
    await invoke("create_folder", { parentId: currentFolderId(), name });
    refreshFiles();
  } catch (e) {
    setFilesStatus("Ошибка: " + e);
  }
});

document.getElementById("btn-files-open").addEventListener("click", async () => {
  const path = document.getElementById("files-path").value.trim();
  const password = document.getElementById("files-password").value;
  try {
    await invoke("open_file_vault", { path, password });
    showFilesUnlocked();
  } catch (e) {
    filesLockError.textContent = String(e);
  }
});

document.getElementById("btn-files-create").addEventListener("click", async () => {
  const path = document.getElementById("files-path").value.trim();
  const password = document.getElementById("files-password").value;
  if (password.length < 8) {
    filesLockError.textContent = "Пароль должен быть не короче 8 символов.";
    return;
  }
  try {
    await invoke("create_file_vault", { path, password });
    showFilesUnlocked();
  } catch (e) {
    filesLockError.textContent = String(e);
  }
});

document.getElementById("btn-files-lock").addEventListener("click", async () => {
  await invoke("lock_file_vault");
  showFilesLocked();
});

document.getElementById("btn-file-add").addEventListener("click", async () => {
  const paths = await dialog.open({ multiple: true, title: "Выберите файлы для добавления" });
  if (!paths) return;
  const list = Array.isArray(paths) ? paths : [paths];
  try {
    const added = await invoke("add_files", { folderId: currentFolderId(), paths: list });
    setFilesStatus(`Добавлено файлов: ${added}`);
    refreshFiles();
  } catch (e) {
    setFilesStatus("Ошибка добавления: " + e);
  }
});

document.getElementById("btn-files-backup").addEventListener("click", async () => {
  const destDir = await dialog.open({ directory: true, title: "Куда сохранить резервную копию" });
  if (!destDir) return;
  try {
    const path = await invoke("export_file_vault_backup", { destDir });
    setFilesStatus("Резервная копия сохранена: " + path);
  } catch (e) {
    setFilesStatus("Ошибка резервного копирования: " + e);
  }
});

showLockScreen();
