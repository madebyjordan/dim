<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { HTMLButtonAttributes } from 'svelte/elements';

  type Props = Omit<
    HTMLButtonAttributes,
    'aria-label' | 'aria-expanded' | 'children' | 'class'
  > & {
    label: string;
    children: Snippet;
    appearance?: 'plain' | 'surface';
    expanded?: boolean;
  };

  let {
    label,
    children,
    appearance = 'plain',
    expanded,
    type = 'button',
    ...attributes
  }: Props = $props();
</script>

<button
  {...attributes}
  {type}
  class:surface={appearance === 'surface'}
  aria-label={label}
  aria-expanded={expanded}
>
  {@render children()}
</button>

<style>
  button {
    position: relative;
    width: clamp(38px, 2.3vw, 68px);
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
    width: clamp(48px, 3.35vw, 92px);
    padding: 24%;
    overflow: hidden;
    background: rgba(255, 255, 255, 0.18);
  }
</style>
