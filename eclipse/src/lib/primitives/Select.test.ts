// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest';
import { mount } from 'svelte';
import Select from './Select.svelte';
import SelectFixture from './Select.test.svelte';

const options = [
  { value: 'one', label: 'One' },
  { value: 'two', label: 'Two' }
];

afterEach(() => {
  document.body.innerHTML = '';
});

function render(
  value = 'one',
  onvaluechange: (value: string) => void = () => undefined
) {
  mount(Select, {
    target: document.body,
    props: { label: 'Track', value, options, onvaluechange }
  });
  return document.querySelector('select') as HTMLSelectElement;
}

describe('Select', () => {
  it('associates a unique label with every instance', () => {
    mount(SelectFixture, { target: document.body });
    const [first, second] = Array.from(document.querySelectorAll('select'));
    const labels = Array.from(document.querySelectorAll('label'));

    expect(first.id).not.toBe(second.id);
    expect(labels.map((label) => label.htmlFor)).toEqual([first.id, second.id]);
  });

  it('represents an invalid value without selecting the first option', () => {
    const select = render('missing');

    expect(select.value).toBe('missing');
    expect(select.selectedOptions[0]?.textContent).toBe('Track');
    expect(select.selectedOptions[0]?.disabled).toBe(true);
  });

  it('reports native selection changes', () => {
    const onvaluechange = vi.fn();
    const select = render('one', onvaluechange);

    select.value = 'two';
    select.dispatchEvent(new Event('change', { bubbles: true }));

    expect(onvaluechange).toHaveBeenCalledOnce();
    expect(onvaluechange).toHaveBeenCalledWith('two');
  });

  it('uses native disabled state for unavailable controls', () => {
    mount(Select, {
      target: document.body,
      props: {
        label: 'Empty',
        value: '',
        options: [],
        onvaluechange: () => undefined
      }
    });

    expect(document.querySelector('select')?.disabled).toBe(true);
  });
});
