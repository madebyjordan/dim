<script lang="ts">
  import type {
    CreateLibraryResponse,
    DirectoryListing,
    StorageRoot
  } from '$lib/api/generated';
  import { session } from '$lib/auth/session.svelte';
  import Button from '$lib/primitives/Button.svelte';
  import IconButton from '$lib/primitives/IconButton.svelte';
  import Select from '$lib/primitives/Select.svelte';

  let {
    open,
    onclose,
    oncreated
  }: {
    open: boolean;
    onclose: () => void;
    oncreated: (id: number) => void;
  } = $props();

  let dialog: HTMLDialogElement;
  let name = $state('');
  let mediaType = $state<'movie' | 'tv'>('movie');
  let roots = $state<StorageRoot[]>([]);
  let activeRoot = $state<StorageRoot | null>(null);
  let locationParts = $state<string[]>([]);
  let listing = $state<DirectoryListing | null>(null);
  let selected = $state<string | null>(null);
  let loading = $state(false);
  let submitting = $state(false);
  let error = $state<string | null>(null);

  $effect(() => {
    if (!dialog) return;
    if (open && !dialog.open) {
      dialog.showModal();
      void showRoots();
    } else if (!open && dialog.open) {
      dialog.close();
    }
  });

  async function showRoots() {
    activeRoot = null;
    locationParts = [];
    listing = null;
    selected = null;
    loading = true;
    error = null;
    try {
      roots = await session.api.get<StorageRoot[]>('filebrowser/roots');
    } catch (cause) {
      error =
        cause instanceof Error ? cause.message : 'Storage roots unavailable';
    } finally {
      loading = false;
    }
  }

  async function browse(
    path: string,
    nextLocation: string[],
    nextRoot: StorageRoot | null = activeRoot
  ) {
    loading = true;
    error = null;
    try {
      const nextListing = await session.api.get<DirectoryListing>(
        'filebrowser',
        { path }
      );
      listing = nextListing;
      activeRoot = nextRoot;
      locationParts = nextLocation;
      selected = nextListing.current;
    } catch (cause) {
      error = cause instanceof Error ? cause.message : 'Folder unavailable';
    } finally {
      loading = false;
    }
  }

  function enterRoot(root: StorageRoot) {
    void browse(root.path, [root.display_name], root);
  }

  function openDirectory(directory: { name: string; path: string }) {
    void browse(directory.path, [...locationParts, directory.name]);
  }

  function goBack() {
    if (locationParts.length <= 1 || !listing?.parent) {
      void showRoots();
      return;
    }
    void browse(listing.parent, locationParts.slice(0, -1));
  }

  function formatBytes(bytes: number) {
    if (!Number.isFinite(bytes) || bytes <= 0) return 'Space unavailable';
    const units = ['B', 'KB', 'MB', 'GB', 'TB', 'PB'];
    const unit = Math.min(
      Math.floor(Math.log(bytes) / Math.log(1024)),
      units.length - 1
    );
    const value = bytes / 1024 ** unit;
    return `${value >= 10 || unit === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[unit]} available`;
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (!selected || !name.trim()) return;
    submitting = true;
    error = null;
    try {
      const created = await session.api.post<CreateLibraryResponse>('library', {
        name: name.trim(),
        locations: [selected],
        media_type: mediaType
      });
      name = '';
      selected = null;
      oncreated(created.id);
      onclose();
    } catch (cause) {
      error =
        cause instanceof Error ? cause.message : 'Library creation failed';
    } finally {
      submitting = false;
    }
  }

  function setMediaType(value: string) {
    if (value === 'movie' || value === 'tv') mediaType = value;
  }
</script>

<dialog bind:this={dialog} {onclose}>
  <form onsubmit={submit}>
    <header>
      <h2>Add Library</h2>
      <IconButton label="Close" onclick={onclose}>×</IconButton>
    </header>
    <label>
      Name
      <input bind:value={name} required autocomplete="off" />
    </label>
    <div class="field">
      <span>Type</span>
      <Select
        label="Type"
        value={mediaType}
        options={[
          { value: 'movie', label: 'Movies' },
          { value: 'tv', label: 'Shows' }
        ]}
        onvaluechange={setMediaType}
      />
    </div>
    <section aria-label="Folder">
      <div class="path">Set Directory /{locationParts.join('/')}{locationParts.length ? '/' : ''}</div>
      {#if activeRoot}
        <div class="directories">
          <button type="button" onclick={goBack}>../</button>
          {#each listing?.directories ?? [] as directory (directory.path)}
            <button type="button" onclick={() => openDirectory(directory)}
              >{directory.name}</button
            >
          {/each}
          {#if loading}<span>Loading…</span>{/if}
        </div>
      {:else}
        <div class="roots" aria-label="Storage roots">
          {#each roots as root (root.path)}
            <button type="button" class="root-card" onclick={() => enterRoot(root)}>
              <span class="drive-icon" aria-hidden="true"></span>
              <span class="root-details">
                <strong>{root.display_name}</strong>
                <small>{formatBytes(root.available_bytes)}</small>
              </span>
            </button>
          {/each}
          {#if loading}<span class="root-status">Loading storage…</span>{/if}
          {#if !loading && roots.length === 0}
            <span class="root-status">No readable storage roots found.</span>
          {/if}
        </div>
      {/if}
    </section>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <footer>
      <Button onclick={onclose}>Cancel</Button>
      <Button
        type="submit"
        tone="primary"
        disabled={!name.trim() || !selected || submitting}
        >{submitting ? 'Adding…' : 'Add Library'}</Button
      >
    </footer>
  </form>
</dialog>

<style>
  dialog {
    width: min(92vw, 560px);
    max-height: min(82vh, 720px);
    padding: 0;
    border: 1px solid var(--color-stroke);
    border-radius: var(--radius-lg);
    color: var(--color-fg);
    background: var(--color-surface);
    box-shadow: var(--shadow-float);
  }
  dialog::backdrop {
    background: rgba(0, 0, 0, 0.72);
    backdrop-filter: blur(10px);
  }
  form {
    display: grid;
    gap: var(--space-4);
    padding: var(--space-5);
  }
  header,
  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
  }
  h2,
  p {
    margin: 0;
  }
  .directories button {
    border: 0;
    color: inherit;
    background: transparent;
    cursor: pointer;
  }
  label,
  .field {
    display: grid;
    gap: 7px;
    color: var(--color-fg-muted);
    font-size: var(--text-sm);
  }
  input {
    min-height: var(--control-height-large);
    padding: 0 var(--space-3);
    border: 1px solid var(--color-stroke);
    border-radius: var(--radius-md);
    color: var(--color-fg);
    background: var(--color-canvas);
  }
  .field :global(.select) {
    width: 100%;
  }
  section {
    overflow: hidden;
    border: 1px solid var(--color-stroke);
    border-radius: var(--radius-md);
  }
  .path {
    padding: 11px 13px;
    overflow: hidden;
    border-bottom: 1px solid rgba(255, 255, 255, 0.1);
    color: var(--color-fg-subtle);
    font: var(--text-xs) var(--font-mono);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .directories {
    max-height: 240px;
    display: grid;
    overflow-y: auto;
    padding: 7px;
  }
  .roots {
    max-height: 280px;
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(190px, 1fr));
    gap: 10px;
    overflow-y: auto;
    padding: 12px;
  }
  .root-card {
    min-height: 92px;
    display: flex;
    align-items: center;
    gap: 13px;
    padding: 14px;
    border: 1px solid var(--color-stroke);
    border-radius: var(--radius-md);
    color: inherit;
    background: var(--color-canvas);
    cursor: pointer;
    text-align: left;
  }
  .root-card:hover {
    border-color: rgba(255, 255, 255, 0.3);
    background: rgba(255, 255, 255, 0.06);
  }
  .drive-icon {
    width: 42px;
    height: 29px;
    position: relative;
    flex: 0 0 auto;
    border: 2px solid currentColor;
    border-radius: 5px;
    color: var(--color-fg-muted);
  }
  .drive-icon::after {
    content: '';
    width: 5px;
    height: 5px;
    position: absolute;
    right: 6px;
    bottom: 5px;
    border-radius: 50%;
    background: currentColor;
  }
  .root-details {
    min-width: 0;
    display: grid;
    gap: 5px;
  }
  .root-details strong,
  .root-details small {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .root-details small,
  .root-status {
    color: var(--color-fg-subtle);
    font-size: var(--text-xs);
  }
  .root-status {
    padding: 12px;
  }
  .directories button {
    padding: 9px 8px;
    border-radius: 6px;
    text-align: left;
  }
  .directories button:hover {
    background: rgba(255, 255, 255, 0.07);
  }
  .directories span {
    padding: 10px;
    color: var(--color-fg-subtle);
  }
  footer {
    justify-content: end;
  }
  .error {
    color: var(--color-danger);
    font-size: 13px;
  }
</style>
