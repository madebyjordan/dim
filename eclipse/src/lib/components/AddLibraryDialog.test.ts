// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { mount, tick, unmount } from 'svelte';
import { session } from '$lib/auth/session.svelte';
import AddLibraryDialog from './AddLibraryDialog.svelte';

const components: Record<string, any>[] = [];

beforeEach(() => {
  Object.defineProperty(HTMLDialogElement.prototype, 'showModal', {
    configurable: true,
    value() {
      this.open = true;
    }
  });
  Object.defineProperty(HTMLDialogElement.prototype, 'close', {
    configurable: true,
    value() {
      this.open = false;
    }
  });
});

afterEach(async () => {
  vi.restoreAllMocks();
  await Promise.all(
    components.splice(0).map((component) => unmount(component))
  );
  document.body.innerHTML = '';
});

async function settle() {
  await Promise.resolve();
  await tick();
  await Promise.resolve();
  await tick();
}

describe('AddLibraryDialog storage roots', () => {
  it('enters a selected native root and preserves folder and back navigation', async () => {
    const get = vi.spyOn(session.api, 'get').mockImplementation(
      (async (path: string, query?: Record<string, unknown>) => {
        if (path === 'filebrowser/roots') {
          return [
            {
              display_name: 'Main Drive',
              path: 'D:\\',
              available_bytes: 536_870_912_000,
              kind: 'fixed'
            }
          ];
        }
        if (query?.path === 'D:\\Media') {
          return {
            current: 'D:\\Media',
            parent: 'D:\\',
            directories: []
          };
        }
        return {
          current: 'D:\\',
          parent: null,
          directories: [{ name: 'Media', path: 'D:\\Media' }]
        };
      }) as typeof session.api.get
    );

    components.push(
      mount(AddLibraryDialog, {
        target: document.body,
        props: {
          open: true,
          onclose: vi.fn(),
          oncreated: vi.fn()
        }
      })
    );
    await settle();

    expect(document.body.textContent).toContain('Set Directory /');
    expect(document.body.textContent).toContain('Main Drive');
    expect(document.body.textContent).toContain('500 GB available');

    (Array.from(document.querySelectorAll('button')).find((button) =>
      button.textContent?.includes('Main Drive')
    ) as HTMLButtonElement).click();
    await settle();

    expect(get).toHaveBeenCalledWith('filebrowser', { path: 'D:\\' });
    expect(document.body.textContent).toContain('Set Directory /Main Drive/');

    (Array.from(document.querySelectorAll('button')).find(
      (button) => button.textContent === 'Media'
    ) as HTMLButtonElement).click();
    await settle();
    expect(document.body.textContent).toContain(
      'Set Directory /Main Drive/Media/'
    );

    (Array.from(document.querySelectorAll('button')).find(
      (button) => button.textContent === '../'
    ) as HTMLButtonElement).click();
    await settle();
    expect(document.body.textContent).toContain('Set Directory /Main Drive/');
  });
});
