<script lang="ts">
  import type { HTMLButtonAttributes } from 'svelte/elements';

  type Props = Omit<
    HTMLButtonAttributes,
    'aria-checked' | 'aria-label' | 'children' | 'class' | 'onclick' | 'role'
  > & {
    label: string;
    checked: boolean;
    oncheckedchange: (checked: boolean) => void;
    tone?: 'quiet' | 'surface';
  };

  let {
    label,
    checked,
    oncheckedchange,
    tone = 'surface',
    type = 'button',
    ...attributes
  }: Props = $props();
</script>

<button
  {...attributes}
  {type}
  role="switch"
  aria-label={label}
  aria-checked={checked}
  class:surface={tone === 'surface'}
  onclick={() => oncheckedchange(!checked)}
></button>

<style>
  button {
    position: relative;
    width: 52px;
    min-height: var(--control-height);
    flex: none;
    padding: 0;
    border: 0;
    color: var(--color-fg);
    background: transparent;
    cursor: pointer;
  }

  button::before,
  button::after {
    position: absolute;
    top: 50%;
    content: '';
    transition:
      background var(--motion-fast) ease,
      border-color var(--motion-fast) ease,
      transform var(--motion-fast) ease;
  }

  button::before {
    right: 0;
    left: 0;
    height: 28px;
    border: 1px solid transparent;
    border-radius: var(--radius-round);
    background: var(--color-control-subtle);
    transform: translateY(-50%);
  }

  button.surface::before {
    border-color: var(--color-stroke);
    background: var(--color-surface);
  }

  button::after {
    left: 4px;
    width: 20px;
    aspect-ratio: 1;
    border-radius: var(--radius-round);
    background: currentColor;
    transform: translateY(-50%);
  }

  button[aria-checked='true']::before {
    border-color: var(--color-brand);
    background: var(--color-brand);
  }

  button[aria-checked='true']::after {
    transform: translate(24px, -50%);
  }

  button:hover:not(:disabled)::before {
    border-color: var(--color-stroke-strong);
  }

  button[aria-checked='true']:hover:not(:disabled)::before {
    border-color: var(--color-brand);
  }

  button:disabled {
    opacity: var(--opacity-disabled);
    cursor: default;
  }
</style>
