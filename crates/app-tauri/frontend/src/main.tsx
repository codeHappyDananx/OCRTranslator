import * as React from "react";
import { createRoot } from "react-dom/client";
import "./styles.css";
import { invoke, listen } from "./tauri";

type ProviderField = {
  key: string;
  label: string;
  required: boolean;
  secret: boolean;
};

type ProviderInfo = {
  id: string;
  name: string;
  implemented: boolean;
  experimental: boolean;
  fields: ProviderField[];
};

type OverlayConfig = {
  width: number;
  offset_x: number;
  offset_y: number;
  screen_margin: number;
  max_height: number;
  opacity: number;
  font_size: number;
  no_drag_ms: number;
  double_click_close: boolean;
  show_source: boolean;
  draggable: boolean;
  source_background: string;
  translation_background: string;
};

type AppConfig = {
  source_lang: string;
  target_lang: string;
  ocr_engine: string;
  translator: string;
  hotkey: string;
  provider_settings: Record<string, Record<string, string>>;
  overlay: OverlayConfig;
};

const defaultOverlay: OverlayConfig = {
  width: 320,
  offset_x: 0,
  offset_y: 0,
  screen_margin: 12,
  max_height: 620,
  opacity: 0.55,
  font_size: 18,
  no_drag_ms: 500,
  double_click_close: true,
  show_source: true,
  draggable: true,
  source_background: "#2858a5",
  translation_background: "#127858",
};

function normalizeKeyName(event: KeyboardEvent) {
  const aliases: Record<string, string> = {
    " ": "Space",
    Escape: "Esc",
    Control: "",
    Shift: "",
    Alt: "",
    Meta: "",
  };
  const key = aliases[event.key] ?? (event.key.length === 1 ? event.key.toUpperCase() : event.key);
  return key;
}

function hotkeyFromKeyboard(event: KeyboardEvent) {
  const key = normalizeKeyName(event);
  if (!key) return "";
  const parts = [];
  if (event.ctrlKey) parts.push("Ctrl");
  if (event.altKey) parts.push("Alt");
  if (event.shiftKey) parts.push("Shift");
  parts.push(key);
  return parts.join("+");
}

function SettingsApp() {
  const [config, setConfig] = React.useState<AppConfig | null>(null);
  const [providers, setProviders] = React.useState<ProviderInfo[]>([]);
  const [status, setStatus] = React.useState("");
  const [recordingHotkey, setRecordingHotkey] = React.useState(false);
  const saveTimer = React.useRef<number | null>(null);

  const implementedProviders = React.useMemo(
    () =>
      providers
        .filter((provider) => provider.implemented && !provider.experimental)
        .sort((a, b) => a.name.localeCompare(b.name, "zh-CN")),
    [providers],
  );
  const selectedProvider = providers.find((provider) => provider.id === config?.translator);

  const queueSave = React.useCallback((next: AppConfig) => {
    if (saveTimer.current) window.clearTimeout(saveTimer.current);
    saveTimer.current = window.setTimeout(async () => {
      try {
        await invoke("save_config", { config: next });
        setStatus("设置已自动保存");
      } catch (error) {
        setStatus(String(error));
      }
    }, 180);
  }, []);

  const updateConfig = React.useCallback(
    (updater: (current: AppConfig) => AppConfig) => {
      setConfig((current) => {
        if (!current) return current;
        const next = updater(current);
        queueSave(next);
        return next;
      });
    },
    [queueSave],
  );

  React.useEffect(() => {
    let cancelled = false;
    async function boot() {
      const providerList = await invoke<ProviderInfo[]>("list_providers");
      if (cancelled) return;
      setProviders(providerList);
      try {
        const engines = await invoke<Array<{ id: string; available: boolean }>>("list_ocr_engines");
        const oneOcr = engines.find((engine) => engine.id === "snippingtool");
        if (oneOcr?.available) {
          setStatus("OCR：SnippingTool OneOCR 已就绪");
        } else {
          setStatus("OCR：正在准备 SnippingTool OneOCR 运行库...");
          await invoke("install_oneocr_runtime");
          setStatus("OCR：SnippingTool OneOCR 已就绪");
        }
      } catch (error) {
        setStatus(`OCR：OneOCR 准备失败，${String(error)}`);
      }
      const loaded = await invoke<AppConfig>("get_config");
      if (cancelled) return;
      const fallbackProvider =
        providerList.find((provider) => provider.id === "bing") ??
        providerList.find((provider) => provider.implemented && !provider.experimental);
      setConfig({
        ...loaded,
        translator:
          providerList.find((provider) => provider.id === loaded.translator && provider.implemented)
            ?.id ?? fallbackProvider?.id ?? "bing",
        ocr_engine: "snippingtool",
        overlay: { ...defaultOverlay, ...loaded.overlay },
      });
    }
    boot().catch((error) => {
      setStatus(String(error));
    });
    listen<string>("ocr-status", (event) => setStatus(event.payload)).catch(() => {});
    listen("ocr-hotkey", () => setStatus("快捷键已触发，正在选择 OCR 区域...")).catch(() => {});
    return () => {
      cancelled = true;
      if (saveTimer.current) window.clearTimeout(saveTimer.current);
    };
  }, []);

  React.useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (!recordingHotkey) return;
      event.preventDefault();
      event.stopPropagation();
      const hotkey = hotkeyFromKeyboard(event);
      if (!hotkey) return;
      setRecordingHotkey(false);
      updateConfig((current) => ({ ...current, hotkey }));
    }
    function onMouseDown(event: MouseEvent) {
      if (!recordingHotkey || (event.button !== 3 && event.button !== 4)) return;
      event.preventDefault();
      event.stopPropagation();
      setRecordingHotkey(false);
      updateConfig((current) => ({ ...current, hotkey: event.button === 3 ? "MouseX1" : "MouseX2" }));
    }
    function onContextMenu(event: MouseEvent) {
      if (recordingHotkey) event.preventDefault();
    }
    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("mousedown", onMouseDown, true);
    window.addEventListener("contextmenu", onContextMenu, true);
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("mousedown", onMouseDown, true);
      window.removeEventListener("contextmenu", onContextMenu, true);
    };
  }, [recordingHotkey, updateConfig]);

  if (!config) {
    return <main className="shell">正在加载...</main>;
  }

  const setOverlay = (patch: Partial<OverlayConfig>) =>
    updateConfig((current) => ({
      ...current,
      overlay: { ...current.overlay, ...patch },
    }));

  return (
    <main className="shell">
      <section className="topbar">
        <div>
          <h1>OCR Translator</h1>
          <p>独立实现的截图 OCR 翻译工具，适合游戏、软件和网页内快速选区翻译。</p>
        </div>
      </section>

      <section className="grid">
        <article className="panel">
          <h2>基础设置</h2>
          <label className="field">
            源语言
            <select
              value={config.source_lang}
              onChange={(event) =>
                updateConfig((current) => ({ ...current, source_lang: event.target.value }))
              }
            >
              <option value="auto">自动</option>
              <option value="en">英语</option>
              <option value="ja">日语</option>
              <option value="ko">韩语</option>
              <option value="zh-CN">简体中文</option>
            </select>
          </label>
          <label className="field">
            目标语言
            <select
              value={config.target_lang}
              onChange={(event) =>
                updateConfig((current) => ({ ...current, target_lang: event.target.value }))
              }
            >
              <option value="zh-CN">简体中文</option>
              <option value="en">英语</option>
              <option value="ja">日语</option>
              <option value="ko">韩语</option>
            </select>
          </label>
          <label className="field">
            快捷键
            <input
              className={`hotkey-input${recordingHotkey ? " recording" : ""}`}
              type="text"
              readOnly
              value={recordingHotkey ? "请按键或鼠标侧键..." : config.hotkey}
              onFocus={() => setRecordingHotkey(true)}
              onClick={() => setRecordingHotkey(true)}
              onBlur={() => setRecordingHotkey(false)}
            />
          </label>
          <div className="hint">OCR：SnippingTool OneOCR</div>
        </article>

        <article className="panel">
          <h2>浮窗设置</h2>
          <label className="field">
            默认宽度
            <input
              type="number"
              min={180}
              max={900}
              step={10}
              value={config.overlay.width}
              onChange={(event) => setOverlay({ width: Number(event.target.value || 320) })}
            />
          </label>
          <label className="field">
            最大高度
            <input
              type="number"
              min={120}
              max={1200}
              step={10}
              value={config.overlay.max_height}
              onChange={(event) => setOverlay({ max_height: Number(event.target.value || 620) })}
            />
          </label>
          <label className="field">
            字体大小
            <input
              type="number"
              min={12}
              max={48}
              step={1}
              value={config.overlay.font_size}
              onChange={(event) => setOverlay({ font_size: Number(event.target.value || 18) })}
            />
          </label>
          <label className="field">
            背景透明度
            <input
              type="number"
              min={0.05}
              max={0.9}
              step={0.05}
              value={config.overlay.opacity}
              onChange={(event) => setOverlay({ opacity: Number(event.target.value || 0.55) })}
            />
          </label>
          <div className="color-grid">
            <label className="field">
              原文背景
              <input
                type="color"
                value={config.overlay.source_background}
                onChange={(event) => setOverlay({ source_background: event.target.value })}
              />
            </label>
            <label className="field">
              译文背景
              <input
                type="color"
                value={config.overlay.translation_background}
                onChange={(event) => setOverlay({ translation_background: event.target.value })}
              />
            </label>
          </div>
          <label className="field">
            屏幕边距
            <input
              type="number"
              min={0}
              max={120}
              step={1}
              value={config.overlay.screen_margin}
              onChange={(event) => setOverlay({ screen_margin: Number(event.target.value || 12) })}
            />
          </label>
          <label className="field check">
            <input
              type="checkbox"
              checked={config.overlay.double_click_close}
              onChange={(event) => setOverlay({ double_click_close: event.target.checked })}
            />
            双击关闭
          </label>
          <label className="field check">
            <input
              type="checkbox"
              checked={config.overlay.show_source}
              onChange={(event) => setOverlay({ show_source: event.target.checked })}
            />
            原文在上，译文在下
          </label>
          <label className="field check">
            <input
              type="checkbox"
              checked={config.overlay.draggable}
              onChange={(event) => setOverlay({ draggable: event.target.checked })}
            />
            允许拖动浮窗
          </label>
        </article>

        <article className="panel wide">
          <h2>翻译源</h2>
          <div className="provider-row">
            <label className="field">
              翻译源
              <select
                value={config.translator}
                onChange={(event) =>
                  updateConfig((current) => ({ ...current, translator: event.target.value }))
                }
              >
                {implementedProviders.map((provider) => (
                  <option key={provider.id} value={provider.id}>
                    {provider.name}
                  </option>
                ))}
              </select>
            </label>
            <span className="badge">{selectedProvider ? "已实现" : ""}</span>
          </div>
          <div className="fields">
            {selectedProvider?.fields.map((field) => (
              <label key={field.key} className="field">
                {field.label}
                {field.required ? " *" : ""}
                <input
                  type={field.secret ? "password" : "text"}
                  value={config.provider_settings?.[selectedProvider.id]?.[field.key] ?? ""}
                  onChange={(event) =>
                    updateConfig((current) => ({
                      ...current,
                      provider_settings: {
                        ...current.provider_settings,
                        [selectedProvider.id]: {
                          ...(current.provider_settings?.[selectedProvider.id] ?? {}),
                          [field.key]: event.target.value,
                        },
                      },
                    }))
                  }
                />
              </label>
            ))}
          </div>
        </article>
      </section>
      <div className="status footer-status">{status}</div>
    </main>
  );
}

createRoot(document.getElementById("root")!).render(<SettingsApp />);
