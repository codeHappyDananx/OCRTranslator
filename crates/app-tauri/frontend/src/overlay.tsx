import * as React from "react";
import { createRoot } from "react-dom/client";
import "./styles.css";
import { Card, CardContent } from "./components/ui/card";
import { ScrollArea } from "./components/ui/scroll-area";
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
  type ResizablePanelHandle,
} from "./components/ui/resizable";
import { invoke, listen, type Unlisten } from "./tauri";

document.documentElement.classList.add("overlay-html");

type OverlayPayload = {
  result_mode: "text_overlay" | "image_replace";
  text: string;
  raw_text: string;
  width: number;
  image_width: number;
  image_height: number;
  source_image_data_url?: string | null;
  image_blocks: ImageReplacementBlock[];
  opacity: number;
  font_size: number;
  max_height: number;
  source_background: string;
  translation_background: string;
  double_click_close: boolean;
  show_source: boolean;
  draggable: boolean;
  log_entry_id?: string | null;
};

type ImageReplacementBlock = {
  source_text: string;
  translated_text: string;
  x: number;
  y: number;
  width: number;
  height: number;
  font_size: number;
  background: string;
  color: string;
  align: "left" | "center" | "right";
  wrap_mode?: "wrap" | "single";
};

const emptyPayload: OverlayPayload = {
  result_mode: "text_overlay",
  text: "",
  raw_text: "",
  width: 320,
  image_width: 320,
  image_height: 240,
  source_image_data_url: null,
  image_blocks: [],
  opacity: 0.55,
  font_size: 18,
  max_height: 620,
  source_background: "#2858a5",
  translation_background: "#127858",
  double_click_close: true,
  show_source: true,
  draggable: true,
  log_entry_id: null,
};

function cleanDisplayText(value: unknown) {
  return String(value ?? "")
    .replace(/\r\n/g, "\n")
    .split("\n")
    .map((line) => line.trim())
    .join("\n")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}

function hexToRgba(hex: string | undefined, opacity: number, fallback: string) {
  const value = /^#[0-9a-f]{6}$/i.test(String(hex ?? "")) ? String(hex) : fallback;
  const intValue = Number.parseInt(value.slice(1), 16);
  const r = (intValue >> 16) & 255;
  const g = (intValue >> 8) & 255;
  const b = intValue & 255;
  return `rgba(${r}, ${g}, ${b}, ${opacity})`;
}

function TranslationSection({
  title,
  text,
  titleVisible,
  className,
  style,
}: {
  title: string;
  text: string;
  titleVisible: boolean;
  className: string;
  style: React.CSSProperties;
}) {
  return (
    <section className={`translation-section ${className}`} style={style}>
      <ScrollArea>
        <div className="section-inner">
          {titleVisible ? <div className="section-title">{title}</div> : null}
          <pre className={`section-text ${className === "source-section" ? "source-text" : "translation-text"}`}>
            {text}
          </pre>
        </div>
      </ScrollArea>
    </section>
  );
}

function rgbaFromHex(hex: string, opacity = 1) {
  if (!/^#[0-9a-f]{6}$/i.test(hex)) return `rgba(5, 5, 5, ${opacity})`;
  const intValue = Number.parseInt(hex.slice(1), 16);
  const r = (intValue >> 16) & 255;
  const g = (intValue >> 8) & 255;
  const b = intValue & 255;
  return `rgba(${r}, ${g}, ${b}, ${opacity})`;
}

function loadCanvasImage(src: string) {
  return new Promise<HTMLImageElement>((resolve, reject) => {
    const image = new Image();
    image.onload = () => resolve(image);
    image.onerror = () => reject(new Error("image load failed"));
    image.src = src;
  });
}

function wrapCanvasText(
  context: CanvasRenderingContext2D,
  text: string,
  maxWidth: number,
) {
  const rows: string[] = [];
  for (const sourceLine of cleanDisplayText(text).split("\n")) {
    let current = "";
    for (const char of Array.from(sourceLine || " ")) {
      const next = current + char;
      if (current && context.measureText(next).width > maxWidth) {
        rows.push(current);
        current = char.trimStart();
      } else {
        current = next;
      }
    }
    rows.push(current);
  }
  return rows;
}

async function renderTranslationLogImage(payload: OverlayPayload) {
  if (!payload.source_image_data_url) return null;
  const width = Math.max(1, Math.round(payload.image_width));
  const height = Math.max(1, Math.round(payload.image_height));
  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  const context = canvas.getContext("2d");
  if (!context) return null;

  const source = await loadCanvasImage(payload.source_image_data_url);
  context.drawImage(source, 0, 0, width, height);

  for (const block of payload.image_blocks) {
    const blockX = Math.round(block.x);
    const blockY = Math.round(block.y);
    const blockWidth = Math.max(1, Math.round(block.width));
    const blockHeight = Math.max(1, Math.round(block.height));
    const fontSize = Math.max(9, Math.round(block.font_size || payload.font_size || 18));
    const paddingX = 2;
    const lineHeight = fontSize * 1.26;
    context.fillStyle = rgbaFromHex(block.background, 0.985);
    context.fillRect(blockX, blockY, blockWidth, blockHeight);
    context.font = `500 ${fontSize}px "Microsoft YaHei UI", "Segoe UI", Arial, sans-serif`;
    context.fillStyle = /^#[0-9a-f]{6}$/i.test(block.color) ? block.color : "#ffffff";
    context.textBaseline = "top";
    context.textAlign = block.align === "right" ? "right" : block.align === "center" ? "center" : "left";

    const rows =
      block.wrap_mode === "single"
        ? [cleanDisplayText(block.translated_text).replace(/\s*\n+\s*/g, " ")]
        : wrapCanvasText(context, block.translated_text, Math.max(1, blockWidth - paddingX * 2));
    const maxRows = Math.max(1, Math.floor(blockHeight / lineHeight));
    const visibleRows = rows.slice(0, maxRows);
    if (rows.length > maxRows && visibleRows.length > 0) {
      visibleRows[visibleRows.length - 1] = `${visibleRows[visibleRows.length - 1].replace(/…$/, "")}…`;
    }
    const textHeight = visibleRows.length * lineHeight;
    const startY =
      block.align === "center"
        ? blockY + Math.max(0, (blockHeight - textHeight) / 2)
        : blockY;
    const textX =
      block.align === "right"
        ? blockX + blockWidth - paddingX
        : block.align === "center"
          ? blockX + blockWidth / 2
          : blockX + paddingX;
    visibleRows.forEach((row, index) => {
      context.fillText(row, textX, startY + index * lineHeight, blockWidth - paddingX * 2);
    });
  }

  return canvas.toDataURL("image/png");
}

function OverlayApp() {
  const [payload, setPayload] = React.useState<OverlayPayload>(emptyPayload);
  const [userSized, setUserSized] = React.useState(false);
  const [viewportSize, setViewportSize] = React.useState({
    width: window.innerWidth || 1,
    height: window.innerHeight || 1,
  });
  const cardRef = React.useRef<HTMLDivElement | null>(null);
  const sourcePanelRef = React.useRef<ResizablePanelHandle | null>(null);
  const translationPanelRef = React.useRef<ResizablePanelHandle | null>(null);
  const lastResize = React.useRef({ width: 0, height: 0 });
  const ignoreResizeUntil = React.useRef(0);
  const loggedEntry = React.useRef<string | null>(null);

  const rawText = cleanDisplayText(payload.raw_text);
  const translatedText = cleanDisplayText(payload.text) || "无翻译结果";
  const imageReplaceMode =
    payload.result_mode === "image_replace" &&
    Boolean(payload.source_image_data_url) &&
    payload.image_blocks.length > 0;
  const showSource = payload.show_source !== false && rawText.length > 0;
  const opacity = payload.opacity ?? 0.55;
  const maxHeight = Math.max(120, Number(payload.max_height ?? 620));
  const sourceBackground = hexToRgba(payload.source_background, opacity, "#2858a5");
  const translationBackground = hexToRgba(
    payload.translation_background,
    opacity,
    "#127858",
  );
  const imageScale =
    imageReplaceMode && payload.image_width > 0 && payload.image_height > 0
      ? Math.min(
          1,
          viewportSize.width / payload.image_width,
          viewportSize.height / payload.image_height,
        )
      : 1;
  const imageDisplayWidth = Math.max(1, payload.image_width * imageScale);
  const imageDisplayHeight = Math.max(1, payload.image_height * imageScale);

  React.useEffect(() => {
    let disposed = false;
    let unlisten: Unlisten | null = null;
    listen<OverlayPayload>("overlay-show", (event) => {
      if (disposed) return;
      setUserSized(false);
      lastResize.current = { width: 0, height: 0 };
      setPayload({ ...emptyPayload, ...event.payload });
    })
      .then((value) => {
        unlisten = value;
      })
      .catch(() => {});
    invoke<OverlayPayload | null>("get_overlay_payload")
      .then((value) => {
        if (value && !disposed) setPayload({ ...emptyPayload, ...value });
      })
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  React.useLayoutEffect(() => {
    if (userSized) return;
    let frame = 0;
    const resize = () => {
      const card = cardRef.current;
      if (!card) return;
      if (imageReplaceMode) {
        const width = Math.max(80, Math.ceil(payload.image_width || payload.width || 320));
        const height = Math.max(36, Math.ceil(payload.image_height || 240));
        if (
          Math.abs(lastResize.current.width - width) <= 1 &&
          Math.abs(lastResize.current.height - height) <= 1
        ) {
          return;
        }
        lastResize.current = { width, height };
        ignoreResizeUntil.current = performance.now() + 300;
        invoke("resize_overlay_to_content", {
          request: { width, height, mode: "image_replace" },
        }).catch(() => {});
        return;
      }
      const width = Math.max(180, Math.ceil(payload.width || card.scrollWidth || 320));
      const sections = Array.from(card.querySelectorAll<HTMLElement>(".section-inner"));
      const sectionHeights = sections.map((section) => Math.ceil(section.scrollHeight));
      const handleHeight = showSource ? 2 : 0;
      const measuredHeight = sectionHeights.reduce((sum, height) => sum + height, 0) + handleHeight;
      const fallbackHeight = Math.ceil(card.scrollHeight || card.getBoundingClientRect().height || 54);
      const minHeight = showSource ? 118 : 54;
      const contentHeight = Math.max(measuredHeight, fallbackHeight, minHeight);
      const height = Math.max(minHeight, Math.min(contentHeight, maxHeight));
      if (showSource && sectionHeights.length >= 2) {
        const total = Math.max(1, sectionHeights[0] + sectionHeights[1]);
        const sourceSize = Math.min(72, Math.max(28, (sectionHeights[0] / total) * 100));
        sourcePanelRef.current?.resize(sourceSize);
        translationPanelRef.current?.resize(100 - sourceSize);
      }
      if (
        Math.abs(lastResize.current.width - width) <= 1 &&
        Math.abs(lastResize.current.height - height) <= 1
      ) {
        return;
      }
      lastResize.current = { width, height };
      ignoreResizeUntil.current = performance.now() + 300;
      invoke("resize_overlay_to_content", {
        request: { width, height, mode: "text_overlay" },
      }).catch(() => {});
    };
    frame = window.requestAnimationFrame(resize);
    return () => window.cancelAnimationFrame(frame);
  }, [payload, imageReplaceMode, showSource, rawText, translatedText, maxHeight, userSized]);

  React.useEffect(() => {
    const entryId = payload.log_entry_id;
    if (!imageReplaceMode || !entryId || loggedEntry.current === entryId) return;
    loggedEntry.current = entryId;
    renderTranslationLogImage(payload)
      .then((translatedImageDataUrl) => {
        if (!translatedImageDataUrl) return;
        return invoke("save_translation_log_render", {
          request: {
            entry_id: entryId,
            translated_image_data_url: translatedImageDataUrl,
          },
        });
      })
      .catch(() => {});
  }, [payload, imageReplaceMode]);

  React.useEffect(() => {
    function onResize() {
      setViewportSize({
        width: window.innerWidth || 1,
        height: window.innerHeight || 1,
      });
      if (performance.now() < ignoreResizeUntil.current) return;
      setUserSized(true);
    }
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  async function closeOverlay() {
    if (payload.double_click_close !== false) {
      await invoke("close_overlay").catch(() => {});
    }
  }

  async function startDrag(event: React.MouseEvent) {
    if (payload.draggable === false || event.button !== 0 || event.detail > 1) return;
    if ((event.target as HTMLElement).closest("[data-no-window-drag]")) return;
    event.preventDefault();
    await invoke("start_overlay_drag").catch(() => {});
  }

  async function startResize(event: React.PointerEvent<HTMLButtonElement>) {
    if (event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    setUserSized(true);
    await invoke("start_overlay_resize_corner").catch(() => {});
  }

  const fontSize = `${payload.font_size ?? 18}px`;
  const replacementFontSize = `${Math.max(14, Math.min(payload.font_size ?? 18, 30))}px`;

  return (
    <div className="overlay-stage" onDoubleClick={closeOverlay}>
      <Card
        ref={cardRef}
        className={`${imageReplaceMode ? "image-replace-card" : "translation-card"}${
          payload.draggable !== false ? " draggable" : ""
        }`}
        style={
          imageReplaceMode
            ? {
                width: imageDisplayWidth,
                height: imageDisplayHeight,
                fontSize: replacementFontSize,
              }
            : { fontSize, maxHeight }
        }
        onMouseDown={startDrag}
      >
        {imageReplaceMode ? (
          <div
            className="image-replace-surface"
            style={{
              width: payload.image_width,
              height: payload.image_height,
              transform: `scale(${imageScale})`,
            }}
          >
            <img
              className="image-replace-source"
              src={payload.source_image_data_url ?? ""}
              aria-hidden="true"
            />
            <div className="image-replace-block-layer" aria-label="图片翻译结果">
              {payload.image_blocks.map((block, index) => (
                <div
                  key={`${index}-${block.x}-${block.y}`}
                  className={`image-replace-block align-${block.align || "left"}`}
                  data-wrap={block.wrap_mode || "wrap"}
                  title={block.source_text}
                  style={{
                    left: block.x,
                    top: block.y,
                    width: block.width,
                    height: block.height,
                    color: block.color,
                    background: rgbaFromHex(block.background, 0.985),
                    fontSize: block.font_size,
                  }}
                >
                  <span>{cleanDisplayText(block.translated_text)}</span>
                </div>
              ))}
            </div>
          </div>
        ) : (
          <>
            <CardContent>
              {showSource ? (
                <ResizablePanelGroup direction="vertical">
                  <ResizablePanel ref={sourcePanelRef} defaultSize={48} minSize={24}>
                    <TranslationSection
                      title="原文"
                      text={rawText}
                      titleVisible
                      className="source-section"
                      style={{ background: sourceBackground, borderRadius: "8px 8px 0 0" }}
                    />
                  </ResizablePanel>
                  <ResizableHandle />
                  <ResizablePanel ref={translationPanelRef} defaultSize={52} minSize={24}>
                    <TranslationSection
                      title="译文"
                      text={translatedText}
                      titleVisible
                      className="translation-section-body"
                      style={{ background: translationBackground, borderRadius: "0 0 8px 8px" }}
                    />
                  </ResizablePanel>
                </ResizablePanelGroup>
              ) : (
                <TranslationSection
                  title="译文"
                  text={translatedText}
                  titleVisible={false}
                  className="translation-section-body solo"
                  style={{ background: translationBackground, borderRadius: 8 }}
                />
              )}
            </CardContent>
            <button
              aria-label="调整浮窗大小"
              className="window-resize-grip"
              data-no-window-drag
              onPointerDown={startResize}
            />
          </>
        )}
      </Card>
    </div>
  );
}

createRoot(document.getElementById("root")!).render(<OverlayApp />);
