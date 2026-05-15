(function () {
  "use strict";

  let links = [];
  let panel;
  let list;
  let badge;
  const downloadStates = new Map();

  function ensurePanel() {
    if (panel) return;

    panel = document.createElement("div");
    panel.id = "simple-m3u8-panel";
    Object.assign(panel.style, {
      position: "fixed",
      right: "16px",
      bottom: "16px",
      zIndex: "2147483647",
      width: "360px",
      maxWidth: "calc(100vw - 32px)",
      maxHeight: "50vh",
      background: "#ffffff",
      border: "1px solid #d8dde6",
      borderRadius: "8px",
      boxShadow: "0 8px 28px rgba(0,0,0,.18)",
      fontFamily: "Arial, sans-serif",
      color: "#1f2937",
      overflow: "hidden",
      display: "none"
    });

    const header = document.createElement("div");
    Object.assign(header.style, {
      display: "flex",
      alignItems: "center",
      justifyContent: "space-between",
      padding: "10px 12px",
      background: "#f3f6fa",
      borderBottom: "1px solid #d8dde6"
    });

    badge = document.createElement("strong");
    badge.textContent = "M3U8: 0";
    badge.style.fontSize = "14px";

    const buttons = document.createElement("div");
    buttons.style.display = "flex";
    buttons.style.gap = "8px";

    const copy = document.createElement("button");
    copy.textContent = "复制";
    styleButton(copy);
    copy.onclick = copyLinks;

    const clear = document.createElement("button");
    clear.textContent = "清空";
    styleButton(clear);
    clear.onclick = clearLinks;

    const close = document.createElement("button");
    close.textContent = "关闭";
    styleButton(close);
    close.onclick = () => {
      panel.style.display = "none";
    };

    buttons.append(copy, clear, close);
    header.append(badge, buttons);

    list = document.createElement("ol");
    Object.assign(list.style, {
      margin: "0",
      padding: "12px 12px 12px 32px",
      maxHeight: "calc(50vh - 48px)",
      overflow: "auto",
      fontSize: "12px"
    });

    panel.append(header, list);
    document.documentElement.appendChild(panel);
  }

  function styleButton(button) {
    Object.assign(button.style, {
      border: "1px solid #c8d0dc",
      background: "#fff",
      borderRadius: "5px",
      padding: "4px 8px",
      cursor: "pointer",
      fontSize: "12px",
      color: "#1f2937"
    });
  }

  function render(nextLinks) {
    links = Array.from(new Set(nextLinks || []));
    ensurePanel();

    badge.textContent = `M3U8: ${links.length}`;
    list.textContent = "";

    links.forEach((url) => {
      const item = document.createElement("li");
      item.title = url;
      Object.assign(item.style, {
        marginBottom: "8px",
        wordBreak: "break-all"
      });

      const row = document.createElement("div");
      Object.assign(row.style, {
        display: "flex",
        alignItems: "flex-start",
        gap: "8px"
      });

      const text = document.createElement("span");
      text.textContent = url;
      text.style.flex = "1";
      text.style.cursor = "pointer";
      text.onclick = () => navigator.clipboard.writeText(url).catch(() => {});

      const download = document.createElement("button");
      styleButton(download);
      download.onclick = () => startDownload(url, download);
      updateDownloadButton(download, state);

      row.append(text, download);
      item.appendChild(row);

      const state = downloadStates.get(url);
      if (state) {
        const status = document.createElement("div");
        status.textContent = formatStatus(state);
        Object.assign(status.style, {
          marginTop: "4px",
          fontSize: "11px",
          color: state.status === "failed" ? "#b91c1c" : "#52606d"
        });
        item.appendChild(status);
      }

      list.appendChild(item);
    });

    panel.style.display = links.length ? "block" : "none";
  }

  async function copyLinks() {
    if (!links.length) return;
    try {
      await navigator.clipboard.writeText(links.join("\n"));
    } catch (_) {
      const textarea = document.createElement("textarea");
      textarea.value = links.join("\n");
      textarea.style.position = "fixed";
      textarea.style.left = "-9999px";
      document.body.appendChild(textarea);
      textarea.select();
      document.execCommand("copy");
      textarea.remove();
    }
  }

  function clearLinks() {
    downloadStates.clear();
    chrome.runtime.sendMessage({ type: "CLEAR_M3U8_LINKS" }, () => render([]));
  }

  function formatStatus(state) {
    if (state.status === "failed") return `失败: ${state.error || "未知错误"}`;
    if (state.status === "completed") return "完成，浏览器已开始保存文件";
    const progress = Number.isFinite(state.progress) ? state.progress.toFixed(1) : "0.0";
    const label = {
      pending: "等待中",
      downloading: "下载中",
      merging: "合并中"
    }[state.status] || state.status || "等待中";
    return `${label} ${progress}%`;
  }

  function updateDownloadButton(button, state) {
    const active = state && !["failed", "completed"].includes(state.status);
    button.disabled = Boolean(active);
    button.textContent = active ? "下载中" : "下载";
    button.style.opacity = active ? "0.6" : "1";
    button.style.cursor = active ? "not-allowed" : "pointer";
  }

  function startDownload(url, button) {
    const current = downloadStates.get(url);
    if (current && !["failed", "completed"].includes(current.status)) return;

    button.disabled = true;
    button.textContent = "提交中";

    downloadStates.set(url, { status: "pending", progress: 0 });
    render(links);

    chrome.runtime.sendMessage({ type: "START_RUST_DOWNLOAD", url }, (response) => {
      if (!response?.ok) {
        downloadStates.set(url, {
          status: "failed",
          error: response?.error || "无法连接 Rust 服务"
        });
        render(links);
        return;
      }

      const current = downloadStates.get(url) || {};
      downloadStates.set(url, {
        ...current,
        taskId: response.taskId,
        status: current.status || "pending",
        progress: current.progress || 0
      });
      render(links);
    });
  }

  chrome.runtime.onMessage.addListener((message) => {
    if (message?.type === "M3U8_LINKS_UPDATED") {
      render(message.links);
    }

    if (message?.type === "M3U8_DOWNLOAD_STATUS") {
      downloadStates.set(message.url, message);
      render(links);
    }
  });

  chrome.runtime.sendMessage({ type: "GET_M3U8_LINKS" }, (response) => {
    render(response?.links || []);
  });
})();
