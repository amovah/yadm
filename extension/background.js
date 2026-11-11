// Hands browser downloads to YADM over its localhost bridge.
const DEFAULTS = { enabled: true, port: 17863, token: "", minSizeMB: 0, skipTypes: "" };

async function config() {
  return { ...DEFAULTS, ...(await chrome.storage.sync.get(DEFAULTS)) };
}

async function cookiesFor(url) {
  try {
    const list = await chrome.cookies.getAll({ url });
    return list.map((c) => `${c.name}=${c.value}`).join("; ");
  } catch {
    return "";
  }
}

function browserName() {
  const ua = navigator.userAgent;
  if (ua.includes("Firefox/")) return "Firefox";
  if (ua.includes("Edg/")) return "Edge";
  if (ua.includes("OPR/")) return "Opera";
  if (ua.includes("Brave")) return "Brave";
  return "Chrome";
}

// Asks YADM for the token; YADM shows an Allow/Deny dialog. One request at a time.
// After a Deny, only the options page's Connect button (`manual`) asks again.
let pairing = null;
function pair(manual = false) {
  pairing ??= (async () => {
    try {
      const cfg = await config();
      const { pairDenied } = await chrome.storage.local.get({ pairDenied: false });
      if (pairDenied && !manual) throw new Error("Not connected: press Connect in the extension's options.");
      const res = await fetch(`http://127.0.0.1:${cfg.port}/v1/pair`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ browser: browserName() }),
      });
      if (res.status === 403) {
        await chrome.storage.local.set({ pairDenied: true });
        throw new Error("Connection was denied in YADM.");
      }
      if (!res.ok) throw new Error(`YADM answered ${res.status}`);
      const { token } = await res.json();
      await chrome.storage.sync.set({ token });
      await chrome.storage.local.set({ pairDenied: false });
      return token;
    } finally {
      pairing = null;
    }
  })();
  return pairing;
}

async function post(cfg, token, payload) {
  return fetch(`http://127.0.0.1:${cfg.port}/v1/downloads`, {
    method: "POST",
    headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
    body: JSON.stringify(payload),
  });
}

async function send(payload) {
  const cfg = await config();
  // Not paired yet, or YADM's token changed: pair (the user confirms in YADM) and retry.
  let res = cfg.token ? await post(cfg, cfg.token, payload) : null;
  if (!res || res.status === 401) res = await post(cfg, await pair(), payload);
  if (res.status !== 202) throw new Error(`YADM answered ${res.status}`);
}

async function buildPayload(url, extra = {}) {
  return {
    url,
    cookies: url.startsWith("http") ? await cookiesFor(url) : "",
    userAgent: navigator.userAgent,
    ...extra,
  };
}

function shouldSkip(item, cfg) {
  if (!/^(https?|ftp):/.test(item.finalUrl || item.url)) return true; // blob:, data:
  if (cfg.minSizeMB > 0 && item.fileSize > 0 && item.fileSize < cfg.minSizeMB * 1024 * 1024) return true;
  const skip = cfg.skipTypes.split(",").map((s) => s.trim().toLowerCase()).filter(Boolean);
  const name = (item.filename || item.finalUrl || "").toLowerCase();
  return skip.some((ext) => name.endsWith("." + ext.replace(/^\./, "")));
}

chrome.downloads.onCreated.addListener(async (item) => {
  const cfg = await config();
  if (!cfg.enabled || shouldSkip(item, cfg)) return;
  const url = item.finalUrl || item.url;
  try {
    const payload = await buildPayload(url, {
      filename: item.filename ? item.filename.split(/[\\/]/).pop() : undefined,
      referrer: item.referrer || undefined,
      fileSize: item.fileSize > 0 ? item.fileSize : undefined,
      mime: item.mime || undefined,
    });
    await send(payload);
    // YADM took it: cancel the browser's copy.
    await chrome.downloads.cancel(item.id);
    await chrome.downloads.erase({ id: item.id });
  } catch (e) {
    // YADM isn't running or rejected it; let the browser download normally.
    console.warn("YADM unavailable, browser keeps the download:", e);
  }
});

chrome.runtime.onInstalled.addListener(async () => {
  chrome.contextMenus.create({ id: "yadm-link", title: "Download with YADM", contexts: ["link", "video", "audio", "image"] });
  // Connect right away if YADM is running; otherwise the first download does it.
  if (!(await config()).token) pair().catch(() => {});
});

// The options page's "Connect" button.
chrome.runtime.onMessage.addListener((msg, _sender, respond) => {
  if (msg?.type !== "pair") return;
  pair(true).then(() => respond({ ok: true }), (e) => respond({ ok: false, error: String(e.message || e) }));
  return true;
});

chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  const url = info.linkUrl || info.srcUrl;
  if (!url) return;
  try {
    await send(await buildPayload(url, { referrer: tab?.url }));
  } catch (e) {
    chrome.notifications.create({ type: "basic", iconUrl: "icon.png", title: "YADM", message: String(e.message || e) });
  }
});
