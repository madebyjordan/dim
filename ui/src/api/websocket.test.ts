import { DimWebSocket } from "./websocket";

class FakeSocket extends EventTarget {
  static OPEN = 1;
  readyState = 0;
  sent: string[] = [];
  close = vi.fn(() => {
    this.readyState = 3;
    this.dispatchEvent(new CloseEvent("close"));
  });
  send(value: string) {
    this.sent.push(value);
  }
  open() {
    this.readyState = FakeSocket.OPEN;
    this.dispatchEvent(new Event("open"));
  }
  message(value: unknown) {
    this.dispatchEvent(
      new MessageEvent("message", { data: JSON.stringify(value) })
    );
  }
}

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

test("owns one socket, authenticates after open, and tears down cleanly", () => {
  const sockets: FakeSocket[] = [];
  const onSocket = vi.fn();
  const manager = new DimWebSocket({
    createSocket: () => {
      const socket = new FakeSocket();
      sockets.push(socket);
      return socket as unknown as WebSocket;
    },
    onSocket,
  });
  manager.start("token");
  manager.start("token");
  expect(sockets).toHaveLength(1);
  sockets[0].open();
  expect(JSON.parse(sockets[0].sent[0])).toEqual({
    type: "authenticate",
    token: "token",
  });
  manager.stop();
  expect(sockets[0].close).toHaveBeenCalledOnce();
  vi.runOnlyPendingTimers();
  expect(sockets).toHaveLength(1);
});

test("uses bounded reconnect and expires on an auth error", () => {
  const sockets: FakeSocket[] = [];
  const states = vi.fn();
  const manager = new DimWebSocket({
    createSocket: () => {
      const socket = new FakeSocket();
      sockets.push(socket);
      return socket as unknown as WebSocket;
    },
    baseDelayMs: 100,
    maxDelayMs: 100,
    random: () => 0.5,
    onState: states,
  });
  manager.start("token");
  sockets[0].open();
  sockets[0].close();
  expect(states).toHaveBeenCalledWith("reconnecting", 100);
  vi.advanceTimersByTime(100);
  expect(sockets).toHaveLength(2);
  sockets[1].open();
  sockets[1].message({ type: "EventAuthErr", id: -1 });
  expect(states).toHaveBeenCalledWith("expired");
  vi.runOnlyPendingTimers();
  expect(sockets).toHaveLength(2);
});
