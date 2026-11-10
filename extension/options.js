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
