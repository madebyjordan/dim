import type { WebSocketAuthenticate, WebSocketEvent } from "./generated";

export type ConnectionState =
  | "connecting"
  | "connected"
  | "reconnecting"
  | "offline"
  | "expired"
  | "stopped";

interface Options {
  createSocket?: (url: string) => WebSocket;
  probe?: () => Promise<boolean>;
  random?: () => number;
  baseDelayMs?: number;
  maxDelayMs?: number;
  idleMs?: number;
  onState?: (state: ConnectionState, retryInMs?: number) => void;
  onSocket?: (socket: WebSocket | null) => void;
  onEvent?: (event: WebSocketEvent) => void;
}

const socketUrl = () => {
  const url = new URL("/ws", window.location.href);
  url.protocol = url.protocol.replace("http", "ws");
  return url.href;
};

const defaultProbe = async () => {
  try {
    const response = await fetch("/health/live", {
      signal: AbortSignal.timeout(5_000),
    });
    return response.ok;
  } catch {
    return false;
  }
};

export class DimWebSocket {
  private socket: WebSocket | null = null;
  private reconnectTimer: number | undefined;
  private openTimer: number | undefined;
  private idleTimer: number | undefined;
  private attempt = 0;
  private stopped = true;
  private token: string | null = null;
  private lastActivity = Date.now();
  private readonly options: Required<Omit<Options, "onEvent">> &
    Pick<Options, "onEvent">;

  constructor(options: Options = {}) {
    this.options = {
      createSocket: options.createSocket ?? ((url) => new WebSocket(url)),
      probe: options.probe ?? defaultProbe,
      random: options.random ?? Math.random,
      baseDelayMs: options.baseDelayMs ?? 1_000,
      maxDelayMs: options.maxDelayMs ?? 30_000,
      idleMs: options.idleMs ?? 45_000,
      onState: options.onState ?? (() => undefined),
      onSocket: options.onSocket ?? (() => undefined),
      onEvent: options.onEvent,
    };
  }

  start(token: string | null) {
    this.token = token;
    if (!this.stopped) return;
    this.stopped = false;
    this.connect(false);
  }

  setToken(token: string | null) {
    this.token = token;
    if (token && this.socket?.readyState === WebSocket.OPEN)
      this.authenticate();
  }

  retryNow() {
    if (this.stopped) return;
    this.clearReconnect();
    this.clearOpenTimer();
    this.closeSocket();
    this.attempt = 0;
    this.connect(false);
  }

  stop() {
    this.stopped = true;
    this.clearReconnect();
    if (this.idleTimer !== undefined) window.clearInterval(this.idleTimer);
    this.idleTimer = undefined;
    this.closeSocket();
    this.options.onState("stopped");
  }

  private connect(reconnecting: boolean) {
    if (this.stopped || this.socket) return;
    this.options.onState(reconnecting ? "reconnecting" : "connecting");
    const socket = this.options.createSocket(socketUrl());
    this.socket = socket;
    this.options.onSocket(socket);
    this.openTimer = window.setTimeout(() => {
      if (socket === this.socket && socket.readyState !== WebSocket.OPEN) {
        socket.close();
      }
    }, 10_000);
    socket.addEventListener("open", () => {
      if (socket !== this.socket) return;
      this.clearOpenTimer();
      this.attempt = 0;
      this.lastActivity = Date.now();
      this.options.onState("connected");
      if (this.token) this.authenticate();
      this.startIdleMonitor();
    });
    socket.addEventListener("message", ({ data }) => {
      this.lastActivity = Date.now();
      try {
        const event = JSON.parse(String(data)) as WebSocketEvent;
        if (event.type === "EventAuthErr") {
          this.options.onState("expired");
          window.dispatchEvent(new CustomEvent("dim:session-expired"));
          this.stop();
          return;
        }
        this.options.onEvent?.(event);
      } catch {
        // Unknown server messages are ignored without destabilising the connection.
      }
    });
    socket.addEventListener("close", () => {
      if (socket !== this.socket) return;
      this.clearOpenTimer();
      this.socket = null;
      this.options.onSocket(null);
      if (!this.stopped) this.scheduleReconnect();
    });
    socket.addEventListener("error", () => socket.close());
  }

  private authenticate() {
    const message: WebSocketAuthenticate = {
      type: "authenticate",
      token: this.token as string,
    };
    this.socket?.send(JSON.stringify(message));
  }

  private scheduleReconnect() {
    if (this.reconnectTimer !== undefined || this.stopped) return;
    const exponential = Math.min(
      this.options.maxDelayMs,
      this.options.baseDelayMs * 2 ** this.attempt++
    );
    const delay = Math.round(
      exponential * (0.75 + this.options.random() * 0.5)
    );
    this.options.onState("reconnecting", delay);
    this.reconnectTimer = window.setTimeout(() => {
      this.reconnectTimer = undefined;
      this.connect(true);
    }, delay);
  }

  private startIdleMonitor() {
    if (this.idleTimer !== undefined) window.clearInterval(this.idleTimer);
    this.idleTimer = window.setInterval(async () => {
      if (Date.now() - this.lastActivity < this.options.idleMs) return;
      if (await this.options.probe()) {
        this.lastActivity = Date.now();
      } else {
        this.options.onState("offline");
        this.socket?.close();
      }
    }, Math.max(1_000, Math.floor(this.options.idleMs / 3)));
  }

  private clearReconnect() {
    if (this.reconnectTimer !== undefined)
      window.clearTimeout(this.reconnectTimer);
    this.reconnectTimer = undefined;
  }

  private clearOpenTimer() {
    if (this.openTimer !== undefined) window.clearTimeout(this.openTimer);
    this.openTimer = undefined;
  }

  private closeSocket() {
    const socket = this.socket;
    this.clearOpenTimer();
    this.socket = null;
    this.options.onSocket(null);
    socket?.close(1000, "Dim client teardown");
  }
}
