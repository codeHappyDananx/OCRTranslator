import * as React from "react";
import { createRoot } from "react-dom/client";
import "./styles.css";
import { listen, type Unlisten } from "./tauri";

type StatusPayload = {
  text: string;
  done?: boolean;
};

function StatusApp() {
  const [text, setText] = React.useState("正在处理...");
  const [done, setDone] = React.useState(false);

  React.useEffect(() => {
    let disposed = false;
    let unlisten: Unlisten | null = null;
    listen<StatusPayload>("status-overlay-update", (event) => {
      if (disposed) return;
      setText(event.payload.text || "正在处理...");
      setDone(Boolean(event.payload.done));
    })
      .then((value) => {
        unlisten = value;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  return (
    <main className={`status-card${done ? " done" : ""}`}>
      <span className="status-spinner" aria-hidden="true" />
      <span className="status-text">{text}</span>
    </main>
  );
}

createRoot(document.getElementById("root")!).render(<StatusApp />);
