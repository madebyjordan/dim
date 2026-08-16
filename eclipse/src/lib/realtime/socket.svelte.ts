import type { WebSocketAuthenticate, WebSocketEvent } from '$lib/api/generated';

export type ConnectionState =
  'idle' | 'connecting' | 'connected' | 'reconnecting' | 'offline';
type Listener = (event: WebSocketEvent) => void;

class RealtimeBoundary {
  state = $state<ConnectionState>('idle');
  lastEvent = $state<WebSocketEvent | null>(null);
  private socket: WebSocket | null = null;
  private retryTimer: number | undefined;
  private attempts = 0;
  private stopped = true;
  private listeners = new Set<Listener>();
  private token: () => string | null = () => null;
  private onExpired: () => void = () => undefined;

  start(token: () => string | null, onExpired: () => void) {
    this.stop();
    this.stopped = false;
    this.token = token;
    this.onExpired = onExpired;
    this.connect(false);
  }

  subscribe(listener: Listener) {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  retry() {
    this.close();
    this.attempts = 0;
    this.connect(false);
  }

  stop() {
    this.stopped = true;
    if (this.retryTimer !== undefined) window.clearTimeout(this.retryTimer);
    this.retryTimer = undefined;
    this.close();
    this.state = 'idle';
  }

  private connect(reconnecting: boolean) {
    if (this.stopped || this.socket) return;
    this.state = reconnecting ? 'reconnecting' : 'connecting';
    const url = new URL('/ws', window.location.href);
    url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:';
    const socket = new WebSocket(url);
    this.socket = socket;
    socket.addEventListener('open', () => {
      if (socket !== this.socket) return;
      this.attempts = 0;
      const token = this.token();
      if (token) {
        const message: WebSocketAuthenticate = { type: 'authenticate', token };
        socket.send(JSON.stringify(message));
      }
    });
    socket.addEventListener('message', ({ data }) => {
      try {
        const event = JSON.parse(String(data)) as WebSocketEvent;
        if (event.type === 'EventAuthErr') {
          this.onExpired();
          this.stop();
          return;
        }
        if (event.type === 'EventAuthOk') this.state = 'connected';
        this.lastEvent = event;
        for (const listener of this.listeners) listener(event);
      } catch {
        // The typed boundary deliberately ignores unknown server frames.
      }
    });
    socket.addEventListener('error', () => socket.close());
    socket.addEventListener('close', () => {
      if (socket !== this.socket) return;
      this.socket = null;
      if (!this.stopped) this.scheduleReconnect();
    });
  }

  private scheduleReconnect() {
    this.state = navigator.onLine ? 'reconnecting' : 'offline';
    const delay = Math.min(30_000, 1_000 * 2 ** this.attempts++);
    this.retryTimer = window.setTimeout(
      () => {
        this.retryTimer = undefined;
        this.connect(true);
      },
      delay * (0.75 + Math.random() * 0.5)
    );
  }

  private close() {
    this.socket?.close();
    this.socket = null;
  }
}

export const realtime = new RealtimeBoundary();
