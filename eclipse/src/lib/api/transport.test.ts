// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ApiTransport } from './transport';

afterEach(() => vi.unstubAllGlobals());

describe('ApiTransport', () => {
  it('uses one authenticated, cookie-aware JSON request path', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      new Response(JSON.stringify({ ok: true }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' }
      })
    );
    vi.stubGlobal('fetch', fetchMock);
    const transport = new ApiTransport(() => 'session-token');

    await expect(
      transport.get<{ ok: boolean }>('thing', { page: 2 })
    ).resolves.toEqual({ ok: true });
    const [url, init] = fetchMock.mock.calls[0] as [URL, RequestInit];
    expect(url.pathname).toBe('/api/v1/thing');
    expect(url.searchParams.get('page')).toBe('2');
    expect(new Headers(init.headers).get('Authorization')).toBe(
      'session-token'
    );
    expect(init.credentials).toBe('same-origin');
  });

  it('normalizes the backend error envelope', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        new Response(
          JSON.stringify({
            error: { code: 'session_expired', message: 'Sign in to continue.' },
            request_id: 'request-1'
          }),
          { status: 401, headers: { 'Content-Type': 'application/json' } }
        )
      )
    );
    const transport = new ApiTransport(() => null);
    await expect(transport.get('auth/whoami')).rejects.toEqual(
      expect.objectContaining({
        status: 401,
        code: 'session_expired',
        requestId: 'request-1'
      })
    );
  });
});
