// The one bridge to the Rust side: `query(view, target?)`.

declare global {
  interface Window {
    __TAURI__: {
      core: { invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> };
    };
  }
}

export class QueryError extends Error {
  constructor(
    readonly kind: string,
    message: string,
  ) {
    super(message);
  }
}

export async function query<T>(view: string, target?: string): Promise<T> {
  try {
    return await window.__TAURI__.core.invoke<T>("query", { view, target: target ?? null });
  } catch (e) {
    const err = (e ?? {}) as { kind?: string; message?: string };
    throw new QueryError(err.kind ?? "error", err.message ?? String(e));
  }
}
