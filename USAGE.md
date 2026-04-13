# M3U8 下载器服务

## 功能特性

- ✅ Web界面管理下载任务
- ✅ REST API接口
- ✅ WebSocket实时通知下载完成
- ✅ 任务状态管理（等待中、下载中、已完成、失败）
- ✅ 静态文件打包进二进制文件
- ✅ 支持批量下载和单任务下载
- ✅ URL 验证，防止 SSRF 攻击
- ✅ 文件路径验证，防止路径遍历攻击
- ✅ 配置值验证，确保输入合法

## 启动服务

### 默认启动（端口8080）
```bash
./m3u8_downloader
```

### 自定义配置启动
```bash
./m3u8_downloader serve --host 0.0.0.0 --port 8080 --concurrent 8
```

**参数验证规则**：
- 并发数（concurrent）：1-32
- 端口（port）：1-65535
- 主机（host）：有效的 IP 地址或域名

**错误示例**：
```
错误: 配置验证失败: concurrent - 并发数过大，最大值为 32
```

### 批量下载模式（保留原有功能）
```bash
./m3u8_downloader batch --file ./download_tasks.json --concurrent 8
```

## API 接口

### 1. 创建下载任务
```http
POST /api/download
Content-Type: application/json

{
  "name": "视频名称",
  "url": "https://example.com/video.m3u8",
  "output_dir": "./output"  // 可选
}
```

**验证规则**：
- URL 必须是 HTTP/HTTPS 协议
- 任务名称不能为空
- 输出路径不能包含路径遍历字符（如 `../`）

响应：
```json
{
  "id": "任务UUID",
  "status": "pending",
  "message": "下载任务已创建"
}
```

**错误示例**：
```json
{
  "error": "URL验证失败: ftp://example.com/video.m3u8 - 不支持的协议: ftp，仅支持 http/https"
}
```

### 2. 获取所有任务
```http
GET /api/tasks
```

### 3. 获取单个任务
```http
GET /api/tasks/{task_id}
```

### 4. 删除任务
```http
DELETE /api/tasks/{task_id}
```

### 5. 获取特定状态的任务
```http
GET /api/tasks/pending
GET /api/tasks/completed
GET /api/tasks/failed
```

### 6. WebSocket实时通知
```http
ws://localhost:8080/api/tasks/{task_id}/ws
```

WebSocket会实时推送任务状态，任务完成或失败后自动关闭连接。

## 任务状态

- `pending` - 等待中
- `downloading` - 下载中
- `completed` - 已完成
- `failed` - 失败

## Web界面

访问 http://localhost:8080 打开Web管理界面，功能包括：
- 创建下载任务
- 查看任务列表和状态
- 实时进度更新
- 任务筛选（全部/等待中/下载中/已完成/失败）
- 删除任务
- 下载完成通知

## 示例

### 使用curl创建任务
```bash
curl -X POST http://localhost:8080/api/download \
  -H "Content-Type: application/json" \
  -d '{"name": "test_video", "url": "https://example.com/video.m3u8"}'
```

### 使用JavaScript创建任务
```javascript
fetch('/api/download', {
  method: 'POST',
  headers: {
    'Content-Type': 'application/json'
  },
  body: JSON.stringify({
    name: 'test_video',
    url: 'https://example.com/video.m3u8'
  })
})
.then(res => res.json())
.then(data => console.log(data));
```

### WebSocket监听任务状态
```javascript
const ws = new WebSocket('ws://localhost:8080/api/tasks/{task_id}/ws');
ws.onmessage = (event) => {
  const task = JSON.parse(event.data);
  console.log('任务状态:', task.status, '进度:', task.progress + '%');
  if (task.status === 'completed') {
    console.log('下载完成！');
  }
};
```

## 编译

```bash
# 开发模式
cargo build

# 发布模式（优化）
cargo build --release
```

编译后的二进制文件在 `target/release/m3u8_downloader`

## 目录结构

```
m3u8_download/
├── src/
│   ├── main.rs           # 入口，支持CLI和服务模式
│   ├── server/           # Web服务模块
│   │   ├── mod.rs        # 路由配置
│   │   ├── handlers.rs   # API处理函数
│   │   └── state.rs      # 任务状态管理
│   ├── downloader/       # 下载核心逻辑
│   └── utils/            # 工具函数
├── static/
│   └── index.html        # Web界面（打包进二进制）
├── Cargo.toml
└── README.md
```
