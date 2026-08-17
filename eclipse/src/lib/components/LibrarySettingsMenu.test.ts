// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest';
import { mount, tick, unmount } from 'svelte';
import LibrarySettingsMenu from './LibrarySettingsMenu.svelte';

const components: Record<string, any>[] = [];
const library = {
  id: 7,
  name: 'Movies',
  locations: [],
  media_type: 'movie',
  auto_scan: true
};
type TestProps = {
  scanning: boolean;
  onautoscan: (enabled: boolean) => void | Promise<void>;
  onscan: () => void | Promise<void>;
  ondelete: () => void | Promise<void>;
};

afterEach(async () => {
  await Promise.all(
    components.splice(0).map((component) => unmount(component))
  );
  document.body.innerHTML = '';
});

function render(props: Partial<TestProps> = {}) {
  const callbacks = {
    onautoscan: vi.fn(),
    onscan: vi.fn(),
    ondelete: vi.fn()
  };
  components.push(
    mount(LibrarySettingsMenu, {
      target: document.body,
      props: {
        library,
        scanning: false,
        ...callbacks,
        ...props
      }
    })
  );
  return callbacks;
}

async function open() {
  const trigger = document.querySelector(
    'button[aria-label="Open settings for Movies"]'
  ) as HTMLButtonElement;
  trigger.click();
  await tick();
  await Promise.resolve();
  return {
    trigger,
    dialog: document.querySelector('[role="dialog"]') as HTMLElement
  };
}

describe('LibrarySettingsMenu', () => {
  it('opens a bespoke settings surface and focuses the switch', async () => {
    render();
    const { trigger, dialog } = await open();
    const control = dialog.querySelector('[role="switch"]');

    expect(trigger.getAttribute('aria-haspopup')).toBe('dialog');
    expect(dialog.getAttribute('aria-label')).toBe('Movies library settings');
    expect(dialog.textContent).toContain('Library settings');
    expect(dialog.textContent).toContain('Manual scan');
    expect(dialog.textContent).toContain('Delete library');
    expect(control?.getAttribute('aria-checked')).toBe('true');
    expect(document.activeElement).toBe(control);
  });

  it('wires auto scan and manual scan while restoring trigger focus', async () => {
    const callbacks = render();
    const { trigger, dialog } = await open();
    (dialog.querySelector('[role="switch"]') as HTMLButtonElement).click();
    await Promise.resolve();
    await tick();
    expect(callbacks.onautoscan).toHaveBeenCalledWith(false);

    const manual = Array.from(dialog.querySelectorAll('button')).find(
      (button) => button.textContent === 'Manual scan'
    ) as HTMLButtonElement;
    manual.click();
    await Promise.resolve();
    await tick();
    expect(callbacks.onscan).toHaveBeenCalledOnce();
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    expect(document.activeElement).toBe(trigger);
  });

  it('wires deletion and disables manual scan while scanning', async () => {
    const ondelete = vi.fn();
    render({ scanning: true, ondelete });
    let opened = await open();
    const manual = Array.from(opened.dialog.querySelectorAll('button')).find(
      (button) => button.textContent === 'Scanning…'
    ) as HTMLButtonElement;
    expect(manual.disabled).toBe(true);

    const remove = Array.from(opened.dialog.querySelectorAll('button')).find(
      (button) => button.textContent === 'Delete library'
    ) as HTMLButtonElement;
    remove.click();
    await Promise.resolve();
    await tick();
    expect(ondelete).toHaveBeenCalledOnce();
    expect(document.querySelector('[role="dialog"]')).toBeNull();
  });
});
