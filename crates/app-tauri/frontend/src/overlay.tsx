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
  text: string;
  raw_text: string;
  width: number;
  opacity: number;
  font_size: number;
  max_height: number;
  source_background: string;
  translation_background: string;
  double_click_close: boolean;
  show_source: boolean;
  draggable: boolean;
};

const emptyPayload: OverlayPayload = {
  text: "",
  raw_text: "",
  width: 320,
  opacity: 0.55,
  font_size: 18,
  max_height: 620,
  source_background: "#2858a5",
  translation_background: "#127858",
  double_click_close: true,
  show_source: true,
  draggable: true,
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

function OverlayApp() {
  const [payload, setPayload] = React.useState<OverlayPayload>(emptyPayload);
  const [userSized, setUserSized] = React.useState(false);
  const cardRef = React.useRef<HTMLDivElement | null>(null);
  const sourcePanelRef = React.useRef<ResizablePanelHandle | null>(null);
  const translationPanelRef = React.useRef<ResizablePanelHandle | null>(null);
  const lastResize = React.useRef({ width: 0, height: 0 });
  const ignoreResizeUntil = React.useRef(0);

  const rawText = cleanDisplayText(payload.raw_text);
  const translatedText = cleanDisplayText(payload.text) || "无翻译结果";
  const showSource = payload.show_source !== false && rawText.length > 0;
  const opacity = payload.opacity ?? 0.55;
  const maxHeight = Math.max(120, Number(payload.max_height ?? 620));
  const sourceBackground = hexToRgba(payload.source_background, opacity, "#2858a5");
  const translationBackground = hexToRgba(
    payload.translation_background,
    opacity,
    "#127858",
  );

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
      invoke("resize_overlay_to_content", { request: { width, height } }).catch(() => {});
    };
    frame = window.requestAnimationFrame(resize);
    return () => window.cancelAnimationFrame(frame);
  }, [payload, showSource, rawText, translatedText, maxHeight, userSized]);

  React.useEffect(() => {
    function onResize() {
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

  return (
    <div className="overlay-stage" onDoubleClick={closeOverlay}>
      <Card
        ref={cardRef}
        className={`translation-card${payload.draggable !== false ? " draggable" : ""}`}
        style={{ fontSize, maxHeight }}
        onMouseDown={startDrag}
      >
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
      </Card>
    </div>
  );
}

createRoot(document.getElementById("root")!).render(<OverlayApp />);
