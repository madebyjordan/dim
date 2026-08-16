// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest';
import { realtime } from './socket.svelte';

class MockSocket extends EventTarget {
  static readonly OPEN = 1;
  readyState = MockSocket.OPEN;
  sent: string[] = [];
  constructor(readonly url: string) {
    super();
  }
  send(value: string) {
    this.sent.push(value);
  }
  close() {
    this.dispatchEvent(new Event('close'));
  }
}

afterEach(() => {
  realtime.stop();
  vi.unstubAllGlobals();
});

describe('RealtimeBoundary', () => {
  it('authenticates with a token and publishes a typed real event', () => {
    let socket: MockSocket | null = null;
    vi.stubGlobal(
      'WebSocket',
      class extends MockSocket {
        constructor(url: string) {
          super(url);
          socket = this;
        }
      }
    );
    const events: string[] = [];
    const unsubscribe = realtime.subscribe((event) => events.push(event.type));
    realtime.start(() => 'token', vi.fn());
    socket!.dispatchEvent(new Event('open'));
    expect(socket!.sent).toEqual([
      JSON.stringify({ type: 'authenticate', token: 'token' })
    ]);
    socket!.dispatchEvent(
      new MessageEvent('message', {
        data: JSON.stringify({ type: 'EventStartedScanning', id: 7 })
      })
    );
    expect(events).toEqual(['EventStartedScanning']);
    socket!.dispatchEvent(
      new MessageEvent('message', {
        data: JSON.stringify({ type: 'EventAuthOk', id: -1 })
      })
    );
    expect(realtime.state).toBe('connected');
    unsubscribe();
  });

  it('allows cookie-authenticated sockets without exposing a token', () => {
    let socket: MockSocket | null = null;
    vi.stubGlobal(
      'WebSocket',
      class extends MockSocket {
        constructor(url: string) {
          super(url);
          socket = this;
        }
      }
    );
    realtime.start(() => null, vi.fn());
    socket!.dispatchEvent(new Event('open'));
    expect(socket!.sent).toEqual([]);
    socket!.dispatchEvent(
      new MessageEvent('message', {
        data: JSON.stringify({ type: 'EventAuthOk', id: -1 })
      })
    );
    expect(realtime.state).toBe('connected');
  });
});
