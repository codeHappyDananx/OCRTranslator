# Third Party Notice

本项目当前使用的主要第三方组件：

- Rust
- Tauri 2
- `windows` crate：调用 Win32 API
- `reqwest`：HTTP 请求
- `serde` / `serde_json`：配置和接口序列化
- `tokio`：异步运行时
- `hmac` / `sha2` / `md5`：传统翻译 API 签名
- `image`：截图 PNG 编码
- `dirs`：用户配置目录定位

本项目不包含 LunaTranslator 源码、资源、图标、二进制文件或配置文件。

发布安装包可能包含本机准备的 Microsoft Snipping Tool OneOCR 运行库，用于离线 OCR。该运行库不提交到源码仓库，使用和再分发需自行遵守 Microsoft 相关条款。
