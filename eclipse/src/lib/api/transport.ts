import type { ApiErrorEnvelope } from './generated';

export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
    readonly requestId?: string
  ) {
    super(message);
  }
}

type QueryValue = string | number | boolean | null | undefined;

export class ApiTransport {
  constructor(private readonly token: () => string | null) {}

  async request<T>(
    path: string,
    init: RequestInit & { query?: Record<string, QueryValue> } = {}
  ): Promise<T> {
    const url = new URL(
      `/api/v1/${path.replace(/^\//, '')}`,
      window.location.origin
    );
    for (const [key, value] of Object.entries(init.query ?? {})) {
      if (value !== undefined && value !== null)
        url.searchParams.set(key, String(value));
    }
    const headers = new Headers(init.headers);
    headers.set('Accept', 'application/json');
    const token = this.token();
    if (token) headers.set('Authorization', token);
    if (init.body && typeof init.body === 'string')
      headers.set('Content-Type', 'application/json');
    const response = await fetch(url, {
      ...init,
      headers,
      credentials: 'same-origin'
    });
    if (!response.ok) {
      const envelope = (await response
        .json()
        .catch(() => null)) as ApiErrorEnvelope | null;
      throw new ApiError(
        response.status,
        envelope?.error.code ?? 'request_failed',
        envelope?.error.message ?? `Request failed (${response.status})`,
        envelope?.request_id
      );
    }
    if (response.status === 204) return undefined as T;
    const type = response.headers.get('content-type') ?? '';
    return (
      type.includes('json') ? response.json() : response.text()
    ) as Promise<T>;
  }

  get<T>(path: string, query?: Record<string, QueryValue>) {
    return this.request<T>(path, { query });
  }

  post<T>(path: string, body?: unknown) {
    return this.request<T>(path, {
      method: 'POST',
      body: body === undefined ? undefined : JSON.stringify(body)
    });
  }

  patch<T>(path: string, body: unknown) {
    return this.request<T>(path, {
      method: 'PATCH',
      body: JSON.stringify(body)
    });
  }

  delete(path: string) {
    return this.request<void>(path, { method: 'DELETE' });
  }
}
