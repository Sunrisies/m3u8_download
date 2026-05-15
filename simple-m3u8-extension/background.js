const tabLinks = new Map();
const BACKEND_URL = "http://localhost:8080";

function normalizeUrl(url) {
  try {
    return new URL(url).href;
  } catch (_) {
    return "";
  }
}

function isM3u8Url(url) {
  return /\.m3u8(?:[?#]|$)/i.test(url);
}

function rememberLink(tabId, url) {
  if (tabId < 0 || !url) return;

  const normalized = normalizeUrl(url);
  if (!normalized || !isM3u8Url(normalized)) return;

  const links = tabLinks.get(tabId) || [];
  if (links.includes(normalized)) return;

  links.push(normalized);
  tabLinks.set(tabId, links);

  chrome.tabs
    .sendMessage(tabId, { type: "M3U8_LINKS_UPDATED", links })
    .catch(() => {
      // The content script may not be available on browser pages or restricted sites.
    });
}

function sanitizeFileName(name) {
  return (name || "m3u8_video")
    .replace(/[\\/:*?"<>|]/g, "_")
    .replace(/\s+/g, "_")
    .slice(0, 80);
}

function taskNameFromUrl(url) {
  try {
    const parsed = new URL(url);
    const last = parsed.pathname.split("/").filter(Boolean).pop() || "m3u8_video";
    return sanitizeFileName(decodeURIComponent(last).replace(/\.m3u8$/i, ""));
  } catch (_) {
    return `m3u8_video_${Date.now()}`;
  }
}

async function requestJson(url, options) {
  const response = await fetch(url, options);
  const text = await response.text();
  let data = {};

  try {
    data = text ? JSON.parse(text) : {};
  } catch (_) {
    data = { message: text };
  }

  if (!response.ok) {
    throw new Error(data.error || data.message || `HTTP ${response.status}`);
  }

  return data;
}

async function startRustDownload(tabId, url) {
  const task = await requestJson(`${BACKEND_URL}/api/download/stream/init`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      name: `${taskNameFromUrl(url)}_${Date.now()}`,
      url
    })
  });

  const taskId = task.id;
  const downloadUrl = task.download_url || `/api/download/stream/${encodeURIComponent(taskId)}`;

  chrome.tabs.sendMessage(tabId, {
    type: "M3U8_DOWNLOAD_STATUS",
    url,
    taskId,
    status: "pending",
    progress: 0
  }).catch(() => {});

  pollTask(tabId, url, taskId);
  try {
    await chrome.downloads.download({
      url: `${BACKEND_URL}${downloadUrl}`,
      saveAs: false
    });
  } catch (error) {
    chrome.tabs.create({ url: `${BACKEND_URL}${downloadUrl}` });
  }

  return { taskId };
}

async function pollTask(tabId, url, taskId) {
  for (;;) {
    await new Promise((resolve) => setTimeout(resolve, 1500));

    let task;
    try {
      task = await requestJson(`${BACKEND_URL}/api/tasks/${taskId}`);
    } catch (error) {
      chrome.tabs.sendMessage(tabId, {
        type: "M3U8_DOWNLOAD_STATUS",
        url,
        taskId,
        status: "failed",
        error: error.message
      }).catch(() => {});
      return;
    }

    chrome.tabs.sendMessage(tabId, {
      type: "M3U8_DOWNLOAD_STATUS",
      url,
      taskId,
      status: task.status,
      progress: task.progress || 0,
      error: task.error || ""
    }).catch(() => {});

    if (task.status === "completed") {
      return;
    }

    if (task.status === "failed") return;
  }
}

chrome.webRequest.onBeforeRequest.addListener(
  (details) => {
    rememberLink(details.tabId, details.url);
  },
  { urls: ["<all_urls>"] }
);

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message?.type === "GET_M3U8_LINKS") {
    const tabId = sender.tab?.id;
    sendResponse({ links: tabId == null ? [] : tabLinks.get(tabId) || [] });
    return;
  }

  if (message?.type === "CLEAR_M3U8_LINKS") {
    const tabId = sender.tab?.id;
    if (tabId != null) tabLinks.set(tabId, []);
    sendResponse({ ok: true });
    return;
  }

  if (message?.type === "START_RUST_DOWNLOAD") {
    const tabId = sender.tab?.id;
    if (tabId == null) {
      sendResponse({ ok: false, error: "无法识别当前标签页" });
      return;
    }

    startRustDownload(tabId, message.url)
      .then((result) => sendResponse({ ok: true, ...result }))
      .catch((error) => sendResponse({ ok: false, error: error.message }));
    return true;
  }
});

chrome.tabs.onRemoved.addListener((tabId) => {
  tabLinks.delete(tabId);
});
