# Simple M3U8 Finder

最小测试版 Chrome/Edge 扩展，用来捕获网页请求中的 `.m3u8` 链接，并提交给本项目的 Rust 服务下载。

## 使用步骤

1. 启动 Rust 服务：

```powershell
cargo run -- serve --port 8080
```

2. 打开浏览器扩展管理页，开启开发者模式。
3. 选择“加载已解压的扩展”，加载这个目录：

```text
D:\project\project\rust\m3u8_download\simple-m3u8-extension
```

4. 打开视频页面并播放，右下角出现 M3U8 面板后点击“下载”。

下载流程：扩展提交 `POST http://localhost:8080/api/download/stream/init`，然后让浏览器下载 `GET /api/download/stream/:id`。Rust 服务只使用临时目录处理切片和合并，直传结束后会删除临时文件，不会在服务器 `output` 目录保留最终 mp4。
