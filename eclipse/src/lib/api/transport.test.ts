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

  it('adds lifecycle attribution to DELETE requests', async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response(null, { status: 204 }));
    vi.stubGlobal('fetch', fetchMock);
    const transport = new ApiTransport(() => null);

    await transport.delete('stream/session/state/kill', {
      teardown_reason: 'normal-player-exit',
      source_generation: 3
    });

    const [url, init] = fetchMock.mock.calls[0] as [URL, RequestInit];
    expect(init.method).toBe('DELETE');
    expect(url.searchParams.get('teardown_reason')).toBe('normal-player-exit');
    expect(url.searchParams.get('source_generation')).toBe('3');
  });
});
