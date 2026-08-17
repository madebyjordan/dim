<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { HTMLButtonAttributes } from 'svelte/elements';

  type Props = Omit<HTMLButtonAttributes, 'children' | 'class'> & {
    children: Snippet;
    tone?: 'primary' | 'quiet' | 'surface';
  };

  let {
    children,
    tone = 'quiet',
    type = 'button',
    ...attributes
  }: Props = $props();
</script>

<button
  {...attributes}
  {type}
  class:primary={tone === 'primary'}
  class:surface={tone === 'surface'}
>
  {@render children()}
</button>

<style>
  button {
    min-height: var(--control-height);
    padding: 0 var(--space-4);
    border-radius: var(--radius-md);
    color: var(--color-fg-muted);
    background: transparent;
    cursor: pointer;
  }
  button:hover:not(:disabled) {
    color: var(--color-fg);
    border-color: var(--color-stroke-strong);
  }
  button.primary {
    color: var(--color-canvas);
    border-color: var(--color-fg);
    background: var(--color-fg);
    font-weight: 700;
  }
  button.surface {
    color: var(--color-fg);
    background: var(--color-surface);
  }
  button:disabled {
    opacity: var(--opacity-disabled);
    cursor: default;
  }
</style>
