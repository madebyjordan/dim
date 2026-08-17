<script lang="ts">
  import type { Snippet } from 'svelte';

  let {
    children,
    size = '1em',
    viewBox = '0 0 24 24',
    label
  }: {
    children: Snippet;
    size?: number | string;
    viewBox?: string;
    label?: string;
  } = $props();
</script>

<svg
  class="icon"
  width={size}
  height={size}
  {viewBox}
  fill="none"
  xmlns="http://www.w3.org/2000/svg"
  role={label ? 'img' : undefined}
  aria-label={label}
  aria-hidden={label ? undefined : 'true'}
  focusable="false"
>
  {@render children()}
</svg>

<style>
  .icon {
    display: block;
    flex: none;
    color: inherit;
  }
</style>
