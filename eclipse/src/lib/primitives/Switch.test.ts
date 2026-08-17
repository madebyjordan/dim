// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest';
import { mount, tick, unmount } from 'svelte';
import Switch from './Switch.svelte';
import SwitchFixture from './Switch.test.svelte';

const components: Record<string, any>[] = [];

afterEach(async () => {
  await Promise.all(
    components.splice(0).map((component) => unmount(component))
  );
  document.body.innerHTML = '';
});

describe('Switch', () => {
  it('uses a controlled checked callback and switch semantics', async () => {
    components.push(mount(SwitchFixture, { target: document.body }));
    const control = document.querySelector(
      '[role="switch"]'
    ) as HTMLButtonElement;

    expect(control.tagName).toBe('BUTTON');
    expect(control.type).toBe('button');
    expect(control.getAttribute('aria-label')).toBe('Setting');
    expect(control.getAttribute('aria-checked')).toBe('false');
    expect(control.classList.contains('surface')).toBe(true);

    control.click();
    await tick();
    expect(control.getAttribute('aria-checked')).toBe('true');

    control.click();
    await tick();
    expect(control.getAttribute('aria-checked')).toBe('false');
  });

  it('does not activate while disabled', () => {
    const oncheckedchange = vi.fn();
    components.push(
      mount(Switch, {
        target: document.body,
        props: {
          label: 'Disabled setting',
          checked: true,
          disabled: true,
          oncheckedchange
        }
      })
    );
    const control = document.querySelector(
      '[role="switch"]'
    ) as HTMLButtonElement;

    expect(control.disabled).toBe(true);
    expect(control.getAttribute('aria-checked')).toBe('true');
    control.click();
    expect(oncheckedchange).not.toHaveBeenCalled();
  });

  it('supports the quiet tone explicitly', () => {
    components.push(
      mount(Switch, {
        target: document.body,
        props: {
          label: 'Quiet setting',
          checked: false,
          tone: 'quiet',
          oncheckedchange: () => undefined
        }
      })
    );

    expect(
      document.querySelector('[role="switch"]')?.classList.contains('surface')
    ).toBe(false);
  });
});
