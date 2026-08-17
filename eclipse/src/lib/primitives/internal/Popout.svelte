<script module lang="ts">
  export type PopoutController = {
    close: (restoreFocus?: boolean) => void;
  };
</script>

<script lang="ts">
  import { tick, type Snippet } from 'svelte';
  import type { HTMLButtonAttributes } from 'svelte/elements';

  type Props = {
    label: string;
    popupRole: 'listbox' | 'menu';
    trigger: Snippet<[HTMLButtonAttributes]>;
    children: Snippet<[PopoutController]>;
    align?: 'start' | 'end';
    disabled?: boolean;
    activeDescendant?: string;
    onopen?: (surface: HTMLDivElement, openingKey?: string) => void;
    onkeydown?: (event: KeyboardEvent, popout: PopoutController) => void;
  };

  let {
    label,
    popupRole,
    trigger,
    children,
    align = 'start',
    disabled = false,
    activeDescendant,
    onopen,
    onkeydown
  }: Props = $props();

  const componentId = $props.id();
  const triggerId = `popout-trigger-${componentId}`;
  const surfaceId = `popout-${componentId}`;
  let root: HTMLDivElement;
  let triggerElement: HTMLButtonElement | null = null;
  let surfaceElement = $state<HTMLDivElement>();
  let open = $state(false);

  const triggerAttributes = $derived<HTMLButtonAttributes>({
    id: triggerId,
    type: 'button',
    disabled,
    'aria-haspopup': popupRole,
    'aria-expanded': open,
    'aria-controls': surfaceId,
    onclick: handleTriggerClick,
    onkeydown: handleTriggerKeydown
  });

  async function openPopout(openingKey?: string) {
    if (disabled || open) return;
    open = true;
    await tick();
    if (surfaceElement) onopen?.(surfaceElement, openingKey);
  }

  function close(restoreFocus = true) {
    if (!open) return;
    open = false;
    if (restoreFocus) triggerElement?.focus();
  }

  const controller: PopoutController = { close };

  function handleTriggerClick(event: MouseEvent) {
    triggerElement = event.currentTarget as HTMLButtonElement;
    if (open) close(false);
    else void openPopout();
  }

  function handleTriggerKeydown(event: KeyboardEvent) {
    triggerElement = event.currentTarget as HTMLButtonElement;
    if (!['ArrowDown', 'ArrowUp', 'Enter', ' '].includes(event.key)) return;
    event.preventDefault();
    event.stopPropagation();
    void openPopout(event.key);
  }

  function handleSurfaceKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      close();
    } else if (event.key === 'Tab') {
      close(false);
    } else {
      onkeydown?.(event, controller);
    }
  }

  function handleDocumentPointerDown(event: PointerEvent) {
    if (open && !root.contains(event.target as Node)) close(false);
  }

  $effect(() => {
    if (!open) return;
    document.addEventListener('pointerdown', handleDocumentPointerDown);
    return () =>
      document.removeEventListener('pointerdown', handleDocumentPointerDown);
  });
</script>

<div class="popout" bind:this={root}>
  {@render trigger(triggerAttributes)}

  {#if open}
    <div
      class="surface"
      class:align-end={align === 'end'}
      id={surfaceId}
      role={popupRole}
      aria-label={label}
      aria-activedescendant={activeDescendant}
      tabindex="-1"
      data-popout-surface
      bind:this={surfaceElement}
      onkeydown={handleSurfaceKeydown}
    >
      {@render children(controller)}
    </div>
  {/if}
</div>

<style>
  .popout {
    position: relative;
    display: inline-grid;
    min-width: 0;
  }

  .surface {
    position: absolute;
    z-index: 10;
    top: calc(100% + var(--space-2));
    left: 0;
    width: max-content;
    min-width: max(100%, var(--popout-min-width));
    max-width: calc(100vw - 2 * var(--space-4));
    display: grid;
    padding: var(--space-2);
    border: 1px solid var(--color-stroke);
    border-radius: var(--radius-lg);
    color: var(--color-fg);
    background: var(--color-popout);
    box-shadow: var(--shadow-float);
    -webkit-backdrop-filter: blur(var(--blur-control));
    backdrop-filter: blur(var(--blur-control));
  }

  .surface.align-end {
    right: 0;
    left: auto;
  }

  .surface :global([data-popout-item]) {
    min-height: var(--control-height);
    padding: 0 var(--space-3);
    border: 0;
    border-radius: var(--radius-md);
    color: inherit;
    background: transparent;
    text-align: left;
    white-space: nowrap;
    cursor: pointer;
  }

  .surface :global([data-popout-item][data-highlighted]) {
    background: var(--color-control-hover);
  }

  .surface :global([data-popout-item]:disabled) {
    opacity: var(--opacity-disabled);
    cursor: default;
  }
</style>
