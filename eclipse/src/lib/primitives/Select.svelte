<script lang="ts">
  import { onMount } from 'svelte';

  export type SelectOption = { value: string; label: string };

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
  let root: HTMLDivElement;
  let open = $state(false);
  let activeIndex = $state(0);
  const selectedIndex = $derived(
    Math.max(
      0,
      options.findIndex((option) => option.value === value)
    )
  );
  const selectedOption = $derived(options[selectedIndex]);
  const menuId = $derived(
    `select-${label.toLowerCase().replace(/[^a-z0-9]+/g, '-')}`
  );

  function showMenu() {
    if (disabled || options.length === 0) return;
    activeIndex = selectedIndex;
    open = true;
  }

  function choose(index: number, restoreFocus: boolean) {
    const option = options[index];
    if (!option) return;
    onvaluechange(option.value);
    open = false;
    if (restoreFocus) {
      root.querySelector<HTMLButtonElement>('.trigger')?.focus();
    } else if (document.activeElement instanceof HTMLElement) {
      // Pointer selection is complete. Do not leave focus pinned inside player controls,
      // otherwise their accessibility focus guard prevents the inactivity fade indefinitely.
      document.activeElement.blur();
    }
  }

  function handleTriggerKeydown(event: KeyboardEvent) {
    if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return;
    event.preventDefault();
    showMenu();
    if (event.key === 'ArrowUp') activeIndex = options.length - 1;
  }

  function handleMenuKeydown(event: KeyboardEvent) {
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      const offset = event.key === 'ArrowDown' ? 1 : -1;
      activeIndex = (activeIndex + offset + options.length) % options.length;
    } else if (event.key === 'Home' || event.key === 'End') {
      event.preventDefault();
      activeIndex = event.key === 'Home' ? 0 : options.length - 1;
    } else if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      choose(activeIndex, true);
    } else if (event.key === 'Escape' || event.key === 'Tab') {
      open = false;
    }
  }

  $effect(() => {
    if (!open) return;
    activeIndex;
    requestAnimationFrame(() => {
      root
        .querySelector<HTMLButtonElement>(`[data-option="${activeIndex}"]`)
        ?.focus();
    });
  });

  onMount(() => {
    function dismiss(event: PointerEvent) {
      if (!root.contains(event.target as Node)) open = false;
    }
    document.addEventListener('pointerdown', dismiss);
    return () => document.removeEventListener('pointerdown', dismiss);
  });
</script>

<div class="select" class:open bind:this={root}>
  <button
    class="trigger"
    type="button"
    {disabled}
    aria-label={label}
    aria-haspopup="listbox"
    aria-expanded={open}
    aria-controls={menuId}
    onclick={() => (open ? (open = false) : showMenu())}
    onkeydown={handleTriggerKeydown}
  >
    <span>{selectedOption?.label ?? label}</span>
    <svg viewBox="0 0 20 20" aria-hidden="true">
      <path d="m4 7 6 6 6-6" />
    </svg>
  </button>

  {#if open}
    <div
      class="menu"
      id={menuId}
      role="listbox"
      aria-label={label}
      tabindex="-1"
      onkeydown={handleMenuKeydown}
    >
      {#each options as option, index (option.value)}
        <button
          type="button"
          role="option"
          id={`${menuId}-${index}`}
          aria-selected={option.value === value}
          class:active={index === activeIndex}
          data-option={index}
          tabindex="-1"
          onclick={(event) => choose(index, event.detail === 0)}
          onpointerenter={() => (activeIndex = index)}>{option.label}</button
        >
      {/each}
    </div>
  {/if}
</div>

<style>
  .select {
    position: relative;
    min-width: 0;
    max-width: 260px;
  }

  .select.open {
    z-index: 10;
  }

  button {
    border: 0;
    color: var(--color-fg);
    font: inherit;
  }

  .trigger {
    width: 100%;
    min-height: var(--control-height);
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    padding: 0 var(--space-3) 0 var(--space-4);
    border-radius: var(--radius-round);
    background: var(--color-control);
    backdrop-filter: blur(8px);
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
    flex: 0 0 20px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
    transition: transform var(--motion-fast) ease;
  }

  .open svg {
    transform: rotate(180deg);
  }

  .menu {
    position: absolute;
    top: calc(100% + var(--space-2));
    left: 0;
    width: max-content;
    min-width: 100%;
    display: grid;
    padding: var(--space-2);
    border: 1px solid var(--color-stroke);
    border-radius: var(--radius-lg);
    background: rgba(78, 78, 74, 0.78);
    box-shadow: var(--shadow-float);
    backdrop-filter: blur(18px);
  }

  .menu button {
    min-height: var(--control-height);
    padding: 0 var(--space-3);
    border-radius: var(--radius-md);
    background: transparent;
    text-align: left;
    white-space: nowrap;
    cursor: pointer;
  }

  .menu button:hover,
  .menu button.active {
    background: rgba(255, 255, 255, 0.14);
  }

  .menu button[aria-selected='true'] {
    font-weight: 600;
  }
</style>
