export type Unlisten = () => void;

type TauriGlobal = {
  core: {
    invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  };
  event: {
    listen<T>(
      event: string,
      handler: (event: { payload: T }) => void,
    ): Promise<Unlisten>;
  };
};

declare global {
  interface Window {
    __TAURI__: TauriGlobal;
  }
}

export function invoke<T>(command: string, args?: Record<string, unknown>) {
  return window.__TAURI__.core.invoke<T>(command, args);
}

export function listen<T>(event: string, handler: (event: { payload: T }) => void) {
  return window.__TAURI__.event.listen<T>(event, handler);
}
