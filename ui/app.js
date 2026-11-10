const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (id) => document.getElementById(id);
const WINDOWS = navigator.userAgent.includes("Windows");
const LINUX = navigator.userAgent.includes("Linux");
const SYMBOLS = WINDOWS ? { Cmd: "Win" } : LINUX ? { Cmd: "Super" } : { Cmd: "⌘", Shift: "⇧", Alt: "⌥", Ctrl: "⌃" };

// Windows and Linux have no fn key (right Ctrl stands in), and Windows has no Accessibility grant.
if (WINDOWS || LINUX) {
  $("try").placeholder = "Kursorni shu yerga qoʻying, oʻng Ctrl ni 1 soniya bosib turing va gapiring";
  document.querySelectorAll(".hold-key").forEach((element) => (element.textContent = "oʻng Ctrl"));
}
if (WINDOWS) {
  document.querySelectorAll(".mac-only").forEach((element) => (element.style.display = "none"));
}
// Linux needs keyboard device access once: a terminal command, then a new login.
if (LINUX) {
  $("step-access").querySelector("h3").textContent = "Klaviatura ruxsati";
  $("step-access").querySelector("p").textContent =
    "Buyruqni nusxalang, terminalda ishga tushiring, keyin tizimdan chiqib qayta kiring. Bu tugmalarni eshitish va matnni yozish uchun kerak.";
  $("access-action").textContent = "Buyruqni nusxalash";
}

let status = null;
let capturing = false;
let downloadError = null;

function megabytes(bytes) {
  return Math.round(bytes / 1_000_000);
}

function renderSteps() {
  const steps = [
    ["step-mic", status.mic === "granted"],
    ["step-access", status.accessibility],
    ["step-model", status.model_present],
  ].filter(([id]) => !WINDOWS || id !== "step-access");
  const firstOpen = steps.findIndex(([, done]) => !done);
  steps.forEach(([id, done], index) => {
    $(id).classList.toggle("done", done);
    $(id).classList.toggle("current", index === firstOpen);
    $(id).querySelector(".marker").textContent = done ? "✓" : String(index + 1);
    $(id).querySelector(".action").hidden = done || index !== firstOpen;
  });

  const label = status.mic === "denied" ? "Sozlamani ochish" : "Ruxsat berish";
  $("mic-action").textContent = label;

  const action = $("model-action");
  action.disabled = status.downloading;
  action.textContent = downloadError ? "Qayta yuklash" : "Yuklab olish";
  if (!status.downloading && !downloadError) {
    $("model-text").textContent =
      `Taxminan ${megabytes(status.model_size)} MB. Bir marta yuklanadi, keyin internetsiz ishlaydi.`;
  }
}

function renderKeys(text) {
  const box = $("toggle-keys");
  box.replaceChildren();
  for (const part of text.split("+")) {
    const key = document.createElement("kbd");
    key.className = "key";
    key.textContent = SYMBOLS[part] ?? part;
    box.append(key);
  }
}

function renderSettings() {
  const { settings } = status;
  if (!capturing) {
    renderKeys(settings.toggle_hotkey);
  }
  $("fn-keys").style.opacity = settings.fn_enabled ? 1 : 0.4;
  $("fn-enabled").checked = settings.fn_enabled;
  $("autostart").checked = settings.autostart;
  $("sounds").checked = settings.sounds;
  $("update-check").checked = settings.auto_update;
  $("auto-send").checked = settings.auto_send;
  $("mic").value = settings.mic_device ?? "";
  $("version").textContent = `Gapir ${status.version}`;
}

async function refresh() {
  status = await invoke("get_status");
  const ready = status.accessibility && status.mic === "granted" && status.model_present;
  $("setup").hidden = ready;
  $("ready").hidden = !ready;
  if (ready) {
    renderSettings();
  } else {
    renderSteps();
  }
}

async function loadMics() {
  const select = $("mic");
  const auto = new Option("Avtomatik", "");
  select.replaceChildren(auto, ...(await invoke("list_mics")).map((mic) => new Option(mic.name, mic.id)));
}

async function save(changes) {
  try {
    await invoke("save_settings", { settings: { ...status.settings, ...changes } });
    await refresh();
    return true;
  } catch (message) {
    $("toggle-text").firstChild.textContent = `${message}. `;
    return false;
  }
}

function hotkeyFrom(event) {
  const code = event.code;
  let key = null;
  if (code.startsWith("Key")) key = code.slice(3);
  else if (code.startsWith("Digit")) key = code.slice(5);
  else if (/^F\d+$/.test(code) || code === "Space") key = code;

  const modifiers = [];
  if (event.ctrlKey) modifiers.push("Ctrl");
  if (event.altKey) modifiers.push("Alt");
  if (event.shiftKey) modifiers.push("Shift");
  if (event.metaKey) modifiers.push("Cmd");
  const hasStrongModifier = event.ctrlKey || event.altKey || event.metaKey;
  return key && hasStrongModifier ? [...modifiers, key].join("+") : null;
}

const TOGGLE_HELP = "Bir bosish boshlaydi, yana bosish toʻxtatadi. ";

async function stopCapture() {
  capturing = false;
  window.removeEventListener("keydown", onCaptureKey, true);
  window.removeEventListener("blur", stopCapture);
  $("toggle-keys").classList.remove("capture");
  $("toggle-text").firstChild.textContent = TOGGLE_HELP;
  await invoke("pause_hotkeys", { paused: false });
  await refresh();
}

async function onCaptureKey(event) {
  event.preventDefault();
  event.stopPropagation();
  if (event.code === "Escape") {
    await stopCapture();
    return;
  }
  const hotkey = hotkeyFrom(event);
  if (hotkey) {
    await invoke("pause_hotkeys", { paused: false });
    capturing = false;
    window.removeEventListener("keydown", onCaptureKey, true);
    window.removeEventListener("blur", stopCapture);
    $("toggle-keys").classList.remove("capture");
    if (await save({ toggle_hotkey: hotkey })) {
      $("toggle-text").firstChild.textContent = TOGGLE_HELP;
    }
    await refresh();
  }
}

async function startCapture() {
  capturing = true;
  $("toggle-keys").replaceChildren();
  $("toggle-keys").classList.add("capture");
  $("toggle-text").firstChild.textContent = "Yangi birikmani bosing. Esc bekor qiladi. ";
  await invoke("pause_hotkeys", { paused: true });
  window.addEventListener("keydown", onCaptureKey, true);
  window.addEventListener("blur", stopCapture);
}

$("mic-action").addEventListener("click", () => invoke("request_permission", { kind: "mic" }));
$("access-action").addEventListener("click", () => invoke("request_permission", { kind: "accessibility" }));
$("change-hotkey").addEventListener("click", startCapture);

$("model-action").addEventListener("click", async () => {
  downloadError = null;
  $("model-progress").hidden = false;
  $("model-text").textContent = "Yuklanmoqda...";
  await invoke("download_model");
  await refresh();
});

$("mic").addEventListener("change", (event) => save({ mic_device: event.target.value || null }));
$("fn-enabled").addEventListener("change", (event) => save({ fn_enabled: event.target.checked }));
$("autostart").addEventListener("change", (event) => save({ autostart: event.target.checked }));
$("sounds").addEventListener("change", (event) => save({ sounds: event.target.checked }));
$("update-check").addEventListener("change", (event) => save({ auto_update: event.target.checked }));
$("auto-send").addEventListener("change", (event) => save({ auto_send: event.target.checked }));

listen("model-progress", ({ payload }) => {
  const percent = (payload.done / payload.total) * 100;
  $("model-progress").hidden = false;
  $("model-progress").firstElementChild.style.width = `${percent}%`;
  $("model-text").textContent = `${megabytes(payload.done)} / ${megabytes(payload.total)} MB`;
});

listen("model-done", async ({ payload }) => {
  downloadError = payload.error;
  $("model-progress").hidden = true;
  if (downloadError) {
    $("model-text").textContent = "Yuklab boʻlmadi. Internetni tekshirib, qayta urinib koʻring.";
    $("model-text").classList.add("error");
  } else {
    $("model-text").classList.remove("error");
  }
  await refresh();
  await loadMics();
});

// Cards light up under the pointer (see `.card::before`).
document.addEventListener("pointermove", (event) => {
  const card = event.target.closest?.(".card");
  if (!card) return;
  const box = card.getBoundingClientRect();
  card.style.setProperty("--x", `${event.clientX - box.left}px`);
  card.style.setProperty("--y", `${event.clientY - box.top}px`);
});

loadMics().then(refresh);
setInterval(refresh, 1500);
