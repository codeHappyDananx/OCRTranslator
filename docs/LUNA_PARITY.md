# LunaTranslator Parity

本项目目标是以 Rust/Tauri 重新实现 LunaTranslator 中与截图 OCR 翻译相关的能力，不复制 LunaTranslator 源码、资源、图标或二进制。

## 翻译源

### 已接入请求逻辑

- `google`
- `bing`
- `ModernMt`
- `qqTranSmart`
- `qqimt`
- `youdaodict`
- `caiyun`
- `TranslateCom`
- `yandex`
- `huoshan`
- `itrans`
- `deepl_1`
- `deeplapi-free`
- `googleapi`
- `azure`
- `baiduapi`
- `youdaoapi`
- `caiyunapi`
- `yandexapi`
- `xiaoniu`
- `ibm`
- `tencentapi`
- `lingva`
- `chatgpt-3rd-party`
- `sakura`
- `sugoix`
- `selfbuild`

### 已列入目录但待实现

- `microsoft`
- `ali`
- `papago`
- `hanshant`
- `chromeai`
- `cdp_chatgpt`
- `aliyunapi`
- `huoshanapi`
- `hwcloud`
- `rengong`
- `premt`
- `atlas`
- `lec`
- `jb7`
- `kingsoft`
- `eztrans`
- `dreye`

本地商业翻译器只做外部调用适配，不内置商业引擎本体。

## OCR 引擎

### 已接入

- Windows OCR
  - 枚举系统可用 OCR 语言
  - 按源语言显式创建 OCR 引擎
  - 默认 `auto` 使用 `en-US`
  - 保存最近一次截图到 `%APPDATA%/OCR-Translator/last_capture.png`
- SnippingTool OneOCR
  - 检测本地 OneOCR 运行库
  - 可从 Microsoft ScreenSketch 包下载并解包 `oneocr.dll`、`oneocr.onemodel`、`onnxruntime.dll`
  - 已接入 OneOCR 动态加载和识别调用
  - 已加入“诊断上一张截图”入口，用当前 OCR 引擎重试 `%APPDATA%/OCR-Translator/last_capture.png`

### 待实现

- 本地 OCR
- 百度 OCR
- 腾讯 OCR
- 有道 OCR
- OCR.space
- 火山 OCR
- Google Cloud Vision
- ChatGPT-like OCR
- 讯飞 OCR
- Manga OCR

下一步优先级是验证 SnippingTool OneOCR 在本机下载的运行库上是否稳定，然后再补云 OCR 和本地 OCR。

## 截图/选区

### 已接入

- 全局鼠标侧键/键盘触发
- 透明选区窗口
- GDI 区域截图
- 选区截图诊断文件

### 待实现

- WinRT Graphics Capture 截图路线
- 多显示器坐标修正
- 游戏独占全屏兼容策略

如果 `last_capture.png` 是黑图或不是目标区域，下一步应优先实现 WinRT Graphics Capture。
