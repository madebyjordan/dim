// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { mount, tick, unmount } from 'svelte';
import { session } from '$lib/auth/session.svelte';
import LoginPage from './+page.svelte';

const { gotoMock } = vi.hoisted(() => ({ gotoMock: vi.fn() }));

vi.mock('$app/navigation', () => ({ goto: gotoMock }));

const components: Record<string, any>[] = [];

function response(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json' }
  });
}

function render() {
  components.push(mount(LoginPage, { target: document.body }));
}

async function settle() {
  for (let index = 0; index < 5; index += 1) {
    await Promise.resolve();
    await tick();
  }
}

function setInput(selector: string, value: string) {
  const input = document.querySelector(selector) as HTMLInputElement;
  input.value = value;
  input.dispatchEvent(new Event('input', { bubbles: true }));
}

function submit() {
  document
    .querySelector('form')
    ?.dispatchEvent(
      new SubmitEvent('submit', { bubbles: true, cancelable: true })
    );
}

beforeEach(() => {
  session.expire();
  gotoMock.mockReset();
  vi.unstubAllGlobals();
});

afterEach(async () => {
  await Promise.all(
    components.splice(0).map((component) => unmount(component))
  );
  document.body.innerHTML = '';
  session.expire();
  vi.unstubAllGlobals();
});

describe('first-run authentication', () => {
  it('does not flash either form while owner state is loading', async () => {
    let resolveOwner!: (value: Response) => void;
    vi.stubGlobal(
      'fetch',
      vi.fn(
        () =>
          new Promise<Response>((resolve) => {
            resolveOwner = resolve;
          })
      )
    );

    render();
    await tick();

    expect(document.body.textContent).toContain('Checking account setup…');
    expect(document.body.textContent).not.toContain('Welcome back');
    expect(document.body.textContent).not.toContain('Create your account');
    expect(document.querySelector('form')).toBeNull();

    resolveOwner(response({ exists: true }));
    await settle();
    expect(document.body.textContent).toContain('Welcome back');
  });

  it('shows account creation for a fresh database', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(response({ exists: false }))
    );

    render();
    await settle();

    expect(document.body.textContent).toContain('Welcome to Eclipse');
    expect(document.body.textContent).toContain('Create your account');
    expect(document.body.textContent).not.toContain('Welcome back');
    expect(
      document.querySelector('[autocomplete="new-password"]')
    ).not.toBeNull();
  });

  it('shows only normal login when an owner exists', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(response({ exists: true }))
    );

    render();
    await settle();

    expect(document.body.textContent).toContain('Welcome back');
    expect(document.body.textContent).toContain('Sign in');
    expect(document.body.textContent).not.toContain('Create your account');
    expect(document.body.textContent).not.toContain('Create account');
    expect(document.querySelector('[autocomplete="new-password"]')).toBeNull();
  });

  it('keeps both forms hidden when setup detection fails and can retry', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(
        response(
          {
            error: { code: 'internal_error', message: 'Setup check failed.' },
            request_id: 'request-1'
          },
          500
        )
      )
      .mockResolvedValueOnce(response({ exists: false }));
    vi.stubGlobal('fetch', fetchMock);

    render();
    await settle();

    expect(document.querySelector('[role="alert"]')?.textContent).toContain(
      'Setup check failed.'
    );
    expect(document.querySelector('form')).toBeNull();
    (document.querySelector('button') as HTMLButtonElement).click();
    await settle();
    expect(document.body.textContent).toContain('Create your account');
  });

  it('registers the first owner, establishes its session, and enters Eclipse', async () => {
    const fetchMock = vi.fn(
      async (input: URL | RequestInfo, _init?: RequestInit) => {
        const url = input instanceof URL ? input : new URL(String(input));
        if (url.pathname.endsWith('/auth/admin_exists'))
          return response({ exists: false });
        if (url.pathname.endsWith('/auth/register'))
          return response({ username: 'owner', token: 'owner-token' });
        if (url.pathname.endsWith('/auth/whoami'))
          return response({
            username: 'owner',
            roles: ['owner'],
            spentWatching: 0
          });
        throw new Error(`Unexpected request: ${url.pathname}`);
      }
    );
    vi.stubGlobal('fetch', fetchMock);

    render();
    await settle();
    setInput('[autocomplete="username"]', 'owner');
    const passwords = document.querySelectorAll(
      '[autocomplete="new-password"]'
    );
    setInput('[autocomplete="new-password"]', 'password123');
    const confirmation = passwords[1] as HTMLInputElement;
    confirmation.value = 'password123';
    confirmation.dispatchEvent(new Event('input', { bubbles: true }));
    submit();
    await settle();

    const registerCall = fetchMock.mock.calls.find(([input]) =>
      String(input).includes('/auth/register')
    );
    expect(registerCall).toBeDefined();
    expect(
      JSON.parse((registerCall?.[1] as RequestInit).body as string)
    ).toEqual({
      username: 'owner',
      password: 'password123'
    });
    expect(sessionStorage.getItem('eclipse.session-token')).toBe('owner-token');
    expect(session.status).toBe('authenticated');
    expect(session.user?.roles).toContain('owner');
    await vi.waitFor(() => expect(gotoMock).toHaveBeenCalledWith('/'));
  });

  it('rejects a password confirmation mismatch without registering', async () => {
    const fetchMock = vi.fn().mockResolvedValue(response({ exists: false }));
    vi.stubGlobal('fetch', fetchMock);

    render();
    await settle();
    setInput('[autocomplete="username"]', 'owner');
    const passwords = document.querySelectorAll(
      '[autocomplete="new-password"]'
    ) as NodeListOf<HTMLInputElement>;
    passwords[0].value = 'password123';
    passwords[0].dispatchEvent(new Event('input', { bubbles: true }));
    passwords[1].value = 'different123';
    passwords[1].dispatchEvent(new Event('input', { bubbles: true }));
    submit();
    await settle();

    expect(document.querySelector('[role="alert"]')?.textContent).toContain(
      'Passwords do not match'
    );
    expect(fetchMock).toHaveBeenCalledOnce();
    expect(gotoMock).not.toHaveBeenCalled();
  });

  it('surfaces backend registration failures and stays on account creation', async () => {
    const fetchMock = vi.fn(
      async (input: URL | RequestInfo, _init?: RequestInit) => {
        const url = input instanceof URL ? input : new URL(String(input));
        if (url.pathname.endsWith('/auth/admin_exists'))
          return response({ exists: false });
        return response(
          {
            error: {
              code: 'invite_required',
              message: 'A valid invite token is required.'
            },
            request_id: 'request-1'
          },
          401
        );
      }
    );
    vi.stubGlobal('fetch', fetchMock);

    render();
    await settle();
    setInput('[autocomplete="username"]', 'owner');
    const passwords = document.querySelectorAll(
      '[autocomplete="new-password"]'
    ) as NodeListOf<HTMLInputElement>;
    for (const input of passwords) {
      input.value = 'password123';
      input.dispatchEvent(new Event('input', { bubbles: true }));
    }
    submit();
    await settle();

    expect(document.querySelector('[role="alert"]')?.textContent).toContain(
      'A valid invite token is required.'
    );
    expect(document.body.textContent).toContain('Create your account');
    expect(gotoMock).not.toHaveBeenCalled();
  });

  it('prevents duplicate first-owner submissions', async () => {
    let resolveRegistration!: (value: Response) => void;
    const fetchMock = vi.fn(
      async (input: URL | RequestInfo, _init?: RequestInit) => {
        const url = input instanceof URL ? input : new URL(String(input));
        if (url.pathname.endsWith('/auth/admin_exists'))
          return response({ exists: false });
        if (url.pathname.endsWith('/auth/register'))
          return new Promise<Response>((resolve) => {
            resolveRegistration = resolve;
          });
        if (url.pathname.endsWith('/auth/whoami'))
          return response({
            username: 'owner',
            roles: ['owner'],
            spentWatching: 0
          });
        throw new Error(`Unexpected request: ${url.pathname}`);
      }
    );
    vi.stubGlobal('fetch', fetchMock);

    render();
    await settle();
    setInput('[autocomplete="username"]', 'owner');
    const passwords = document.querySelectorAll(
      '[autocomplete="new-password"]'
    ) as NodeListOf<HTMLInputElement>;
    for (const input of passwords) {
      input.value = 'password123';
      input.dispatchEvent(new Event('input', { bubbles: true }));
    }
    submit();
    submit();
    await tick();

    expect(
      fetchMock.mock.calls.filter(([input]) =>
        String(input).includes('/auth/register')
      )
    ).toHaveLength(1);
    expect(
      (document.querySelector('button') as HTMLButtonElement).disabled
    ).toBe(true);

    resolveRegistration(response({ username: 'owner', token: 'owner-token' }));
    await vi.waitFor(() => expect(gotoMock).toHaveBeenCalledWith('/'));
  });

  it('preserves the existing login request and session flow', async () => {
    const fetchMock = vi.fn(
      async (input: URL | RequestInfo, _init?: RequestInit) => {
        const url = input instanceof URL ? input : new URL(String(input));
        if (url.pathname.endsWith('/auth/admin_exists'))
          return response({ exists: true });
        if (url.pathname.endsWith('/auth/login'))
          return response({ token: 'login-token' });
        if (url.pathname.endsWith('/auth/whoami'))
          return response({
            username: 'existing',
            roles: ['owner'],
            spentWatching: 0
          });
        throw new Error(`Unexpected request: ${url.pathname}`);
      }
    );
    vi.stubGlobal('fetch', fetchMock);

    render();
    await settle();
    setInput('[autocomplete="username"]', 'existing');
    setInput('[autocomplete="current-password"]', 'password123');
    submit();
    await settle();

    const loginCall = fetchMock.mock.calls.find(([input]) =>
      String(input).includes('/auth/login')
    );
    expect(JSON.parse((loginCall?.[1] as RequestInit).body as string)).toEqual({
      username: 'existing',
      password: 'password123'
    });
    expect(session.status).toBe('authenticated');
    await vi.waitFor(() => expect(gotoMock).toHaveBeenCalledWith('/'));
  });
});
