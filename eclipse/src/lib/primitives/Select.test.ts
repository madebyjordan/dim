// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest';
import { mount, tick, unmount } from 'svelte';
import DropdownMenuFixture from './DropdownMenu.test.svelte';
import Select from './Select.svelte';
import SelectFixture from './Select.test.svelte';

const options = [
  { value: 'one', label: 'One' },
  { value: 'disabled', label: 'Disabled', disabled: true },
  { value: 'two', label: 'Two' }
];
const components: Record<string, any>[] = [];

afterEach(async () => {
  await Promise.all(
    components.splice(0).map((component) => unmount(component))
  );
  document.body.innerHTML = '';
});

function renderSelect({
  value = 'one',
  disabled = false,
  onvaluechange = () => undefined
}: {
  value?: string;
  disabled?: boolean;
  onvaluechange?: (value: string) => void;
} = {}) {
  components.push(
    mount(Select, {
      target: document.body,
      props: {
        label: 'Track',
        value,
        options,
        disabled,
        onvaluechange
      }
    })
  );
  return document.querySelector(
    'button[aria-label="Track"]'
  ) as HTMLButtonElement;
}

function press(target: Element, key: string) {
  target.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true }));
}

async function open(trigger: HTMLButtonElement, role: 'listbox' | 'menu') {
  trigger.click();
  await tick();
  await Promise.resolve();
  return document.querySelector(`[role="${role}"]`) as HTMLElement;
}

function activeOption(listbox: HTMLElement) {
  const id = listbox.getAttribute('aria-activedescendant');
  return id ? document.getElementById(id) : null;
}

describe('Select', () => {
  it('opens a bespoke listbox and maps pointer selection to value changes', async () => {
    const onvaluechange = vi.fn();
    const trigger = renderSelect({ onvaluechange });

    expect(document.querySelector('select')).toBeNull();
    expect(trigger.textContent).toContain('One');

    const listbox = await open(trigger, 'listbox');
    const renderedOptions = Array.from(
      listbox.querySelectorAll<HTMLButtonElement>('[role="option"]')
    );

    expect(trigger.getAttribute('aria-expanded')).toBe('true');
    expect(renderedOptions).toHaveLength(3);
    expect(renderedOptions[0].getAttribute('aria-selected')).toBe('true');
    expect(renderedOptions[1].disabled).toBe(true);
    expect(document.activeElement).toBe(listbox);

    renderedOptions[2].dispatchEvent(
      new Event('pointerdown', { bubbles: true })
    );
    renderedOptions[2].click();
    await tick();

    expect(onvaluechange).toHaveBeenCalledWith('two');
    expect(document.querySelector('[role="listbox"]')).toBeNull();
    expect(document.activeElement).toBe(trigger);
  });

  it('navigates enabled options and activates with Enter or Space', async () => {
    const onvaluechange = vi.fn();
    const trigger = renderSelect({ onvaluechange });
    let listbox = await open(trigger, 'listbox');

    expect(activeOption(listbox)?.textContent).toContain('One');
    press(listbox, 'ArrowDown');
    await tick();
    expect(activeOption(listbox)?.textContent).toContain('Two');
    press(listbox, 'Enter');
    await tick();
    expect(onvaluechange).toHaveBeenLastCalledWith('two');

    press(trigger, 'ArrowDown');
    await tick();
    await Promise.resolve();
    listbox = document.querySelector('[role="listbox"]') as HTMLElement;
    press(listbox, ' ');
    await tick();
    expect(onvaluechange).toHaveBeenLastCalledWith('one');
  });

  it('supports Home and End while skipping disabled options', async () => {
    const trigger = renderSelect();
    const listbox = await open(trigger, 'listbox');

    press(listbox, 'End');
    await tick();
    expect(activeOption(listbox)?.textContent).toContain('Two');
    press(listbox, 'Home');
    await tick();
    expect(activeOption(listbox)?.textContent).toContain('One');
  });

  it('dismisses with Escape and restores trigger focus', async () => {
    const trigger = renderSelect();
    const listbox = await open(trigger, 'listbox');

    press(listbox, 'Escape');
    await tick();

    expect(document.querySelector('[role="listbox"]')).toBeNull();
    expect(document.activeElement).toBe(trigger);
  });

  it('dismisses on an outside pointer without stealing focus', async () => {
    const trigger = renderSelect();
    const outside = document.createElement('button');
    outside.textContent = 'Outside';
    document.body.append(outside);
    await open(trigger, 'listbox');

    outside.dispatchEvent(new Event('pointerdown', { bubbles: true }));
    outside.focus();
    await tick();

    expect(document.querySelector('[role="listbox"]')).toBeNull();
    expect(document.activeElement).toBe(outside);
  });

  it('disables the trigger without opening a listbox', async () => {
    const trigger = renderSelect({ disabled: true });

    expect(trigger.disabled).toBe(true);
    await open(trigger, 'listbox');
    expect(document.querySelector('[role="listbox"]')).toBeNull();
  });

  it('does not present the first option as an invalid controlled value', async () => {
    const trigger = renderSelect({ value: 'missing' });

    expect(trigger.textContent).toContain('Track');
    const listbox = await open(trigger, 'listbox');
    expect(listbox.querySelector('[aria-selected="true"]')).toBeNull();
  });

  it('gives multiple instances collision-safe trigger and popout IDs', () => {
    components.push(mount(SelectFixture, { target: document.body }));
    components.push(
      mount(DropdownMenuFixture, {
        target: document.body,
        props: { onaction: () => undefined }
      })
    );
    const triggers = Array.from(
      document.querySelectorAll<HTMLButtonElement>('[aria-haspopup]')
    );

    expect(triggers).toHaveLength(3);
    expect(new Set(triggers.map((trigger) => trigger.id)).size).toBe(3);
    expect(
      new Set(triggers.map((trigger) => trigger.getAttribute('aria-controls')))
        .size
    ).toBe(3);
  });
});

describe('DropdownMenu', () => {
  it('uses the same popout lifecycle with menu action semantics', async () => {
    const onaction = vi.fn();
    components.push(
      mount(DropdownMenuFixture, {
        target: document.body,
        props: { onaction }
      })
    );
    const trigger = document.querySelector(
      'button[aria-label="Open actions"]'
    ) as HTMLButtonElement;

    expect(trigger.getAttribute('aria-haspopup')).toBe('menu');
    const menu = await open(trigger, 'menu');
    const unavailable = Array.from(menu.querySelectorAll('button')).find(
      (button) => button.textContent === 'Unavailable action'
    ) as HTMLButtonElement;

    expect(menu.querySelector('[role="separator"]')).not.toBeNull();
    expect(unavailable.disabled).toBe(true);
    unavailable.click();
    expect(onaction).not.toHaveBeenCalled();
    expect(document.activeElement?.textContent).toBe('Available action');

    press(document.activeElement as Element, 'End');
    expect(document.activeElement?.textContent).toBe('Second action');
    press(document.activeElement as Element, 'Home');
    expect(document.activeElement?.textContent).toBe('Available action');
    press(document.activeElement as Element, ' ');
    await tick();

    expect(onaction).toHaveBeenCalledWith('available');
    expect(document.activeElement).toBe(trigger);
  });

  it('supports Arrow navigation, Escape, and a disabled icon trigger', async () => {
    const onaction = vi.fn();
    components.push(
      mount(DropdownMenuFixture, {
        target: document.body,
        props: { onaction }
      })
    );
    const trigger = document.querySelector(
      'button[aria-label="Open actions"]'
    ) as HTMLButtonElement;
    await open(trigger, 'menu');

    press(document.activeElement as Element, 'ArrowUp');
    expect(document.activeElement?.textContent).toBe('Second action');
    press(document.activeElement as Element, 'Escape');
    await tick();
    expect(document.querySelector('[role="menu"]')).toBeNull();
    expect(document.activeElement).toBe(trigger);

    await unmount(components.pop()!);
    components.push(
      mount(DropdownMenuFixture, {
        target: document.body,
        props: { disabled: true, onaction }
      })
    );
    const disabledTrigger = document.querySelector(
      'button[aria-label="Open actions"]'
    ) as HTMLButtonElement;
    expect(disabledTrigger.disabled).toBe(true);
    await open(disabledTrigger, 'menu');
    expect(document.querySelector('[role="menu"]')).toBeNull();
  });
});
