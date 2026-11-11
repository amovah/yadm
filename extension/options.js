const DEFAULTS = { enabled: true, port: 17863, token: "", minSizeMB: 0, skipTypes: "" };
const $ = (id) => document.getElementById(id);
const status = (t) => ($("status").textContent = t);

async function load() {
  const c = await chrome.storage.sync.get(DEFAULTS);
  $("enabled").checked = c.enabled;
  $("token").value = c.token;
  $("port").value = c.port;
  $("minSizeMB").value = c.minSizeMB;
  $("skipTypes").value = c.skipTypes;
}

function read() {
  return {
    enabled: $("enabled").checked,
    token: $("token").value.trim(),
    port: Number($("port").value) || DEFAULTS.port,
    minSizeMB: Number($("minSizeMB").value) || 0,
    skipTypes: $("skipTypes").value,
  };
}

$("save").onclick = async () => {
  await chrome.storage.sync.set(read());
  status("Saved");
};

async function ping(c) {
  try {
    const r = await fetch(`http://127.0.0.1:${c.port}/v1/ping`, { headers: { Authorization: `Bearer ${c.token}` } });
    return r.ok ? "ok" : r.status === 401 ? "unpaired" : "error";
  } catch {
    return "offline";
  }
}

async function showPairing() {
  const c = await chrome.storage.sync.get(DEFAULTS);
  $("token").value = c.token;
  const state = c.token ? await ping(c) : "unpaired";
  $("pairState").textContent = {
    ok: "Connected to YADM",
    unpaired: "Not connected",
    offline: "YADM isn't running",
    error: "YADM didn't answer",
  }[state];
  $("connect").hidden = state !== "unpaired";
}

$("connect").onclick = async () => {
  $("connect").disabled = true;
  $("pairState").textContent = "Confirm in YADM…";
  const r = await chrome.runtime.sendMessage({ type: "pair" });
  $("connect").disabled = false;
  if (r?.ok) await showPairing();
  else $("pairState").textContent = r?.error?.includes("Failed to fetch") || r?.error?.includes("NetworkError") ? "YADM isn't running" : r?.error || "Couldn't connect";
};

$("test").onclick = async () => {
  const c = read();
  try {
    const r = await fetch(`http://127.0.0.1:${c.port}/v1/ping`, { headers: { Authorization: `Bearer ${c.token}` } });
    status(r.ok ? "Connected to YADM" : r.status === 401 ? "Wrong token" : `Error ${r.status}`);
  } catch {
    status("YADM isn't running");
  }
};

load();
showPairing();
