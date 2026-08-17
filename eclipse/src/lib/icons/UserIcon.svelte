<script lang="ts">
  import Icon from '$lib/primitives/Icon.svelte';

  let { size = 24 }: { size?: number | string } = $props();
</script>

<Icon {size}>
  <circle cx="12" cy="8.5" r="3.5" stroke="currentColor" />
  <path
    d="M6 19C6 14.5001 9.28778 15 12 15C14.7122 15 18 14.5001 18 19"
    stroke="currentColor"
  />
</Icon>
