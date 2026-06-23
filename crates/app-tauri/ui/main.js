const invoke = (...args) => window.__TAURI__.core.invoke(...args);
const listen = (...args) => window.__TAURI__.event.listen(...args);

const els = {
  sourceLang: document.querySelector("#sourceLang"),
  targetLang: document.querySelector("#targetLang"),
  hotkey: document.querySelector("#hotkey"),
  ocrStatus: document.querySelector("#ocrStatus"),
  overlayWidth: document.querySelector("#overlayWidth"),
  fontSize: document.querySelector("#fontSize"),
  opacity: document.querySelector("#opacity"),
  sourceBackground: document.querySelector("#sourceBackground"),
  translationBackground: document.querySelector("#translationBackground"),
  screenMargin: document.querySelector("#screenMargin"),
  doubleClickClose: document.querySelector("#doubleClickClose"),
  showSource: document.querySelector("#showSource"),
  overlayDraggable: document.querySelector("#overlayDraggable"),
  translator: document.querySelector("#translator"),
  providerBadge: document.querySelector("#providerBadge"),
  providerFields: document.querySelector("#providerFields"),
  status: document.querySelector("#status"),
};

let config = null;
let providers = [];
let ocrEngines = [];
let recordingHotkey = false;
let saveTimer = null;

function setStatus(text) {
  els.status.textContent = text;
}

function providerById(id) {
  return providers.find((p) => p.id === id);
}

function formatBytes(bytes) {
  if (!bytes && bytes !== 0) return "未知大小";
  if (bytes >= 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`;
  if (bytes >= 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(2)} MB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(2)} KB`;
  return `${bytes} B`;
}

function applyConfig(cfg) {
  config = cfg;
  const selectedProvider = providerById(cfg.translator);
  const selectedSettings = {};
  const missingRequired =
    selectedProvider?.fields?.some((field) => field.required && !String(selectedSettings[field.key] ?? "").trim()) ??
    false;
  if (!selectedProvider || !selectedProvider.implemented || missingRequired) {
    config.translator = providerById("bing") ? "bing" : (providers.find((p) => p.implemented && !p.experimental)?.id ?? "bing");
  }
  els.sourceLang.value = cfg.source_lang;
  els.targetLang.value = cfg.target_lang;
  config.ocr_engine = "snippingtool";
  els.hotkey.value = cfg.hotkey;
  els.overlayWidth.value = cfg.overlay.width;
  els.fontSize.value = cfg.overlay.font_size;
  els.opacity.value = cfg.overlay.opacity;
  els.sourceBackground.value = cfg.overlay.source_background ?? "#2858a5";
  els.translationBackground.value = cfg.overlay.translation_background ?? "#127858";
  els.screenMargin.value = cfg.overlay.screen_margin;
  els.doubleClickClose.checked = cfg.overlay.double_click_close;
  els.showSource.checked = cfg.overlay.show_source === true;
  els.overlayDraggable.checked = cfg.overlay.draggable !== false;
  els.translator.value = config.translator;
  renderProviderFields();
}

function collectConfig() {
  const current = config ?? {};
  return {
    source_lang: els.sourceLang.value,
    target_lang: els.targetLang.value,
    ocr_engine: "snippingtool",
    translator: els.translator.value,
    hotkey: els.hotkey.value.trim() || "MouseX1",
    provider_settings: {},
    overlay: {
      width: Number(els.overlayWidth.value || 320),
      offset_x: 0,
      offset_y: 0,
      screen_margin: Number(els.screenMargin.value || 12),
      opacity: Number(els.opacity.value || 0.55),
      source_background: els.sourceBackground.value || "#2858a5",
      translation_background: els.translationBackground.value || "#127858",
      font_size: Number(els.fontSize.value || 18),
      no_drag_ms: current.overlay?.no_drag_ms ?? 500,
      double_click_close: els.doubleClickClose.checked,
      show_source: els.showSource.checked,
      draggable: els.overlayDraggable.checked,
    },
  };
}

function renderProviders() {
  els.translator.innerHTML = "";
  const ordered = providers
    .filter((provider) => provider.implemented && !provider.experimental)
    .sort((a, b) => a.name.localeCompare(b.name, "zh-CN"));
  for (const provider of ordered) {
    const option = document.createElement("option");
    option.value = provider.id;
    option.textContent = provider.name;
    els.translator.appendChild(option);
  }
}

function renderProviderFields() {
  const provider = providerById(els.translator.value);
  els.providerFields.innerHTML = "";
  if (!provider) return;

  const flags = [];
  flags.push("已实现");
  els.providerBadge.textContent = flags.join(" / ");

  const settings = config?.provider_settings?.[provider.id] ?? {};
  for (const field of provider.fields) {
    const label = document.createElement("label");
    label.textContent = field.label + (field.required ? " *" : "");
    const input = document.createElement("input");
    input.dataset.provider = provider.id;
    input.dataset.key = field.key;
    input.type = field.secret ? "password" : "text";
    input.value = settings[field.key] ?? "";
    input.addEventListener("input", () => {
      config.provider_settings ??= {};
      config.provider_settings[provider.id] ??= {};
      config.provider_settings[provider.id][field.key] = input.value;
      scheduleSave();
    });
    label.appendChild(input);
    els.providerFields.appendChild(label);
  }
}

async function save() {
  const next = collectConfig();
  next.provider_settings = {};
  await invoke("save_config", { config: next });
  applyConfig(next);
  setStatus("设置已自动保存");
}

function scheduleSave() {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(async () => {
    try {
      await save();
    } catch (e) {
      setStatus(String(e));
    }
  }, 180);
}

function normalizeKeyName(event) {
  const key = event.key;
  if (!key) return "";
  const aliases = {
    " ": "Space",
    Escape: "Esc",
    Control: "",
    Shift: "",
    Alt: "",
    Meta: "",
  };
  return aliases[key] ?? (key.length === 1 ? key.toUpperCase() : key);
}

function hotkeyFromKeyboard(event) {
  const key = normalizeKeyName(event);
  if (!key) return "";
  const parts = [];
  if (event.ctrlKey) parts.push("Ctrl");
  if (event.altKey) parts.push("Alt");
  if (event.shiftKey) parts.push("Shift");
  parts.push(key);
  return parts.join("+");
}

async function setHotkey(value) {
  els.hotkey.value = value;
  config.hotkey = value;
  await save();
}

function startHotkeyRecording() {
  recordingHotkey = true;
  els.hotkey.value = "请按键或鼠标侧键...";
  els.hotkey.classList.add("recording");
}

function stopHotkeyRecording() {
  recordingHotkey = false;
  els.hotkey.classList.remove("recording");
  els.hotkey.value = config?.hotkey || "MouseX1";
}

async function boot() {
  providers = await invoke("list_providers");
  renderProviders();
  try {
    ocrEngines = await invoke("list_ocr_engines");
    const oneOcr = ocrEngines.find((engine) => engine.id === "snippingtool");
    if (oneOcr?.available) {
      els.ocrStatus.textContent = "OCR：SnippingTool OneOCR 已就绪";
    } else {
      els.ocrStatus.textContent = "OCR：正在准备 SnippingTool OneOCR 运行库...";
      await invoke("install_oneocr_runtime");
      els.ocrStatus.textContent = "OCR：SnippingTool OneOCR 已就绪";
    }
  } catch (e) {
    els.ocrStatus.textContent = `OCR：OneOCR 准备失败，${String(e)}`;
  }
  applyConfig(await invoke("get_config"));
  els.translator.addEventListener("change", () => {
    config.translator = els.translator.value;
    renderProviderFields();
    scheduleSave();
  });
  [
    els.sourceLang,
    els.targetLang,
    els.overlayWidth,
    els.fontSize,
    els.opacity,
    els.sourceBackground,
    els.translationBackground,
    els.screenMargin,
    els.doubleClickClose,
    els.showSource,
    els.overlayDraggable,
  ].forEach((el) => {
    el.addEventListener("change", scheduleSave);
  });
  els.hotkey.addEventListener("focus", startHotkeyRecording);
  els.hotkey.addEventListener("click", startHotkeyRecording);
  els.hotkey.addEventListener("blur", stopHotkeyRecording);
  window.addEventListener("keydown", async (event) => {
    if (!recordingHotkey) return;
    event.preventDefault();
    event.stopPropagation();
    const hotkey = hotkeyFromKeyboard(event);
    if (hotkey) {
      stopHotkeyRecording();
      await setHotkey(hotkey);
    }
  });
  window.addEventListener("mousedown", async (event) => {
    if (!recordingHotkey) return;
    if (event.button !== 3 && event.button !== 4) return;
    event.preventDefault();
    event.stopPropagation();
    stopHotkeyRecording();
    await setHotkey(event.button === 3 ? "MouseX1" : "MouseX2");
  });
  window.addEventListener("contextmenu", (event) => {
    if (recordingHotkey) event.preventDefault();
  });
  await listen("ocr-status", (event) => setStatus(event.payload));
  await listen("ocr-hotkey", () => setStatus("快捷键已触发，正在选择 OCR 区域..."));
}

boot().catch((e) => {
  document.body.innerHTML = `<pre>${String(e)}</pre>`;
});

