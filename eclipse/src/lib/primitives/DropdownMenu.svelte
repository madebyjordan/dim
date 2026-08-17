<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { HTMLButtonAttributes } from 'svelte/elements';
  import Popout, { type PopoutController } from './internal/Popout.svelte';
  import { findEnabledIndex } from './internal/navigation';

  export type DropdownMenuItem = {
    label: string;
    disabled?: boolean;
    separatorBefore?: boolean;
    onselect: () => void;
  };

  type Props = {
    label: string;
    trigger: Snippet<[HTMLButtonAttributes]>;
    items: DropdownMenuItem[];
    header?: Snippet;
    align?: 'start' | 'end';
    disabled?: boolean;
  };

  let {
    label,
    trigger,
    items,
    header,
    align = 'start',
    disabled = false
  }: Props = $props();

  let activeIndex = $state(-1);
  let itemElements = $state<HTMLButtonElement[]>([]);

  function focus(index: number) {
    activeIndex = index;
    if (index >= 0) itemElements[index]?.focus();
  }

  function handleOpen(surface: HTMLDivElement, openingKey?: string) {
    const direction = openingKey === 'ArrowUp' ? -1 : 1;
    focus(
      findEnabledIndex(
        items,
        direction === 1 ? 0 : items.length - 1,
        direction,
        true
      )
    );
    if (activeIndex < 0) surface.focus();
  }

  function move(direction: 1 | -1) {
    focus(
      findEnabledIndex(
        items,
        activeIndex < 0 ? (direction === 1 ? -1 : 0) : activeIndex,
        direction,
        false
      )
    );
  }

  function activate(index: number, popout: PopoutController) {
    const item = items[index];
    if (!item || item.disabled) return;
    popout.close();
    item.onselect();
  }

  function handleKeydown(event: KeyboardEvent, popout: PopoutController) {
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      event.stopPropagation();
      move(event.key === 'ArrowDown' ? 1 : -1);
    } else if (event.key === 'Home' || event.key === 'End') {
      event.preventDefault();
      event.stopPropagation();
      const direction = event.key === 'Home' ? 1 : -1;
      focus(
        findEnabledIndex(
          items,
          direction === 1 ? 0 : items.length - 1,
          direction,
          true
        )
      );
    } else if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      event.stopPropagation();
      activate(activeIndex, popout);
    }
  }
</script>

<Popout
  {label}
  popupRole="menu"
  {trigger}
  {align}
  disabled={disabled || items.length === 0}
  onopen={handleOpen}
  onkeydown={handleKeydown}
>
  {#snippet children(popout: PopoutController)}
    {#if header}<div class="header">{@render header()}</div>{/if}
    {#each items as item, index}
      {#if item.separatorBefore}
        <div class="separator" role="separator"></div>
      {/if}
      <button
        type="button"
        role="menuitem"
        disabled={item.disabled}
        data-popout-item
        data-highlighted={index === activeIndex ? '' : undefined}
        tabindex={index === activeIndex ? 0 : -1}
        bind:this={itemElements[index]}
        onfocus={() => (activeIndex = index)}
        onpointerenter={() => {
          if (!item.disabled) focus(index);
        }}
        onclick={() => activate(index, popout)}>{item.label}</button
      >
    {/each}
  {/snippet}
</Popout>

<style>
  .header {
    padding: var(--space-1) var(--space-2) var(--space-2);
    color: var(--color-fg-subtle);
    font-size: var(--text-xs);
  }

  .separator {
    height: 1px;
    margin: var(--space-2);
    background: var(--color-stroke);
  }
</style>
