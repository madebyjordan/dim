import { apiRequest, SESSION_EXPIRED_EVENT } from "./transport";

afterEach(() => vi.restoreAllMocks());

test("adds auth, parses JSON, and returns the correlation ID on server errors", async () => {
  const fetchMock = vi.spyOn(window, "fetch").mockResolvedValue(
    new Response(
      JSON.stringify({
        error: { code: "library_not_found", message: "Missing." },
        request_id: "request-1",
      }),
      { status: 404, headers: { "content-type": "application/json" } }
    )
  );

  await expect(
    apiRequest("library/4", { token: "secret" })
  ).rejects.toMatchObject({
    status: 404,
    code: "library_not_found",
    requestId: "request-1",
  });
  expect(
    new Headers(fetchMock.mock.calls[0][1]?.headers).get("Authorization")
  ).toBe("secret");
});

test("signals expiry only for an authenticated 401", async () => {
  vi.spyOn(window, "fetch").mockResolvedValue(
    new Response(
      JSON.stringify({
        error: { code: "session_expired", message: "Sign in." },
        request_id: "request-2",
      }),
      { status: 401, headers: { "content-type": "application/json" } }
    )
  );
  const expired = vi.fn();
  window.addEventListener(SESSION_EXPIRED_EVENT, expired);
  await expect(
    apiRequest("auth/whoami", { token: "expired" })
  ).rejects.toBeTruthy();
  expect(expired).toHaveBeenCalledOnce();
  window.removeEventListener(SESSION_EXPIRED_EVENT, expired);
});

test("distinguishes offline failures", async () => {
  vi.spyOn(window, "fetch").mockRejectedValue(new TypeError("network"));
  vi.spyOn(window.navigator, "onLine", "get").mockReturnValue(false);
  await expect(apiRequest("library")).rejects.toMatchObject({
    kind: "offline",
    code: "offline",
  });
});
