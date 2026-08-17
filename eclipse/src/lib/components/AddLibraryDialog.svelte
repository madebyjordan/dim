<script lang="ts">
  import type {
    CreateLibraryResponse,
    DirectoryListing
  } from '$lib/api/generated';
  import { session } from '$lib/auth/session.svelte';
  import Button from '$lib/primitives/Button.svelte';
  import IconButton from '$lib/primitives/IconButton.svelte';

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
  let listing = $state<DirectoryListing | null>(null);
  let selected = $state<string | null>(null);
  let loading = $state(false);
  let submitting = $state(false);
  let error = $state<string | null>(null);

  $effect(() => {
    if (!dialog) return;
    if (open && !dialog.open) {
      dialog.showModal();
      void browse();
    } else if (!open && dialog.open) {
      dialog.close();
    }
  });

  async function browse(path?: string) {
    loading = true;
    error = null;
    try {
      listing = await session.api.get<DirectoryListing>('filebrowser', {
        path
      });
      selected = listing.current;
    } catch (cause) {
      error = cause instanceof Error ? cause.message : 'Folder unavailable';
    } finally {
      loading = false;
    }
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
    <label>
      Type
      <select bind:value={mediaType}>
        <option value="movie">Movies</option>
        <option value="tv">Shows</option>
      </select>
    </label>
    <section aria-label="Folder">
      <div class="path">{listing?.current ?? 'Loading folders…'}</div>
      <div class="directories">
        {#if listing?.parent}
          <button
            type="button"
            onclick={() => browse(listing?.parent ?? undefined)}>../</button
          >
        {/if}
        {#each listing?.directories ?? [] as directory (directory.path)}
          <button type="button" onclick={() => browse(directory.path)}
            >{directory.name}</button
          >
        {/each}
        {#if loading}<span>Loading…</span>{/if}
      </div>
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
  label {
    display: grid;
    gap: 7px;
    color: var(--color-fg-muted);
    font-size: 13px;
  }
  input,
  select {
    min-height: var(--control-height-large);
    padding: 0 var(--space-3);
    border: 1px solid var(--color-stroke);
    border-radius: var(--radius-md);
    color: var(--color-fg);
    background: var(--color-canvas);
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
    font: 12px var(--font-mono);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .directories {
    max-height: 240px;
    display: grid;
    overflow-y: auto;
    padding: 7px;
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
    color: rgba(255, 255, 255, 0.42);
  }
  footer {
    justify-content: end;
  }
  .error {
    color: var(--color-danger);
    font-size: 13px;
  }
</style>
