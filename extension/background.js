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

async function send(payload) {
  const cfg = await config();
  if (!cfg.token) throw new Error("Pair the extension with YADM first (Options).");
  const res = await fetch(`http://127.0.0.1:${cfg.port}/v1/downloads`, {
    method: "POST",
    headers: { "Content-Type": "application/json", Authorization: `Bearer ${cfg.token}` },
    body: JSON.stringify(payload),
  });
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
  if (!cfg.enabled || !cfg.token || shouldSkip(item, cfg)) return;
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

chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.create({ id: "yadm-link", title: "Download with YADM", contexts: ["link", "video", "audio", "image"] });
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
