<script lang="ts">
  import type { HTMLButtonAttributes } from 'svelte/elements';
  import DropdownMenu, { type DropdownMenuItem } from './DropdownMenu.svelte';
  import IconButton from './IconButton.svelte';

  let {
    disabled = false,
    onaction
  }: {
    disabled?: boolean;
    onaction: (action: string) => void;
  } = $props();

  const items: DropdownMenuItem[] = [
    {
      label: 'Unavailable action',
      disabled: true,
      onselect: () => onaction('unavailable')
    },
    { label: 'Available action', onselect: () => onaction('available') },
    {
      label: 'Second action',
      separatorBefore: true,
      onselect: () => onaction('second')
    }
  ];
</script>

{#snippet trigger(attributes: HTMLButtonAttributes)}
  <IconButton {...attributes} label="Open actions">•••</IconButton>
{/snippet}

{#snippet header()}<span>Account</span>{/snippet}

<DropdownMenu label="Actions" {trigger} {header} {items} {disabled} />
