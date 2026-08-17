<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { HTMLSelectAttributes } from 'svelte/elements';

  type Props = Omit<
    HTMLSelectAttributes,
    'aria-label' | 'children' | 'class'
  > & {
    label: string;
    children: Snippet;
  };

  let { label, children, ...attributes }: Props = $props();
</script>

<label>
  <span>{label}</span>
  <select {...attributes} aria-label={label}>
    {@render children()}
  </select>
</label>

<style>
  label {
    min-width: 0;
    display: grid;
  }
  span {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
    border: 0;
  }
  select {
    width: 100%;
    max-width: 260px;
    min-height: var(--control-height);
    padding: 0 var(--space-2) 0 var(--space-2);
    border: none;
    border-radius: var(--radius-round);
    color: var(--color-fg);
    background: var(--color-control);
  }
</style>
