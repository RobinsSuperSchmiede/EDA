/** Type definitions for @openshell/sdk. See docs/sdk.md. */

export type Permission = "read" | "ui" | "actions" | "services" | "files" | "code" | "system";

export type ServiceState =
  | "stopped"
  | "pending"
  | "setup"
  | "starting"
  | "running"
  | "unhealthy"
  | "stopping"
  | "backoff"
  | "exited"
  | "crashed"
  | "failed";

export interface ServiceStatus {
  name: string;
  kind: "process" | "external" | "static";
  category?: string;
  description?: string;
  state: ServiceState;
  pid?: number;
  port?: number;
  url?: string;
  command?: string;
  restarts: number;
  exit_code?: number;
  last_error?: string;
  started_at?: string;
  depends_on: string[];
  autostart: boolean;
}

export interface LogLine {
  seq: number;
  ts: string;
  service: string;
  stream: "stdout" | "stderr" | "system";
  text: string;
}

export interface ActionMeta {
  description?: string;
  /** JSON Schema of the input object. */
  input_schema?: object;
  /** Scope an agent needs to invoke the action (default "actions"). */
  permission?: Permission;
}

export declare class OpenShellError extends Error {}

export interface OpenShellApi {
  readonly available: boolean;
  readonly mode: "development" | "production" | null;
  readonly host: "desktop" | "browser" | null;
  info(): Promise<{ app: string; version?: string; mode: string; host: string; permissions: Permission[]; services: string[] }>;
  runtime: {
    status(): Promise<{ services: ServiceStatus[] }>;
    logs(opts?: { service?: string; since?: number; limit?: number; contains?: string; errors_only?: boolean }): Promise<{
      lines: LogLine[];
      last_seq: number;
    }>;
    start(name: string): Promise<unknown>;
    stop(name: string): Promise<unknown>;
    restart(name: string): Promise<unknown>;
  };
  window: {
    show(): Promise<unknown>;
    hide(): Promise<unknown>;
    focus(): Promise<unknown>;
    minimize(): Promise<unknown>;
    maximize(): Promise<unknown>;
    unmaximize(): Promise<unknown>;
  };
  notify(title: string, body?: string): Promise<unknown>;
  providers(): Promise<{ providers: Array<{ name: string; kind: string; url?: string; available: boolean; models: string[] }> }>;
  db: {
    list(): Promise<{ databases: Array<{ name: string; path: string; agent_access: string; tables: Array<{ name: string; rows: number }> }> }>;
    schema(table?: string, database?: string): Promise<{ database: string; tables: Array<{ table: string; columns: object[]; sql: string | null }> }>;
    query<Row = Record<string, unknown>>(
      sql: string,
      params?: unknown[] | Record<string, unknown>,
      database?: string,
    ): Promise<{ columns: string[]; rows: Row[]; row_count: number; truncated: boolean }>;
    execute(
      sql: string,
      params?: unknown[] | Record<string, unknown>,
      database?: string,
    ): Promise<{ ok: true; changes: number; last_insert_rowid?: number }>;
  };
  invoke<T = unknown>(name: string, input?: object): Promise<T>;
  agent: {
    registerAction<I = any, O = any>(name: string, meta: ActionMeta, handler: (input: I) => O | Promise<O>): () => void;
    registerAction<I = any, O = any>(name: string, handler: (input: I) => O | Promise<O>): () => void;
    unregisterAction(name: string): void;
  };
  call<T = unknown>(tool: string, args?: object): Promise<T>;
}

export declare const OpenShell: OpenShellApi;
export default OpenShell;

declare global {
  interface Window {
    __OPENSHELL__?: {
      api: string;
      bridge: string;
      token: string;
      host: "desktop" | "browser";
      mode: string;
      app: string;
      /** Desktop window: who draws the title bar, and (with "overlay") the window functions. */
      window?: {
        title_bar: "native" | "overlay";
        os: "windows" | "macos" | "linux" | string;
        minimize?(): Promise<void>;
        toggleMaximize?(): Promise<void>;
        close?(): Promise<void>;
        isMaximized?(): Promise<boolean>;
        showMenu?(): Promise<void>;
      };
    };
    OpenShell?: OpenShellApi;
  }
}
