// ==UserScript==
// @name         M3U8 下载助手（精简版）
// @namespace    http://tampermonkey.net/
// @version      3.2
// @description  拦截 m3u8，自定义文件名，下载进度实时显示
// @match        *://*/*
// @run-at       document-start
// @grant        unsafeWindow
// @grant        GM_xmlhttpRequest
// ==/UserScript==

(function () {
    'use strict';
    const BACKEND_URL = 'http://localhost:8080';
    const realWin = unsafeWindow;

    if (realWin.__m3u8InterceptorInstalled) return;
    realWin.__m3u8InterceptorInstalled = true;

    // 初始化顶层存储
    if (realWin.top === realWin.self) realWin.__m3u8Links = realWin.__m3u8Links || new Set();

    // ========== 注入页面拦截脚本（修复版） ==========
    const injectScript = `
    (function() {
        window.__m3u8Links = window.__m3u8Links || new Set();
        const add = (url) => {
            if (!url) return;
            try {
                let abs = new URL(url, location.href).href;
                if (abs.includes('.m3u8') && /^https?:/.test(abs)) {
                    if (top === self) window.__m3u8Links.add(abs);
                    else top.postMessage({ type: 'M3U8_LINK_FOUND', url: abs }, '*');
                }
            } catch(e) {}
        };

        // ----- 修复 WebSocket 劫持（保留所有静态属性和原型）-----
        const OriginalWebSocket = window.WebSocket;
        class InterceptedWebSocket extends OriginalWebSocket {
            constructor(url, protocols) {
                super(url, protocols);
                add(url);   // 拦截 m3u8 地址
            }
        }
        // 确保所有静态属性被完整继承（extends 已继承，但某些环境下可能缺失，手动复制一次确保）
        Object.setPrototypeOf(InterceptedWebSocket, OriginalWebSocket);
        // 替换全局 WebSocket
        window.WebSocket = InterceptedWebSocket;

        // ----- XHR 劫持（无侵入）-----
        const xhrOpen = XMLHttpRequest.prototype.open;
        XMLHttpRequest.prototype.open = function(m, u) { this._url = u; return xhrOpen.apply(this, arguments); };
        const xhrSend = XMLHttpRequest.prototype.send;
        XMLHttpRequest.prototype.send = function(b) { if (this._url) add(this._url); return xhrSend.apply(this, arguments); };

        // ----- fetch 劫持（无侵入）-----
        const origFetch = window.fetch;
        window.fetch = function(i) { add(typeof i === 'string' ? i : i.url); return origFetch.apply(this, arguments); };

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

    // ========== 辅助函数 ==========
    const sanitize = n => n ? n.replace(/[\\/:*?"<>|]/g, '_').trim() : 'untitled';
    const defaultFileName = (link) => {
        let t = document.title;
        if (t?.trim()) return sanitize(t);
        try {
            let last = new URL(link).pathname.split('/').pop().replace(/\.m3u8$/i, '');
            return sanitize(decodeURIComponent(last)) || 'video';
        } catch (e) { return 'm3u8_video'; }
    };

    const showNotification = (msg, type = 'success') => {
        let n = document.getElementById('tm-notification');
        if (n) n.remove();
        n = document.createElement('div');
        n.id = 'tm-notification';
        Object.assign(n.style, {
            position: 'fixed', bottom: '90px', right: '20px', zIndex: '10002', padding: '12px 20px',
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
            onload: resp => { if (resp.status === 200 || resp.status === 201) try { resolve(JSON.parse(resp.responseText).id); } catch (e) { reject('解析失败'); } else reject(`状态码 ${resp.status}`); },
            onerror: err => reject(`网络错误: ${err}`)
        });
    });

    // ========== 进度弹窗组件 ==========
    const createProgressModal = (taskId, taskName, url) => {
        const modal = document.createElement('div');
        modal.id = `download-progress-${taskId}`;
        Object.assign(modal.style, { position: 'fixed', top: '80px', right: '20px', width: '320px', backgroundColor: '#fff', borderRadius: '8px', boxShadow: '0 4px 20px rgba(0,0,0,0.3)', zIndex: '10001', fontFamily: 'sans-serif', transition: 'all 0.2s ease', overflow: 'hidden' });
        modal.innerHTML = `<div class="full-view"><div style="display:flex;justify-content:space-between;align-items:center;padding:12px 16px;background:#f5f5f5;border-bottom:1px solid #e0e0e0"><h3 style="margin:0;font-size:14px;font-weight:bold;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;max-width:180px">📥 ${taskName}</h3><div style="display:flex;gap:8px"><button class="minimize" style="background:none;border:none;font-size:18px;cursor:pointer;color:#666" title="最小化">−</button><button class="close" style="background:none;border:none;font-size:18px;cursor:pointer;color:#666" title="关闭">✖</button></div></div><div style="padding:16px;border-bottom:1px solid #f0f0f0"><div style="width:100%;background:#f0f0f0;border-radius:4px;overflow:hidden;margin-bottom:8px"><div class="progress-bar" style="width:0%;height:24px;background:#4caf50;text-align:center;line-height:24px;color:white;font-size:12px">0%</div></div><div class="status-text" style="font-size:12px;color:#666">状态：等待开始...</div></div><div style="padding:0 16px 16px 16px;font-size:10px;color:#999;word-break:break-all">${url}</div></div><div class="mini-view" style="display:none;width:50px;height:50px;border-radius:50%;background:#4caf50;cursor:pointer;align-items:center;justify-content:center;text-align:center;color:white;font-size:16px;font-weight:bold;box-shadow:0 2px 8px rgba(0,0,0,0.2)">0%</div>`;
        document.body.appendChild(modal);
        const fullView = modal.querySelector('.full-view');
        const miniView = modal.querySelector('.mini-view');
        const progressBar = modal.querySelector('.progress-bar');
        const statusDiv = modal.querySelector('.status-text');
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
                if (status === 'downloading') statusDiv.textContent = '状态：下载中...';
                else if (status === 'completed') { statusDiv.textContent = '✅ 下载完成！'; progressBar.style.backgroundColor = '#4caf50'; miniView.style.backgroundColor = '#4caf50'; miniView.textContent = '✓'; }
                else if (status === 'failed') { statusDiv.textContent = `❌ 下载失败: ${err || '未知错误'}`; progressBar.style.backgroundColor = '#f44336'; miniView.style.backgroundColor = '#f44336'; miniView.textContent = '✗'; }
                else if (status === 'pending') statusDiv.textContent = '状态：等待中...';
            },
            close: () => modal.remove()
        };
    };

    const monitorTask = async (taskId, taskName, url) => {
        const ui = createProgressModal(taskId, taskName, url);
        let ws, heartbeat, completed = false;
        const finalize = (success, err = '') => {
            if (completed) return;
            completed = true;
            clearTimeout(heartbeat); clearInterval(heartbeat);
            if (ws && ws.readyState === WebSocket.OPEN) ws.close();
            ui.close();
            showNotification(`${taskName} ${success ? '下载完成' : '下载失败' + (err ? ': ' + err : '')}`, success ? 'success' : 'error');
        };
        const connect = () => {
            ws = new WebSocket(`${BACKEND_URL.replace('http', 'ws')}/api/tasks/${taskId}/ws`);
            ws.onopen = () => { heartbeat = setInterval(() => { if (ws?.readyState === WebSocket.OPEN) ws.send('ping'); }, 30000); };
            ws.onmessage = e => {
                try {
                    let task = JSON.parse(e.data);
                    if (task.progress !== undefined) ui.updateProgress(task.progress);
                    if (task.status) ui.updateStatus(task.status, task.error);
                    if (task.progress >= 99.99 && !completed) finalize(true);
                    else if (task.status === 'completed' && !completed) finalize(true);
                    else if (task.status === 'failed' && !completed) finalize(false, task.error);
                } catch (e) { }
            };
            ws.onerror = () => { if (!completed) finalize(false, 'WebSocket错误'); };
            ws.onclose = () => { if (!completed) finalize(false, '连接中断'); };
        };
        connect();
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
        panel.innerHTML = `<div style="padding:12px 20px;border-bottom:1px solid #e0e0e0;display:flex;justify-content:space-between;align-items:center;background:#f5f5f5"><h3 style="margin:0">📺 m3u8 链接 (${links.length})</h3><button class="close-modal" style="background:none;border:none;font-size:20px;cursor:pointer;color:#666">✖</button></div><button id="copy-all" style="margin:0 20px 12px 20px;padding:6px 12px;background:#4caf50;color:white;border:none;border-radius:4px;cursor:pointer;font-size:14px;align-self:flex-start">📋 复制全部链接</button><div style="flex:1;overflow-y:auto;padding:16px 20px"><ul style="margin:0;padding-left:20px;list-style:decimal" id="link-list"></ul></div>`;
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
                    let taskId = await startDownload(link, `${fileName}_${Date.now()}`);
                    monitorTask(taskId, fileName, link);
                } catch (err) { showNotification(`启动下载失败: ${err}`, 'error'); }
            };
            li.appendChild(span);
            listContainer.appendChild(li);
        });
        panel.querySelector('.close-modal').onclick = () => modal.remove();
        panel.querySelector('#copy-all').onclick = async () => {
            try { await navigator.clipboard.writeText(links.join('\n')); panel.querySelector('#copy-all').textContent = '✅ 已复制！'; setTimeout(() => panel.querySelector('#copy-all').textContent = '📋 复制全部链接', 2000); } catch (e) { alert('复制失败'); }
        };
        modal.appendChild(panel);
        document.body.appendChild(modal);
    };

    function ensureButton() {
        if (realWin.top !== realWin.self) return;
        if (document.getElementById('tm-m3u8-btn')) return;
        if (!realWin.__m3u8Links?.size) return;
        const btn = document.createElement('button');
        btn.id = 'tm-m3u8-btn';
        btn.textContent = `🎬 1获取 m3u8 链接 (${realWin.__m3u8Links.size})`;
        Object.assign(btn.style, { position: 'fixed', bottom: '20px', right: '20px', zIndex: '9999', padding: '8px 16px', border: 'none', borderRadius: '6px', backgroundColor: '#fff', color: '#333', fontWeight: 'bold', cursor: 'pointer', boxShadow: '0 2px 6px rgba(0,0,0,0.2)', transition: 'background-color 0.2s ease', fontFamily: 'sans-serif' });
        btn.onmouseenter = () => btn.style.backgroundColor = '#e0e0e0';
        btn.onmouseleave = () => btn.style.backgroundColor = '#fff';
        btn.onclick = () => { if (realWin.__m3u8Links?.size) showLinksModal(Array.from(realWin.__m3u8Links)); };
        document.body.appendChild(btn);
        let last = -1;
        const update = () => {
            let c = realWin.__m3u8Links?.size || 0;
            if (c !== last) { last = c; btn.textContent = c ? `🎬1 获取 m3u8 链接 (${c})` : '🎬 1获取 m3u8 链接'; btn.style.display = c ? 'block' : 'none'; }
        };
        update();
        setInterval(update, 1000);
    }
})();