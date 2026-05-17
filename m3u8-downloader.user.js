// ==UserScript==
// @name         M3U8 下载助手（精简版）
// @namespace    http://tampermonkey.net/
// @version      3.7
// @description  拦截 m3u8，直传下载，自定义文件名，下载进度实时显示
// @match        *://*/*
// @run-at       document-start
// @grant        unsafeWindow
// @grant        GM_xmlhttpRequest
// @grant        GM_download
// @grant        GM_setClipboard
// @grant        GM_getValue
// @grant        GM_setValue
// @connect      *
// ==/UserScript==

(function () {
    'use strict';
    let BACKEND_URL = (() => {
        try { return GM_getValue('backendUrl', 'http://192.168.1.28:8080'); } catch (e) { return 'http://192.168.1.28:8080'; }
    })();
    const realWin = unsafeWindow;
    const isFirefox = /\bFirefox\//.test(navigator.userAgent);
    const isMobile = /Mobi|Android|iPhone|iPad|iPod|IEMobile|Opera Mini/i.test(navigator.userAgent)
        || (typeof navigator.maxTouchPoints === 'number' && navigator.maxTouchPoints > 1 && window.innerWidth < 1024)
        || (typeof screen !== 'undefined' && screen.width < 768);

    // ========== 下载队列 ==========
    const downloadQueue = [];
    let queueProcessing = false;

    if (realWin.__m3u8InterceptorInstalled) return;
    realWin.__m3u8InterceptorInstalled = true;

    // 初始化顶层存储
    try { if (realWin.top === realWin.self) realWin.__m3u8Links = realWin.__m3u8Links || new Set(); } catch (e) { /* cross-origin iframe */ }

    // ========== 注入页面拦截脚本（修复版） ==========
    const injectScript = `
    (function() {
        window.__m3u8Links = window.__m3u8Links || new Set();
        const add = (url) => {
            if (!url) return;
            try {
                let abs = new URL(url, location.href).href;
                if (abs.includes('.m3u8') && /^https?:/.test(abs)) {
                      try { if (top === self) window.__m3u8Links.add(abs);
                      else top.postMessage({ type: 'M3U8_LINK_FOUND', url: abs }, '*'); } catch(e) { /* cross-origin iframe */ }
                }
            } catch(e) {}
        };

        // Firefox 对替换部分原生构造器更严格，可能抛 SecurityError。
        // m3u8 通常来自 XHR/fetch/DOM，Firefox 下跳过 WebSocket 劫持以保持脚本可用。
        if (!${isFirefox}) {
            try {
                const OriginalWebSocket = window.WebSocket;
                class InterceptedWebSocket extends OriginalWebSocket {
                    constructor(url, protocols) {
                        if (protocols === undefined) super(url);
                        else super(url, protocols);
                        add(url);
                    }
                }
                try { Object.setPrototypeOf(InterceptedWebSocket, OriginalWebSocket); } catch(e) {}
                window.WebSocket = InterceptedWebSocket;
            } catch(e) {}
        }

        // ----- XHR 劫持（无侵入）-----
        try {
            const xhrOpen = XMLHttpRequest.prototype.open;
            XMLHttpRequest.prototype.open = function(m, u) { this._url = u; return xhrOpen.apply(this, arguments); };
            const xhrSend = XMLHttpRequest.prototype.send;
            XMLHttpRequest.prototype.send = function(b) { if (this._url) add(this._url); return xhrSend.apply(this, arguments); };
        } catch(e) {}

        // ----- fetch 劫持（无侵入）-----
        try {
            const origFetch = window.fetch;
            window.fetch = function(i) { add(typeof i === 'string' ? i : i.url); return origFetch.apply(this, arguments); };
        } catch(e) {}

        // ----- 监听 DOM 变化与扫描（保持不变）-----
        new MutationObserver(muts => {
            muts.forEach(m => {
                if (m.type === 'attributes' && (m.attributeName === 'src' || m.attributeName === 'href')) {
                    add(m.target[m.attributeName] || m.target.getAttribute(m.attributeName));
                } else if (m.type === 'childList') {
                    m.addedNodes.forEach(n => {
                        if (n.nodeType === 1) {
                            ['src','href','data-src','data-href'].forEach(a => { let v = n.getAttribute(a); if(v) add(v); });
                            if (n.tagName === 'VIDEO' || n.tagName === 'AUDIO') { if(n.src) add(n.src); if(n.currentSrc) add(n.currentSrc); }
                        }
                    });
                }
            });
        }).observe(document.documentElement, { childList: true, subtree: true, attributes: true, attributeFilter: ['src','href'] });

        const scan = () => document.querySelectorAll('[src],[href],[data-src],[data-href],video,audio,source').forEach(el => {
            let s = el.src || el.getAttribute('src') || el.href || el.getAttribute('href') || el.currentSrc;
            if(s) add(s);
            if(el.tagName === 'SOURCE' && el.src) add(el.src);
        });
        if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', scan);
        else scan();
        setInterval(scan, 3000);
    })();`;
    const s = document.createElement('script');
    s.textContent = injectScript;
    (document.head || document.documentElement).appendChild(s);
    s.remove();

    // ========== 顶层监听 iframe 消息 & 轮询 ==========
    try {
        if (realWin.top === realWin.self) {
            realWin.addEventListener('message', e => {
                if (e.data?.type === 'M3U8_LINK_FOUND' && e.data.url && !realWin.__m3u8Links.has(e.data.url)) {
                    realWin.__m3u8Links.add(e.data.url);
                    if (realWin.__m3u8Links.size === 1) ensureButton();
                    const btn = document.getElementById('tm-m3u8-btn');
                    if (btn) btn.textContent = `🎬 1获取 m3u8 链接 (${realWin.__m3u8Links.size})`;
                }
            });
            document.addEventListener('DOMContentLoaded', () => realWin.__m3u8Links?.size && ensureButton());
            setInterval(() => { if (realWin.__m3u8Links?.size && !document.getElementById('tm-m3u8-btn')) ensureButton(); }, 1500);
        }
    } catch (e) { /* cross-origin iframe */ }

    // ========== 辅助函数 ==========
    const copyText = (text) => {
        try {
            if (typeof GM_setClipboard === 'function') {
                GM_setClipboard(text, 'text');
                return Promise.resolve(true);
            }
        } catch (e) { }

        if (navigator.clipboard && window.isSecureContext) {
            return navigator.clipboard.writeText(text).then(() => true);
        }

        fallbackCopy(text);
        return Promise.resolve(true);
    };

    // 复制降级（fallback for insecure context）
    const fallbackCopy = (text) => {
        const ta = document.createElement('textarea');
        ta.value = text;
        ta.style.position = 'fixed';
        ta.style.left = '-9999px';
        document.body.appendChild(ta);
        ta.select();
        try { document.execCommand('copy'); alert('已复制！'); } catch (e) { alert('复制失败，请手动复制'); }
        document.body.removeChild(ta);
    };
    const sanitize = n => n
        ? n.replace(/[\\/:*?"<>|\x00-\x1f\x7f]/g, '_').replace(/_+/g, '_').replace(/^_|_+$/g, '').trim() || 'untitled'
        : 'untitled';
    const defaultFileName = (link) => {
        let t = document.title;
        if (t?.trim()) return sanitize(t);
        try {
            let last = new URL(link).pathname.split('/').pop().replace(/\.m3u8$/i, '');
            return sanitize(decodeURIComponent(last)) || 'video';
        } catch (e) { return 'm3u8_video'; }
    };
    const normalizeStatus = (status) => String(status || '').trim().toLowerCase();
    const formatBytes = (bytes) => {
        if (!Number.isFinite(bytes) || bytes <= 0) return '0 B';
        const units = ['B', 'KB', 'MB', 'GB', 'TB'];
        let value = bytes;
        let index = 0;
        while (value >= 1024 && index < units.length - 1) {
            value /= 1024;
            index += 1;
        }
        return `${value.toFixed(value >= 100 || index === 0 ? 0 : 1)} ${units[index]}`;
    };
    const parseResponseError = (resp, fallback) => {
        try {
            const data = JSON.parse(resp.responseText || '{}');
            return data.error || data.message || fallback;
        } catch (e) {
            return fallback;
        }
    };
    const parseDownloadError = (err) => {
        if (!err) return '未知错误';
        if (typeof err === 'string') return err;
        return err.error || err.details || err.message || JSON.stringify(err);
    };
    const getTaskStatus = (taskId) => new Promise((resolve, reject) => {
        GM_xmlhttpRequest({
            method: 'GET',
            url: `${BACKEND_URL}/api/tasks/${encodeURIComponent(taskId)}`,
            onload: resp => {
                if (resp.status >= 200 && resp.status < 300) {
                    try { resolve(JSON.parse(resp.responseText)); } catch (e) { reject('任务状态解析失败'); }
                } else {
                    reject(parseResponseError(resp, `状态码 ${resp.status}`));
                }
            },
            onerror: err => reject(`网络错误: ${parseDownloadError(err)}`)
        });
    });

    const showNotification = (msg, type = 'success') => {
        let n = document.getElementById('tm-notification');
        if (n) n.remove();
        n = document.createElement('div');
        n.id = 'tm-notification';
        Object.assign(n.style, {
            position: 'fixed', bottom: '190px', right: '20px', zIndex: '10002', padding: '12px 20px',
            borderRadius: '8px', backgroundColor: type === 'success' ? '#4caf50' : '#f44336', color: 'white',
            fontSize: '14px', fontWeight: 'bold', fontFamily: 'sans-serif', boxShadow: '0 2px 10px rgba(0,0,0,0.2)',
            opacity: '0', transform: 'translateX(100%)', transition: 'opacity 0.3s ease, transform 0.3s ease', pointerEvents: 'none'
        });
        n.textContent = msg;
        document.body.appendChild(n);
        setTimeout(() => { n.style.opacity = '1'; n.style.transform = 'translateX(0)'; }, 10);
        setTimeout(() => { n.style.opacity = '0'; n.style.transform = 'translateX(100%)'; setTimeout(() => n.remove(), 300); }, 3000);
    };

    const showFileNameDialog = (link) => new Promise(resolve => {
        const def = defaultFileName(link);
        const overlay = document.createElement('div');
        overlay.id = 'tm-filename-modal';
        Object.assign(overlay.style, { position: 'fixed', top: 0, left: 0, width: '100%', height: '100%', backgroundColor: 'rgba(0,0,0,0.5)', display: 'flex', alignItems: 'center', justifyContent: 'center', zIndex: '10050', fontFamily: 'sans-serif' });
        const box = document.createElement('div');
        Object.assign(box.style, { backgroundColor: '#fff', borderRadius: '8px', width: '350px', maxWidth: '80%', padding: '20px', boxShadow: '0 4px 20px rgba(0,0,0,0.3)' });
        box.innerHTML = `<h3 style="margin:0 0 12px 0">请输入文件名</h3><input type="text" value="${def.replace(/"/g, '&quot;')}" placeholder="文件名（不含扩展名）" style="width:100%;padding:8px;margin-bottom:16px;border:1px solid #ccc;border-radius:4px;box-sizing:border-box;font-size:14px"><div style="display:flex;justify-content:flex-end;gap:10px"><button id="tm-confirm" style="padding:6px 16px;background:#4caf50;color:#fff;border:none;border-radius:4px;cursor:pointer">确定</button><button id="tm-cancel" style="padding:6px 16px;background:#f44336;color:#fff;border:none;border-radius:4px;cursor:pointer">取消</button></div>`;
        overlay.appendChild(box);
        document.body.appendChild(overlay);
        const input = box.querySelector('input');
        input.focus(); input.select();
        const cleanup = () => overlay.remove();
        const confirm = () => { let val = input.value.trim() || def; cleanup(); resolve(sanitize(val)); };
        const cancel = () => { cleanup(); resolve(null); };
        box.querySelector('#tm-confirm').onclick = confirm;
        box.querySelector('#tm-cancel').onclick = cancel;
        overlay.onclick = e => { if (e.target === overlay) cancel(); };
        input.onkeypress = e => { if (e.key === 'Enter') confirm(); };
    });

    const startDownload = (url, name) => new Promise((resolve, reject) => {
        GM_xmlhttpRequest({
            method: 'POST', url: `${BACKEND_URL}/api/download`, headers: { 'Content-Type': 'application/json' },
            data: JSON.stringify({ url, name, output_dir: null }),
            onload: resp => {
                if (resp.status === 200 || resp.status === 201) {
                    try { resolve(JSON.parse(resp.responseText).id); } catch (e) { reject('解析失败'); }
                } else reject(parseResponseError(resp, `状态码 ${resp.status}`));
            },
            onerror: err => reject(`网络错误: ${err}`)
        });
    });

    const initDirectDownload = (url, name) => new Promise((resolve, reject) => {
        GM_xmlhttpRequest({
            method: 'POST',
            url: `${BACKEND_URL}/api/download/stream/init`,
            headers: { 'Content-Type': 'application/json' },
            data: JSON.stringify({ url, name, output_dir: null }),
            onload: resp => {
                if (resp.status === 200 || resp.status === 201) {
                    try {
                        const data = JSON.parse(resp.responseText);
                        if (!data.id) reject('未返回任务 ID');
                        else resolve(data);
                    } catch (e) {
                        reject('解析失败');
                    }
                } else {
                    reject(parseResponseError(resp, `状态码 ${resp.status}`));
                }
            },
            onerror: err => reject(`网络错误: ${err}`)
        });
    });

    const triggerDirectDownload = (taskId, fileName, hooks = {}) => new Promise((resolve, reject) => {
        GM_download({
            url: `${BACKEND_URL}/api/download/stream/${encodeURIComponent(taskId)}`,
            name: `${fileName}.mp4`,
            saveAs: false,
            onprogress: e => {
                if (hooks.onprogress) hooks.onprogress(e.loaded || 0, e.lengthComputable ? e.total : 0);
            },
            onload: () => resolve(),
            onerror: err => reject(parseDownloadError(err))
        });
    });

    const triggerCompletedTaskDownload = (taskId, fileName, hooks = {}) => new Promise((resolve, reject) => {
        GM_download({
            url: `${BACKEND_URL}/api/tasks/${encodeURIComponent(taskId)}/download`,
            name: `${fileName}.mp4`,
            saveAs: false,
            onprogress: e => {
                if (hooks.onprogress) hooks.onprogress(e.loaded || 0, e.lengthComputable ? e.total : 0);
            },
            onload: () => resolve(),
            onerror: err => reject(parseDownloadError(err))
        });
    });

    // ========== 进度弹窗组件 ==========
    const createProgressModal = (taskId, taskName, url) => {
        const modal = document.createElement('div');
        modal.id = `download-progress-${taskId}`;
        Object.assign(modal.style, { position: 'fixed', top: '80px', right: '20px', width: '320px', backgroundColor: '#fff', borderRadius: '8px', boxShadow: '0 4px 20px rgba(0,0,0,0.3)', zIndex: '10001', fontFamily: 'sans-serif', transition: 'all 0.2s ease', overflow: 'hidden' });
        modal.innerHTML = `<div class="full-view"><div style="display:flex;justify-content:space-between;align-items:center;padding:12px 16px;background:#f5f5f5;border-bottom:1px solid #e0e0e0"><h3 style="margin:0;font-size:14px;font-weight:bold;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;max-width:180px">📥 ${taskName}</h3><div style="display:flex;gap:8px"><button class="minimize" style="background:none;border:none;font-size:18px;cursor:pointer;color:#666" title="最小化">−</button><button class="close" style="background:none;border:none;font-size:18px;cursor:pointer;color:#666" title="关闭">✖</button></div></div><div style="padding:16px;border-bottom:1px solid #f0f0f0"><div style="width:100%;background:#f0f0f0;border-radius:4px;overflow:hidden;margin-bottom:8px"><div class="progress-bar" style="width:0%;height:24px;background:#4caf50;text-align:center;line-height:24px;color:white;font-size:12px">0%</div></div><div class="status-text" style="font-size:12px;color:#666">状态：等待开始...</div><div class="download-text" style="font-size:11px;color:#888;margin-top:6px">浏览器下载：等待开始...</div></div><div style="padding:0 16px 16px 16px;font-size:10px;color:#999;word-break:break-all">${url}</div></div><div class="mini-view" style="display:none;width:50px;height:50px;border-radius:50%;background:#4caf50;cursor:pointer;align-items:center;justify-content:center;text-align:center;color:white;font-size:16px;font-weight:bold;box-shadow:0 2px 8px rgba(0,0,0,0.2)">0%</div>`;
        document.body.appendChild(modal);
        const fullView = modal.querySelector('.full-view');
        const miniView = modal.querySelector('.mini-view');
        const progressBar = modal.querySelector('.progress-bar');
        const statusDiv = modal.querySelector('.status-text');
        const downloadDiv = modal.querySelector('.download-text');
        const minimizeBtn = modal.querySelector('.minimize');
        const closeBtn = modal.querySelector('.close');
        let isMinimized = false;
        const setMinimized = (min) => {
            isMinimized = min;
            fullView.style.display = min ? 'none' : 'block';
            miniView.style.display = min ? 'flex' : 'none';
            modal.style.width = min ? '50px' : '320px';
            modal.style.height = min ? '50px' : 'auto';
            modal.style.borderRadius = min ? '50%' : '8px';
            modal.style.backgroundColor = min ? 'transparent' : '#fff';
            modal.style.boxShadow = min ? 'none' : '0 4px 20px rgba(0,0,0,0.3)';
        };
        minimizeBtn.onclick = () => setMinimized(true);
        closeBtn.onclick = () => modal.remove();
        miniView.onclick = () => setMinimized(false);
        return {
            updateProgress: (pct) => {
                let p = Math.min(100, Math.max(0, pct));
                progressBar.style.width = `${p}%`;
                progressBar.textContent = `${Math.floor(p)}%`;
                miniView.textContent = `${Math.floor(p)}%`;
                let bg = p < 30 ? '#f44336' : (p < 80 ? '#ff9800' : '#4caf50');
                miniView.style.backgroundColor = bg;
                progressBar.style.backgroundColor = bg;
            },
            updateStatus: (status, err = '') => {
                status = normalizeStatus(status);
                if (status === 'downloading') statusDiv.textContent = '状态：下载中...';
                else if (status === 'merging') statusDiv.textContent = '状态：合并并直传中...';
                else if (status === 'completed') { statusDiv.textContent = '✅ 下载完成！'; progressBar.style.backgroundColor = '#4caf50'; miniView.style.backgroundColor = '#4caf50'; miniView.textContent = '✓'; }
                else if (status === 'failed') { statusDiv.textContent = `❌ 下载失败: ${err || '未知错误'}`; progressBar.style.backgroundColor = '#f44336'; miniView.style.backgroundColor = '#f44336'; miniView.textContent = '✗'; }
                else if (status === 'pending') statusDiv.textContent = '状态：等待中...';
            },
            updateDownloadState: (text, color = '#888') => {
                if (downloadDiv) {
                    downloadDiv.textContent = `浏览器下载：${text}`;
                    downloadDiv.style.color = color;
                }
            },
            close: () => modal.remove()
        };
    };

    const monitorTask = async (taskId, taskName, url, opts = {}) => {
        const ui = createProgressModal(taskId, taskName, url);
        let ws, heartbeat, completed = false;
        let pollTimer = null;
        let downloadDone = !opts.expectBrowserDownload;
        let taskDone = false;
        let taskFailed = false;
        let wsFailed = false;
        let fallbackTimer = null;
        let browserDownloadStarted = false;
        const tryFinalize = () => {
            if (!completed && taskDone && downloadDone) finalize(true);
        };
        const finalize = (success, err = '') => {
            if (completed) return;
            completed = true;
            clearTimeout(heartbeat); clearInterval(heartbeat);
            clearInterval(pollTimer);
            clearTimeout(fallbackTimer);
            if (ws && ws.readyState === WebSocket.OPEN) ws.close();
            ui.close();
            showNotification(`${taskName} ${success ? '下载完成' : '下载失败' + (err ? ': ' + err : '')}`, success ? 'success' : 'error');
        };
        const handleTaskUpdate = (task) => {
            if (task.progress !== undefined) ui.updateProgress(task.progress);
            if (task.status) ui.updateStatus(task.status, task.error);
            const status = normalizeStatus(task.status);
            if (status === 'completed') {
                taskDone = true;
                if (opts.downloadFileOnCompleted && !browserDownloadStarted) {
                    browserDownloadStarted = true;
                    ui.updateDownloadState('服务端完成，正在保存到浏览器下载目录...', '#666');
                    opts.downloadFileOnCompleted({
                        onprogress: (loaded, total) => {
                            if (!total || total <= 0) {
                                ui.updateDownloadState(`已接收 ${formatBytes(loaded)}`, '#666');
                                return;
                            }
                            const pct = Math.min(100, Math.max(0, loaded / total * 100));
                            ui.updateDownloadState(`${pct.toFixed(1)}% (${formatBytes(loaded)} / ${formatBytes(total)})`, '#666');
                        }
                    })
                        .then(() => {
                            downloadDone = true;
                            ui.updateDownloadState('已保存到浏览器下载目录', '#4caf50');
                            tryFinalize();
                        })
                        .catch(err => {
                            ui.updateDownloadState(`失败: ${err || '未知错误'}`, '#f44336');
                            finalize(false, err || '浏览器下载失败');
                        });
                    return;
                }
                tryFinalize();
            } else if (status === 'failed') {
                taskFailed = true;
                finalize(false, task.error);
            }
        };
        const poll = () => {
            getTaskStatus(taskId)
                .then(handleTaskUpdate)
                .catch(err => {
                    if (!completed && !taskDone && !taskFailed) finalize(false, err);
                });
        };
        const connect = () => {
            // 以下情况跳过 WebSocket（不稳定或不可用），使用轮询：
            //   - Firefox（构造器替换限制）
            //   - 移动端（WS 兼容性差、HTTPS 混合内容拦截、连接不稳定）
            //   - HTTPS 页面 + HTTP 后端（ws:// 被混合内容策略拦截）
            if (isFirefox || isMobile || (window.isSecureContext && BACKEND_URL.startsWith('http://'))) {
                poll();
                pollTimer = setInterval(poll, 1500);
                return;
            }
            ws = new WebSocket(`${BACKEND_URL.replace('http', 'ws')}/api/tasks/${taskId}/ws`);
            ws.onopen = () => { heartbeat = setInterval(() => { if (ws?.readyState === WebSocket.OPEN) ws.send('ping'); }, 30000); };
            ws.onmessage = e => {
                try {
                    handleTaskUpdate(JSON.parse(e.data));
                } catch (e) { }
            };
            ws.onerror = () => {
                if (!completed && !taskDone && !taskFailed && !wsFailed) {
                    wsFailed = true;
                    clearTimeout(heartbeat); clearInterval(heartbeat);
                    // WebSocket 连接失败，降级到轮询
                    poll();
                    pollTimer = setInterval(poll, 1500);
                }
            };
            ws.onclose = () => {
                if (completed || taskDone || taskFailed || wsFailed) return;
                // WebSocket 意外关闭，降级到轮询
                clearTimeout(heartbeat); clearInterval(heartbeat);
                poll();
                pollTimer = setInterval(poll, 1500);
            };
        };
        connect();
        if (opts.expectBrowserDownload) {
            ui.updateDownloadState('准备启动...', '#666');
        }
        return {
            markBrowserDownloadStarted: () => ui.updateDownloadState('已开始', '#666'),
            markBrowserDownloadProgress: (loaded, total) => {
                if (!total || total <= 0) {
                    ui.updateDownloadState(`已接收 ${formatBytes(loaded)}`, '#666');
                    return;
                }
                const pct = Math.min(100, Math.max(0, loaded / total * 100));
                ui.updateDownloadState(`${pct.toFixed(1)}% (${formatBytes(loaded)} / ${formatBytes(total)})`, '#666');
            },
            markBrowserDownloadCompleted: () => {
                downloadDone = true;
                ui.updateDownloadState('已保存到浏览器下载目录', '#4caf50');
                tryFinalize();
            },
            markBrowserDownloadFailed: (err) => {
                ui.updateDownloadState(`失败: ${err || '未知错误'}`, '#f44336');
                finalize(false, err || '浏览器下载失败');
            }
        };
    };

    // ========== 下载队列 ==========
    const addToQueue = (link, fileName) => {
        downloadQueue.push({ link, fileName, status: 'pending' });
        updateQueueUI();
        if (!queueProcessing) processQueue();
    };

    const processQueue = async () => {
        if (queueProcessing) return;
        queueProcessing = true;
        while (downloadQueue.length > 0 && queueProcessing) {
            const item = downloadQueue[0];
            item.status = 'downloading';
            updateQueueUI();
            try {
                const taskName = `${item.fileName}_${Date.now()}`;
                const taskId = await startDownload(item.link, taskName);
                await monitorTask(taskId, item.fileName, item.link, {
                    expectBrowserDownload: true,
                    downloadFileOnCompleted: hooks => {
                        if (isMobile) {
                            const downloadUrl = `${BACKEND_URL}/api/tasks/${encodeURIComponent(taskId)}/download`;
                            window.location.href = downloadUrl;
                            return Promise.resolve();
                        }
                        return triggerCompletedTaskDownload(taskId, item.fileName, hooks);
                    }
                });
                item.status = 'completed';
            } catch (err) {
                item.status = 'failed';
                showNotification(`下载失败: ${err}`, 'error');
            }
            downloadQueue.shift();
            updateQueueUI();
        }
        queueProcessing = false;
        updateQueueUI();
    };

    const updateQueueUI = () => {
        const el = document.getElementById('tm-queue-status');
        if (!el) return;
        const total = downloadQueue.length;
        const completed = downloadQueue.filter(i => i.status === 'completed').length;
        const failed = downloadQueue.filter(i => i.status === 'failed').length;
        const downloading = downloadQueue.filter(i => i.status === 'downloading').length;
        const pending = total - completed - failed - downloading;
        const parts = [];
        if (downloading > 0) parts.push(`📥 ${downloading} 下载中`);
        if (pending > 0) parts.push(`⏳ ${pending} 待处理`);
        if (completed > 0) parts.push(`✅ ${completed} 完成`);
        if (failed > 0) parts.push(`❌ ${failed} 失败`);
        el.textContent = `📋 队列: ${parts.length ? parts.join(' | ') : '空'}`;
        el.style.backgroundColor = failed > 0 ? '#fff3e0' : '#f5f5f5';
    };

    // ========== 设置面板 ==========
    const showSettingsModal = () => {
        let existing = document.getElementById('tm-settings-modal');
        if (existing) existing.remove();
        const overlay = document.createElement('div');
        overlay.id = 'tm-settings-modal';
        Object.assign(overlay.style, {
            position: 'fixed', top: 0, left: 0, width: '100%', height: '100%',
            backgroundColor: 'rgba(0,0,0,0.5)', display: 'flex', alignItems: 'center', justifyContent: 'center',
            zIndex: '10050', fontFamily: 'sans-serif'
        });
        const box = document.createElement('div');
        Object.assign(box.style, {
            backgroundColor: '#fff', borderRadius: '12px', width: '380px', maxWidth: '85%',
            padding: '24px', boxShadow: '0 4px 24px rgba(0,0,0,0.25)'
        });
        box.innerHTML = `
            <h3 style="margin:0 0 16px 0;font-size:16px">⚙️ 设置</h3>
            <label style="display:block;font-size:13px;color:#555;margin-bottom:4px">后端下载服务地址</label>
            <input type="text" id="tm-settings-url" value="${BACKEND_URL.replace(/"/g, '&quot;')}"
                placeholder="http://192.168.1.28:8080"
                style="width:100%;padding:10px 12px;margin-bottom:12px;border:1px solid #ccc;border-radius:6px;box-sizing:border-box;font-size:14px">
            <div style="font-size:12px;color:#999;margin-bottom:16px;line-height:1.5">
                💡 修改后立即生效，无需刷新页面。请确保服务端已在目标地址运行。
            </div>
            <div style="display:flex;justify-content:flex-end;gap:10px">
                <button id="tm-settings-save" style="padding:8px 20px;background:#4caf50;color:#fff;border:none;border-radius:6px;cursor:pointer;font-size:14px">保存</button>
                <button id="tm-settings-cancel" style="padding:8px 20px;background:#999;color:#fff;border:none;border-radius:6px;cursor:pointer;font-size:14px">取消</button>
            </div>
        `;
        overlay.appendChild(box);
        document.body.appendChild(overlay);
        const input = box.querySelector('#tm-settings-url');
        input.focus(); input.select();
        box.querySelector('#tm-settings-cancel').onclick = () => overlay.remove();
        overlay.onclick = e => { if (e.target === overlay) overlay.remove(); };
        box.querySelector('#tm-settings-save').onclick = () => {
            const val = input.value.trim();
            if (!val) { showNotification('请输入有效的服务端地址', 'error'); return; }
            try {
                new URL(val);
            } catch (e) {
                showNotification('地址格式不正确，需包含 http:// 或 https://', 'error');
                return;
            }
            GM_setValue('backendUrl', val);
            BACKEND_URL = val;
            overlay.remove();
            showNotification('后端地址已更新', 'success');
        };
        input.onkeypress = e => { if (e.key === 'Enter') box.querySelector('#tm-settings-save').click(); };
    };

    const showLinksModal = (links) => {
        let existing = document.getElementById('tm-m3u8-modal');
        if (existing) existing.remove();
        const modal = document.createElement('div');
        modal.id = 'tm-m3u8-modal';
        Object.assign(modal.style, { position: 'fixed', top: 0, left: 0, width: '100%', height: '100%', backgroundColor: 'rgba(0,0,0,0.6)', display: 'flex', alignItems: 'center', justifyContent: 'center', zIndex: '10000', fontFamily: 'sans-serif' });
        modal.onclick = e => { if (e.target === modal) modal.remove(); };
        const panel = document.createElement('div');
        Object.assign(panel.style, { backgroundColor: '#fff', borderRadius: '8px', width: '80%', maxWidth: '800px', maxHeight: '80%', display: 'flex', flexDirection: 'column', boxShadow: '0 4px 20px rgba(0,0,0,0.3)', overflow: 'hidden' });
        panel.innerHTML = `<div style="padding:12px 20px;border-bottom:1px solid #e0e0e0;display:flex;justify-content:space-between;align-items:center;background:#f5f5f5"><h3 style="margin:0">📺 m3u8 链接 (${links.length})</h3><button class="close-modal" style="background:none;border:none;font-size:20px;cursor:pointer;color:#666">✖</button></div><div id="tm-queue-status" style="padding:8px 20px;font-size:12px;color:#666;background:#f5f5f5;border-bottom:1px solid #e0e0e0">📋 队列: 空</div><button id="copy-all" style="margin:0 20px 8px 20px;padding:6px 12px;background:#4caf50;color:white;border:none;border-radius:4px;cursor:pointer;font-size:14px;align-self:flex-start">📋 复制全部链接</button><div style="flex:1;overflow-y:auto;padding:8px 20px 16px 20px"><ul style="margin:0;padding-left:20px;list-style:decimal" id="link-list"></ul></div>`;
        const listContainer = panel.querySelector('#link-list');
        links.forEach(link => {
            const li = document.createElement('li');
            li.style.marginBottom = '12px'; li.style.wordBreak = 'break-all';
            const span = document.createElement('span');
            span.textContent = link;
            Object.assign(span.style, { fontSize: '13px', fontFamily: 'monospace', backgroundColor: '#f9f9f9', padding: '2px 4px', borderRadius: '3px', cursor: 'pointer', display: 'inline-block', maxWidth: 'calc(100% - 60px)', overflow: 'hidden', textOverflow: 'ellipsis' });
            span.title = '点击下载此视频（可自定义文件名）';
            span.onclick = async (e) => {
                e.stopPropagation();
                let fileName = await showFileNameDialog(link);
                if (!fileName) return;
                try {
                    const taskName = `${fileName}_${Date.now()}`;
                    const taskId = await startDownload(link, taskName);
                    await monitorTask(taskId, fileName, link, {
                        expectBrowserDownload: true,
                        downloadFileOnCompleted: hooks => {
                            if (isMobile) {
                                const downloadUrl = `${BACKEND_URL}/api/tasks/${encodeURIComponent(taskId)}/download`;
                                window.location.href = downloadUrl;
                                return Promise.resolve();
                            }
                            return triggerCompletedTaskDownload(taskId, fileName, hooks);
                        }
                    });
                } catch (err) { showNotification(`启动下载失败: ${err}`, 'error'); }
            };
            const queueBtn = document.createElement('button');
            queueBtn.textContent = '➕';
            queueBtn.title = '加入下载队列';
            Object.assign(queueBtn.style, {
                marginLeft: '6px', padding: '2px 6px', border: '1px solid #ccc',
                borderRadius: '4px', backgroundColor: '#fff', cursor: 'pointer',
                fontSize: '13px', verticalAlign: 'middle'
            });
            queueBtn.onclick = async (e) => {
                e.stopPropagation();
                let fileName = await showFileNameDialog(link);
                if (fileName) addToQueue(link, fileName);
            };
            const wrapper = document.createElement('div');
            wrapper.style.cssText = 'display:flex;align-items:center;gap:4px';
            span.style.maxWidth = 'calc(100% - 50px)';
            wrapper.appendChild(span);
            wrapper.appendChild(queueBtn);
            li.appendChild(wrapper);
            listContainer.appendChild(li);
        });
        panel.querySelector('.close-modal').onclick = () => modal.remove();
        panel.querySelector('#copy-all').onclick = async () => {
            try { await copyText(links.join('\n')); panel.querySelector('#copy-all').textContent = '✅ 已复制！'; setTimeout(() => panel.querySelector('#copy-all').textContent = '📋 复制全部链接', 2000); } catch (e) { fallbackCopy(links.join('\n')); }
        };
        modal.appendChild(panel);
        document.body.appendChild(modal);
    };

    function ensureButton() {
        try { if (realWin.top !== realWin.self) return; } catch (e) { return; }
        if (document.getElementById('tm-m3u8-btn')) return;
        if (!realWin.__m3u8Links?.size) return;
        const btn = document.createElement('button');
        btn.id = 'tm-m3u8-btn';
        btn.textContent = `🎬 1获取 m3u8 链接 (${realWin.__m3u8Links.size})`;
        Object.assign(btn.style, { position: 'fixed', bottom: '120px', right: '20px', zIndex: '9999', padding: '8px 16px', border: 'none', borderRadius: '6px', backgroundColor: '#fff', color: '#333', fontWeight: 'bold', cursor: 'pointer', boxShadow: '0 2px 6px rgba(0,0,0,0.2)', transition: 'background-color 0.2s ease', fontFamily: 'sans-serif' });
        btn.onmouseenter = () => btn.style.backgroundColor = '#e0e0e0';
        btn.onmouseleave = () => btn.style.backgroundColor = '#fff';
        btn.onclick = () => { if (realWin.__m3u8Links?.size) showLinksModal(Array.from(realWin.__m3u8Links)); };
        document.body.appendChild(btn);

        // 设置按钮
        const settingsBtn = document.createElement('button');
        settingsBtn.id = 'tm-m3u8-settings';
        settingsBtn.textContent = '⚙️';
        Object.assign(settingsBtn.style, {
            position: 'fixed', bottom: '175px', right: '20px', zIndex: '9999',
            padding: '8px 12px', border: 'none', borderRadius: '6px',
            backgroundColor: '#fff', color: '#333', fontWeight: 'bold',
            cursor: 'pointer', boxShadow: '0 2px 6px rgba(0,0,0,0.2)',
            transition: 'background-color 0.2s ease', fontFamily: 'sans-serif',
            fontSize: '16px', lineHeight: '1'
        });
        settingsBtn.onmouseenter = () => settingsBtn.style.backgroundColor = '#e0e0e0';
        settingsBtn.onmouseleave = () => settingsBtn.style.backgroundColor = '#fff';
        settingsBtn.onclick = () => showSettingsModal();
        document.body.appendChild(settingsBtn);
        let last = -1;
        const update = () => {
            let c = realWin.__m3u8Links?.size || 0;
            if (c !== last) { last = c; btn.textContent = c ? `🎬1 获取 m3u8 链接 (${c})` : '🎬 1获取 m3u8 链接'; btn.style.display = c ? 'block' : 'none'; }
        };
        update();
        setInterval(update, 1000);
    }
})();

