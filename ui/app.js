"use strict";
const $ = (id) => document.getElementById(id);
const tauri = window.__TAURI__;
const preview = !tauri;
const invoke = (command, args) => preview
  ? Promise.reject(new Error("Preview only. Open the desktop app to manage accounts."))
  : tauri.core.invoke(command, args);
let accounts = [];
let activeUser = null;
let busy = false;
let loadFailed = false;
let toastTimer;
let dialogAction;

function toast(message, error = false) {
  clearTimeout(toastTimer);
  $("toast").textContent = String(message);
  $("toast").classList.toggle("error", error);
  $("toast").hidden = false;
  toastTimer = setTimeout(() => { $("toast").hidden = true; }, 6500);
}

function updateDisabled() {
  $("add-input").disabled = busy || loadFailed || preview;
  $("add-btn").disabled = busy || loadFailed || preview;
  $("clear-all-btn").disabled = busy || loadFailed || preview || !accounts.length;
  $("logout-btn").disabled = busy || loadFailed || preview;
  document.querySelectorAll(".account-actions button").forEach(b => {
    b.disabled = busy || preview || (b.classList.contains("login-btn") && b.dataset.expired === "true");
  });
}

async function run(action) {
  if (busy) return false;
  busy = true;
  updateDisabled();
  try { await action(); return true; }
  catch (error) { toast(String(error), true); return false; }
  finally { busy = false; updateDisabled(); }
}

function render() {
  $("acct-count").textContent = accounts.length;
  const query = $("search").value.trim().toLowerCase();
  const shown = accounts.filter(a => `${a.label} ${a.username} ${a.steamid}`.toLowerCase().includes(query));
  $("account-list").replaceChildren(...shown.map(buildCard));
  $("empty-state").hidden = accounts.length > 0 || loadFailed;
  $("no-results").hidden = !accounts.length || shown.length > 0;
  updateDisabled();
}

function buildCard(account) {
  const card = $("card-tpl").content.firstElementChild.cloneNode(true);
  const name = account.label || account.username;
  const selected = account.username.toLowerCase() === (activeUser || "").toLowerCase();
  const expired = account.expires_at != null && account.expires_at <= Date.now() / 1000;
  card.querySelector("h3").textContent = name;
  card.querySelector("h3").title = name;
  card.querySelector(".account-avatar").textContent = name.slice(0, 2).toUpperCase();
  card.querySelector(".account-id").textContent = `${account.username} · ${account.steamid}`;
  card.classList.toggle("is-selected", selected);
  card.querySelector(".selected").hidden = !selected;
  const status = card.querySelector(".token-status");
  status.textContent = expired ? "Token expired · add a fresh token" : account.expires_at
    ? `Token expiry: ${new Date(account.expires_at * 1000).toLocaleDateString()} · unverified`
    : "Token saved · expiry unknown · unverified";
  status.classList.toggle("expired", expired);
  const load = card.querySelector(".login-btn");
  load.dataset.expired = String(expired);
  load.setAttribute("aria-label", `Load ${name}`);
  load.onclick = () => confirmAction("Load account?", `Steam and its running processes will close. Save any game progress first. Steam will then start with ${account.username}.`, "Load account", async () => {
    const message = await invoke("login", { steamid: account.steamid });
    activeUser = await invoke("active_user");
    render(); toast(message);
  });
  card.querySelector(".rename-btn").onclick = () => rename(account);
  card.querySelector(".remove-btn").onclick = () => confirmAction("Remove account?", `Remove ${name} from this loader? This does not revoke its token or remove saved Steam sessions.`, "Remove", async () => {
    await invoke("remove_account", { steamid: account.steamid });
    accounts = accounts.filter(a => a.steamid !== account.steamid);
    render(); toast("Account removed from the loader.");
  });
  return card;
}

function confirmAction(title, copy, label, action, value) {
  $("dialog-title").textContent = title;
  $("dialog-copy").textContent = copy;
  $("dialog-confirm").textContent = label;
  $("dialog-error").hidden = true;
  $("rename-input").hidden = value === undefined;
  $("rename-label").hidden = value === undefined;
  $("rename-input").value = value ?? "";
  dialogAction = action;
  $("dialog").showModal();
  if (value !== undefined) { $("rename-input").focus(); $("rename-input").select(); }
  else $("dialog-cancel").focus();
}

function rename(account) {
  confirmAction("Rename account", "Choose a display name. Your Steam login name stays the same. Leave blank to reset it.", "Save", async () => {
    const name = $("rename-input").value.trim();
    await invoke("rename_account", { steamid: account.steamid, name });
    account.label = name;
    render(); toast("Display name saved.");
  }, account.label || "");
}

$("add-form").addEventListener("submit", event => {
  event.preventDefault();
  const line = $("add-input").value.trim();
  if (!line) return;
  run(async () => {
    const account = await invoke("add_account", { line });
    const index = accounts.findIndex(a => a.steamid === account.steamid);
    if (index < 0) accounts.push(account); else accounts[index] = account;
    $("add-input").value = "";
    $("search").value = "";
    render(); toast(index < 0 ? "Account added." : "Account token updated.");
  });
});
$("search").addEventListener("input", render);
$("dialog-cancel").onclick = () => { if (!busy) $("dialog").close(); };
$("dialog").addEventListener("cancel", event => { if (busy) event.preventDefault(); });
$("dialog-form").addEventListener("submit", async event => {
  event.preventDefault();
  if (busy) return;
  $("dialog-confirm").disabled = true;
  $("dialog-cancel").disabled = true;
  const success = await run(async () => {
    try { await dialogAction(); }
    catch (error) { $("dialog-error").textContent = String(error); $("dialog-error").hidden = false; throw error; }
  });
  $("dialog-confirm").disabled = false;
  $("dialog-cancel").disabled = false;
  if (success) $("dialog").close();
});
$("clear-all-btn").onclick = () => confirmAction("Remove all accounts?", "Remove every account from this loader? Steam's saved sessions will remain. You will need your tokens to add these accounts again.", "Remove all", async () => {
  await invoke("clear_all"); accounts = []; render(); toast("All accounts removed from the loader.");
});
$("logout-btn").onclick = () => confirmAction("Close Steam / sign out?", "Save your game progress first. This closes Steam and clears automatic account selection. It does not revoke tokens or erase saved Steam sessions.", "Close Steam", async () => {
  const message = await invoke("logout"); activeUser = null; render(); toast(message);
});
$("min-btn").onclick = () => tauri?.window.getCurrentWindow().minimize();
$("close-btn").onclick = () => tauri?.window.getCurrentWindow().close();

async function init() {
  if (preview) {
    $("preview-note").hidden = false;
    $("min-btn").disabled = true;
    $("close-btn").disabled = true;
    if (new URLSearchParams(location.search).has("preview")) {
      accounts = [
        { username: "personal", label: "Main account", steamid: "76561198012345678", expires_at: 2000000000 },
        { username: "second_account", label: "", steamid: "76561198087654321", expires_at: null },
      ];
      activeUser = "personal";
    }
  } else {
    try { const data = await invoke("bootstrap"); accounts = data.accounts; activeUser = data.active_user; }
    catch (error) { loadFailed = true; $("load-error").textContent = String(error); $("load-error").hidden = false; }
  }
  render();
}
init();
