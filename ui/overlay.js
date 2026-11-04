const { listen } = window.__TAURI__.event;

const pill = document.getElementById("pill");
const bars = document.getElementById("bars");
const text = document.getElementById("text");
const time = document.getElementById("time");

const BAR_COUNT = 11;
const history = new Array(BAR_COUNT).fill(0.12);
for (let i = 0; i < BAR_COUNT; i += 1) {
  bars.append(document.createElement("i"));
}

let view = { view: "hidden" };
let notice = null;

function clock(seconds) {
  const minutes = String(Math.floor(seconds / 60)).padStart(2, "0");
  return `${minutes}:${String(seconds % 60).padStart(2, "0")}`;
}

function render() {
  pill.hidden = view.view === "hidden" && !notice;
  pill.classList.toggle("recording", view.view === "recording" && !view.waiting);
  pill.classList.toggle("waiting", Boolean(view.waiting));
  pill.classList.toggle("thinking", view.view === "transcribing" && !view.waiting);

  const recording = view.view === "recording";
  bars.hidden = !recording || view.waiting || Boolean(notice);
  time.hidden = !recording || Boolean(notice);
  time.textContent = clock(view.elapsed ?? 0);

  if (notice) {
    text.textContent = notice;
  } else if (view.waiting) {
    text.textContent = "Matn kutmoqda";
  } else if (view.view === "starting") {
    text.textContent = "Tayyorlanmoqda";
  } else if (view.view === "transcribing") {
    text.textContent = "Matnga oʻgirilmoqda";
  } else {
    text.textContent = "";
  }
  text.hidden = text.textContent === "";
}

listen("overlay", ({ payload }) => {
  if ("level" in payload) {
    history.shift();
    history.push(Math.max(0.12, Math.min(1, Math.sqrt(payload.level) * 2.4)));
    [...bars.children].forEach((bar, index) => {
      bar.style.transform = `scaleY(${history[index]})`;
    });
    return;
  }
  if ("notice" in payload) {
    notice = payload.notice;
  } else {
    view = payload;
  }
  render();
});
