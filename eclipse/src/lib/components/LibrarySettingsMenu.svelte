<script lang="ts">
  import type { HTMLButtonAttributes } from 'svelte/elements';
  import type { Library } from '$lib/api/generated';
  import IconButton from '$lib/primitives/IconButton.svelte';
  import Switch from '$lib/primitives/Switch.svelte';
  import Popout, {
    type PopoutController
  } from '$lib/primitives/internal/Popout.svelte';

  let {
    library,
    scanning,
    hasselection,
    onautoscan,
    onscan,
    ondelete,
    oneditinfo
  }: {
    library: Library;
    scanning: boolean;
    hasselection: boolean;
    onautoscan: (enabled: boolean) => void | Promise<void>;
    onscan: () => void | Promise<void>;
    ondelete: () => void | Promise<void>;
    oneditinfo: () => void;
  } = $props();

  let pending = $state<'auto-scan' | 'scan' | 'delete' | 'edit' | null>(null);

  async function run(
    action: NonNullable<typeof pending>,
    callback: () => void | Promise<void>,
    popout?: PopoutController
  ) {
    if (pending) return;
    pending = action;
    try {
      if (action !== 'auto-scan') popout?.close();
      await callback();
    } catch {
      // The page presents API errors in its existing status surface.
    } finally {
      pending = null;
    }
  }

  function focusFirstControl(surface: HTMLDivElement) {
    surface.querySelector<HTMLButtonElement>('button')?.focus();
  }
</script>

{#snippet trigger(attributes: HTMLButtonAttributes)}
  <IconButton
    {...attributes}
    label={`Open settings for ${library.name}`}
    tone="surface"
  >
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="M4 7h10M18 7h2M4 17h2M10 17h10" />
      <circle cx="16" cy="7" r="2" />
      <circle cx="8" cy="17" r="2" />
    </svg>
  </IconButton>
{/snippet}

<Popout
  label={`${library.name} library settings`}
  popupRole="dialog"
  {trigger}
  align="end"
  onopen={focusFirstControl}
>
  {#snippet children(popout: PopoutController)}
    <div class="panel">
      <h2>Library settings</h2>
      <div class="setting">
        <span>Auto scan</span>
        <Switch
          label={`Automatically scan ${library.name}`}
          checked={library.auto_scan}
          disabled={pending !== null}
          oncheckedchange={(enabled) =>
            void run('auto-scan', () => onautoscan(enabled))}
        />
      </div>
      <button
        type="button"
        data-popout-item
        disabled={pending !== null || scanning}
        onclick={() => void run('scan', onscan, popout)}
        >{scanning ? 'Scanning…' : 'Manual scan'}</button
      >
      <button
        type="button"
        class="danger"
        data-popout-item
        disabled={pending !== null}
        onclick={() => void run('delete', ondelete, popout)}
        >Delete library</button
      >
      {#if hasselection}
        <div class="file-settings">
          <h2>File settings</h2>
          <button
            type="button"
            data-popout-item
            disabled={pending !== null}
            onclick={() => void run('edit', oneditinfo, popout)}
            >Edit Info</button
          >
        </div>
      {/if}
    </div>
  {/snippet}
</Popout>

<style>
  .panel {
    width: 280px;
    display: grid;
  }

  h2 {
    margin: 0;
    padding: var(--space-3);
    color: var(--color-fg-muted);
    font-size: var(--text-lg);
    font-weight: 500;
  }

  .setting {
    min-height: calc(var(--control-height) + var(--space-2));
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
    padding-inline: var(--space-3);
  }

  button[data-popout-item] {
    width: 100%;
    font-size: var(--text-md);
  }

  button.danger[data-popout-item] {
    color: var(--color-danger);
  }

  .file-settings {
    display: grid;
    margin-top: var(--space-2);
    padding-top: var(--space-2);
    border-top: 1px solid var(--color-stroke);
  }

  svg {
    width: 100%;
    height: 100%;
    fill: none;
    stroke: currentColor;
    stroke-linecap: round;
    stroke-linejoin: round;
    stroke-width: 1.5;
  }
</style>
