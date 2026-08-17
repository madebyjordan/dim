<script lang="ts">
  import Switch from './Switch.svelte';

  let { disabled = false }: { disabled?: boolean } = $props();
  let checked = $state(false);
</script>

<Switch
  label="Setting"
  {checked}
  {disabled}
  oncheckedchange={(value) => (checked = value)}
/>
