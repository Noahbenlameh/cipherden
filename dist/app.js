const invoke = window.__TAURI__.core.invoke;

const lockScreen = document.getElementById("lock-screen");
const dashboard = document.getElementById("dashboard");
const lockError = document.getElementById("lock-error");
const status = document.getElementById("status");
const entriesBody = document.getElementById("entries-body");
const modal = document.getElementById("entry-modal");

let editingId = null;

function showDashboard() {
  lockScreen.classList.add("hidden");
  dashboard.classList.remove("hidden");
  refreshEntries();
}

function showLockScreen(message) {
  dashboard.classList.add("hidden");
  lockScreen.classList.remove("hidden");
  lockError.textContent = message || "";
}

function setStatus(text) {
  status.textContent = text;
  if (text) setTimeout(() => { if (status.textContent === text) status.textContent = ""; }, 3000);
}

async function refreshEntries(query) {
  try {
    const entries = query
      ? await invoke("search_entries", { query })
      : await invoke("list_entries");
    renderEntries(entries);
  } catch (e) {
    showLockScreen("Хранилище заблокировано по таймауту, введите пароль снова.");
  }
}

function renderEntries(entries) {
  entriesBody.innerHTML = "";
  for (const e of entries) {
    const tr = document.createElement("tr");
    tr.innerHTML = `
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

document.getElementById("search").addEventListener("input", (e) => {
  refreshEntries(e.target.value.trim() || undefined);
});

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
  const destDir = window.prompt("Папка для резервной копии (например, путь на втором носителе):", "./backup");
  if (!destDir) return;
  try {
    const path = await invoke("export_backup", { destDir });
    setStatus("Резервная копия сохранена: " + path);
  } catch (e) {
    setStatus("Ошибка резервного копирования: " + e);
  }
});

// Poll for server-enforced auto-lock so the UI reflects it even with no
// user-initiated command in flight.
setInterval(async () => {
  if (dashboard.classList.contains("hidden")) return;
  const unlocked = await invoke("is_unlocked");
  if (!unlocked) showLockScreen("Хранилище автоматически заблокировано по таймауту бездействия.");
}, 10000);

showLockScreen();
