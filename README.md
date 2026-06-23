# OCR Translator

面向游戏、软件和网页内文字的轻量 OCR 翻译工具。项目是 clean-room 独立实现，不包含 LunaTranslator 的源码、资源、图标、配置文件或 UI。

## 当前状态

- Rust workspace 已建立：`app-core`、`app-windows`、`app-tauri`
- Tauri 设置界面已完成
- 支持配置源语言、目标语言、翻译源、浮窗参数、快捷键和浮窗颜色
- 全局输入低层监听已接入，支持鼠标侧键和键盘快捷键触发一次选区 OCR 翻译
- 全屏透明选区窗口已接入
- Win32 截图、Windows OCR、翻译调用、鼠标释放点附近浮窗显示已连通
- 已加入 SnippingTool OneOCR 运行库检测、打包内置和识别调用
- 浮窗支持窄宽度、透明度、字号、边缘避让、双击关闭、拖动、长文滚动和原文/译文背景色
- 翻译源保留传统网页源，不要求用户填写 API Key
- 已实现翻译源：Microsoft、ModernMt、有道、iTrans、Yandex、Papago、Bing、qqTranSmart、彩云、Lingva、QQ IMT、Google、阿里、DeepL、TranslateCom、火山
- 安装包：`target\release\bundle\nsis\OCR Translator_0.1.0_x64-setup.exe`
- MSI：`target\release\bundle\msi\OCR Translator_0.1.0_x64_en-US.msi`
- 便携版 zip：`installer\OCR-Translator-portable-v0.1.0.zip`

## 开发

```powershell
cd F:\AI\dn-ocr-translator
cargo check
cargo run -p ocr-translator
```

如果使用 npm/Tauri CLI，PowerShell 可能拦截 `npm.ps1`，可使用 `npm.cmd`。

## 打包

```powershell
cd F:\AI\dn-ocr-translator
powershell -ExecutionPolicy Bypass -File scripts\package_release.ps1
```

打包脚本会：

- 准备 `crates\app-tauri\resources\SnippingTool` 下的 OneOCR 运行库
- 运行格式检查、编译检查、UI 回归和 OCR 回归
- 生成 release exe、NSIS 安装包、MSI 和便携 zip

源码仓库不提交 `SnippingTool` OCR 二进制；安装包和便携包会在本机打包阶段携带这些文件，安装后无需用户再单独安装 OCR。

## 配置

首次启动会创建：

```text
%APPDATA%\OCR-Translator\config.json
```

默认值：

- 源语言：自动
- 目标语言：简体中文
- OCR：SnippingTool OneOCR
- 翻译源：Bing
- 快捷键：MouseX1
- 浮窗宽度：320px
- 屏幕边距：12px
- 双击关闭：开启

## 使用

1. 启动 `OCR-Translator.exe`。
2. 在设置页选择翻译源、快捷键和浮窗样式。
3. 游戏、软件或网页中按默认鼠标侧键 `MouseX1`。
4. 拉框选择英文区域，松开鼠标后会在松开位置附近显示译文浮窗。
5. 双击译文浮窗关闭。

快捷键可以填写 `MouseX1`、`MouseX2`、`F8`、`Ctrl+Shift+Q` 这类格式。监听只旁路触发 OCR，不会阻止游戏自己收到同一个按键。

每次 OCR 会保存实际截图到：

```text
%APPDATA%\OCR-Translator\last_capture.png
```

截图如果是黑图或不是选区内容，说明截图链路需要继续处理；截图正常但 OCR 为空，则优先确认 OneOCR 运行库是否可用。

## 合规说明

本项目不复制 LunaTranslator 的源码、资源或 UI。
OneOCR 运行库来自本机 Microsoft Snipping Tool 组件，源码仓库不包含该二进制。
