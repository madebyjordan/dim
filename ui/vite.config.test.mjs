import config from "./vite.config.mjs";

it("rewrites the development WebSocket host and HTTP origin together", () => {
  expect(config.server.proxy["/ws"]).toMatchObject({
    target: "http://127.0.0.1:8000",
    changeOrigin: true,
    rewriteWsOrigin: true,
    ws: true,
  });
});
