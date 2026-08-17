<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { HTMLButtonAttributes } from 'svelte/elements';

  type Props = Omit<
    HTMLButtonAttributes,
    'aria-label' | 'children' | 'class'
  > & {
    label: string;
    children: Snippet;
    tone?: 'quiet' | 'surface';
  };

  let {
    label,
    children,
    tone = 'quiet',
    type = 'button',
    ...attributes
  }: Props = $props();
</script>

<button
  {...attributes}
  {type}
  class:surface={tone === 'surface'}
  aria-label={label}
>
  {@render children()}
</button>

<style>
  button {
    position: relative;
    width: var(--control-height-large);
    aspect-ratio: 1;
    display: grid;
    flex: none;
    place-items: center;
    padding: 0;
    border: 0;
    border-radius: var(--radius-round);
    color: var(--color-fg);
    background: transparent;
    cursor: pointer;
  }
  button.surface {
    width: 52px;
    padding: 24%;
    overflow: hidden;
    background: var(--color-control-subtle);
  }
  button:disabled {
    opacity: var(--opacity-disabled);
    cursor: default;
  }
  @media (min-width: 1800px) {
    button {
      width: 52px;
    }
    button.surface {
      width: 64px;
    }
  }
</style>
