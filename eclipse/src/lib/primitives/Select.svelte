<script lang="ts">
  import type { HTMLButtonAttributes } from 'svelte/elements';
  import Popout, { type PopoutController } from './internal/Popout.svelte';
  import { findEnabledIndex } from './internal/navigation';

  export type SelectOption = {
    value: string;
    label: string;
    disabled?: boolean;
  };

  type Props = {
    label: string;
    value: string;
    options: SelectOption[];
    disabled?: boolean;
    onvaluechange: (value: string) => void;
  };

  let {
    label,
    value,
    options,
    disabled = false,
    onvaluechange
  }: Props = $props();

  const componentId = $props.id();
  const selectedIndex = $derived(
    options.findIndex((option) => option.value === value)
  );
  const selectedOption = $derived(options[selectedIndex]);
  const optionIdBase = `select-option-${componentId}`;
  let activeIndex = $state(-1);
  const activeOptionId = $derived(
    activeIndex >= 0 ? `${optionIdBase}-${activeIndex}` : undefined
  );

  function handleOpen(surface: HTMLDivElement, openingKey?: string) {
    const direction = openingKey === 'ArrowUp' ? -1 : 1;
    activeIndex = findEnabledIndex(
      options,
      selectedIndex >= 0
        ? selectedIndex
        : direction === 1
          ? 0
          : options.length - 1,
      direction,
      true
    );
    surface.focus();
  }

  function move(direction: 1 | -1) {
    activeIndex = findEnabledIndex(
      options,
      activeIndex < 0 ? (direction === 1 ? -1 : 0) : activeIndex,
      direction,
      false
    );
  }

  function activate(index: number, popout: PopoutController) {
    const option = options[index];
    if (!option || option.disabled) return;
    popout.close();
    onvaluechange(option.value);
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
      activeIndex = findEnabledIndex(
        options,
        direction === 1 ? 0 : options.length - 1,
        direction,
        true
      );
    } else if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      event.stopPropagation();
      activate(activeIndex, popout);
    }
  }
</script>

{#snippet trigger(attributes: HTMLButtonAttributes)}
  <button class="trigger" {...attributes} aria-label={label}>
    <span>{selectedOption?.label ?? label}</span>
    <svg viewBox="0 0 20 20" aria-hidden="true">
      <path d="m4 7 6 6 6-6" />
    </svg>
  </button>
{/snippet}

<div class="select">
  <Popout
    {label}
    popupRole="listbox"
    {trigger}
    disabled={disabled || options.length === 0}
    activeDescendant={activeOptionId}
    onopen={handleOpen}
    onkeydown={handleKeydown}
  >
    {#snippet children(popout)}
      {#each options as option, index}
        <button
          id={`${optionIdBase}-${index}`}
          type="button"
          role="option"
          aria-selected={index === selectedIndex}
          aria-disabled={option.disabled ?? false}
          disabled={option.disabled}
          data-popout-item
          data-highlighted={index === activeIndex ? '' : undefined}
          tabindex="-1"
          onpointerdown={(event) => event.preventDefault()}
          onpointerenter={() => {
            if (!option.disabled) activeIndex = index;
          }}
          onclick={() => activate(index, popout)}
        >
          <span>{option.label}</span>
          {#if index === selectedIndex}<span aria-hidden="true">✓</span>{/if}
        </button>
      {/each}
    {/snippet}
  </Popout>
</div>

<style>
  .select {
    display: inline-grid;
    min-width: 0;
    max-width: 100%;
  }

  .trigger {
    width: 100%;
    min-height: var(--control-height);
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    padding: 0 var(--space-3) 0 var(--space-4);
    border: 0;
    border-radius: var(--radius-round);
    color: var(--color-fg);
    font: inherit;
    background: var(--color-control);
    -webkit-backdrop-filter: blur(var(--blur-control));
    backdrop-filter: blur(var(--blur-control));
    white-space: nowrap;
    cursor: pointer;
  }

  .trigger:disabled {
    opacity: var(--opacity-disabled);
    cursor: default;
  }

  svg {
    width: 20px;
    height: 20px;
    flex: none;
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    transition: transform var(--motion-fast) ease;
  }

  .trigger[aria-expanded='true'] svg {
    transform: rotate(180deg);
  }

  [role='option'] {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
  }

  [role='option'][aria-selected='true'] {
    font-weight: 600;
  }
</style>
