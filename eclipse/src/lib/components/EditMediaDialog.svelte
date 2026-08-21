<script lang="ts">
  import type { Library, Media, MediaFile } from '$lib/api/generated';
  import { session } from '$lib/auth/session.svelte';
  import { imageUrl } from '$lib/catalog/catalog';
  import Button from '$lib/primitives/Button.svelte';
  import IconButton from '$lib/primitives/IconButton.svelte';

  type ExternalSearchResult = {
    id: string;
    title: string;
    year?: number | null;
    overview?: string | null;
    poster_path?: string | null;
  };

  let {
    open,
    media,
    file,
    library,
    onclose,
    onsaved
  }: {
    open: boolean;
    media: Media | null;
    file: MediaFile | null;
    library: Library | null;
    onclose: () => void;
    onsaved: (title: string) => void | Promise<void>;
  } = $props();

  let dialog: HTMLDialogElement;
  let mode = $state<'auto' | 'manual'>('auto');
  let contextFiles = $state<MediaFile[]>([]);
  let query = $state('');
  let results = $state<ExternalSearchResult[]>([]);
  let searching = $state(false);
  let saving = $state(false);
  let error = $state<string | null>(null);
  let title = $state('');
  let synopsis = $state('');
  let year = $state('');
  let genres = $state('');
  let language = $state('');
  let rating = $state('');
  let artwork = $state('');
  let searchVersion = 0;

  const contextFile = $derived(file ?? contextFiles[0] ?? null);
  const unmatched = $derived(
    contextFile?.metadata_provider === 'local' &&
      contextFile?.match_provenance === 'local_filename'
  );

  $effect(() => {
    if (!dialog) return;
    if (open && media && !dialog.open) {
      mode = 'auto';
      query = contextFile?.raw_name || media.name;
      title = media.name;
      synopsis = media.description ?? '';
      year = media.year ? String(media.year) : '';
      genres = media.genres.join(', ');
      language = media.language ?? '';
      rating = media.rating === undefined ? '' : String(media.rating);
      artwork = '';
      results = [];
      error = null;
      dialog.showModal();
      void loadFilesAndSearch();
    } else if (!open && dialog.open) {
      dialog.close();
    }
  });

  async function loadFilesAndSearch() {
    if (!media || !library) return;
    try {
      contextFiles = await session.api.get<MediaFile[]>(
        `media/${media.id}/files`
      );
      query = contextFiles[0]?.raw_name || media.name;
      await search();
    } catch (cause) {
      if (file) {
        contextFiles = [file];
        query = file.raw_name || media.name;
        await search();
      } else {
        error =
          cause instanceof Error
            ? cause.message
            : 'File details could not load';
      }
    }
  }

  async function search() {
    const normalized = query.trim();
    if (!normalized || !library) {
      results = [];
      return;
    }
    const version = ++searchVersion;
    searching = true;
    error = null;
    try {
      const found = await session.api.get<ExternalSearchResult[]>(
        'media/tmdb_search',
        { query: normalized, media_type: library.media_type }
      );
      if (version === searchVersion) results = found;
    } catch (cause) {
      if (version === searchVersion) {
        results = [];
        error = cause instanceof Error ? cause.message : 'No matches found';
      }
    } finally {
      if (version === searchVersion) searching = false;
    }
  }

  async function applyMatch(result: ExternalSearchResult) {
    if (!library || contextFiles.length === 0 || saving) return;
    saving = true;
    error = null;
    try {
      await session.api.patch('mediafile/match', {
        tmdb_id: result.id,
        media_type: library.media_type,
        mediafiles: contextFiles.map((candidate) => candidate.id)
      });
      onclose();
      await onsaved(result.title);
    } catch (cause) {
      error =
        cause instanceof Error
          ? cause.message
          : 'The match could not be applied';
    } finally {
      saving = false;
    }
  }

  function optionalNumber(value: string | number): number | null {
    if (value === '' || value === null) return null;
    const parsed = Number(value);
    return Number.isFinite(parsed) ? parsed : null;
  }

  async function saveManual(event?: SubmitEvent) {
    event?.preventDefault();
    if (!media) return;
    saving = true;
    error = null;
    try {
      await session.api.patch(`media/${media.id}/manual`, {
        title: title.trim(),
        synopsis: synopsis.trim(),
        year: optionalNumber(year),
        genres: genres
          .split(',')
          .map((genre) => genre.trim())
          .filter(Boolean),
        language: language.trim(),
        rating: optionalNumber(rating),
        artwork: artwork.trim() || null
      });
      const savedTitle = title.trim();
      onclose();
      await onsaved(savedTitle);
    } catch (cause) {
      error =
        cause instanceof Error
          ? cause.message
          : 'Manual metadata could not be saved';
    } finally {
      saving = false;
    }
  }
</script>

<dialog bind:this={dialog} {onclose} aria-label="Edit Media">
  {#if media}
    <div class="shell">
      <header>
        <h2>Edit Media</h2>
        <IconButton label="Close" onclick={onclose}>×</IconButton>
      </header>

      <section class="file-context" aria-label="Selected file">
        <div class="poster">
          {#if imageUrl(media.poster_path)}
            <img src={imageUrl(media.poster_path) ?? ''} alt="" />
          {:else}
            <span aria-hidden="true">▶</span>
          {/if}
        </div>
        <div>
          <strong>{contextFile?.target_file ?? media.name}</strong>
          {#if unmatched}<span class="status">Unmatched</span>{/if}
        </div>
      </section>

      <nav aria-label="Edit mode">
        <button
          type="button"
          class:active={mode === 'auto'}
          onclick={() => (mode = 'auto')}>Auto</button
        >
        <button
          type="button"
          class:active={mode === 'manual'}
          onclick={() => (mode = 'manual')}>Manual</button
        >
      </nav>

      {#if mode === 'auto'}
        <div class="auto">
          <form
            class="search"
            onsubmit={(event) => {
              event.preventDefault();
              void search();
            }}
          >
            <label>
              Title search
              <span
                ><input bind:value={query} autocomplete="off" /><Button
                  type="submit"
                  disabled={searching}
                  >{searching ? 'Searching…' : 'Search'}</Button
                ></span
              >
            </label>
          </form>
          <div
            class="results"
            aria-label="Metadata matches"
            aria-busy={searching}
          >
            {#each results as result (result.id)}
              <button
                type="button"
                class="result"
                disabled={saving}
                onclick={() => void applyMatch(result)}
              >
                <div class="result-poster">
                  {#if result.poster_path}<img
                      src={result.poster_path}
                      alt=""
                    />{:else}<span aria-hidden="true">▶</span>{/if}
                </div>
                <span class="result-copy">
                  <strong
                    >{result.title}{result.year
                      ? ` (${result.year})`
                      : ''}</strong
                  >
                  <small>{result.overview || 'No synopsis available.'}</small>
                </span>
              </button>
            {/each}
            {#if !searching && results.length === 0 && !error}<p>
                No results yet.
              </p>{/if}
          </div>
        </div>
      {:else}
        <form class="manual" onsubmit={saveManual}>
          <div class="grid">
            <label
              >Artwork URL<input
                bind:value={artwork}
                type="url"
                placeholder="https://…"
              /></label
            >
            <label>Title<input bind:value={title} required /></label>
            <label class="wide"
              >Synopsis<textarea bind:value={synopsis} rows="4"
              ></textarea></label
            >
            <label
              >Year<input
                bind:value={year}
                type="number"
                min="1870"
                max="3000"
              /></label
            >
            <label
              >Rating<input
                bind:value={rating}
                type="number"
                min="0"
                max="10"
                step="0.1"
              /></label
            >
            <label
              >Genres<input
                bind:value={genres}
                placeholder="Drama, Crime"
              /></label
            >
            <label
              >Language<input
                bind:value={language}
                placeholder="Portuguese"
              /></label
            >
          </div>
          <footer>
            <Button onclick={onclose}>Cancel</Button><button
              type="button"
              class="save"
              disabled={!title.trim() || saving}
              onclick={() => void saveManual()}
              >{saving ? 'Saving…' : 'Save'}</button
            >
          </footer>
        </form>
      {/if}
      {#if error}<p class="error" role="alert">{error}</p>{/if}
    </div>
  {/if}
</dialog>

<style>
  dialog {
    width: min(94vw, 760px);
    max-height: min(88vh, 820px);
    padding: 0;
    overflow: hidden;
    border: 1px solid var(--color-stroke);
    border-radius: var(--radius-lg);
    color: var(--color-fg);
    background: var(--color-surface);
    box-shadow: var(--shadow-float);
  }
  dialog::backdrop {
    background: rgba(0, 0, 0, 0.76);
    backdrop-filter: blur(10px);
  }
  .shell {
    display: grid;
    gap: var(--space-4);
    padding: var(--space-5);
  }
  header,
  footer,
  .file-context,
  nav,
  .search label span {
    display: flex;
    align-items: center;
  }
  header,
  footer {
    justify-content: space-between;
    gap: var(--space-3);
  }
  h2,
  p {
    margin: 0;
  }
  .file-context {
    gap: var(--space-3);
    min-width: 0;
  }
  .file-context > div:last-child {
    min-width: 0;
    display: grid;
    gap: 5px;
  }
  .file-context strong {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .poster,
  .result-poster {
    flex: 0 0 auto;
    overflow: hidden;
    display: grid;
    place-items: center;
    color: var(--color-fg-subtle);
    background: var(--color-canvas);
  }
  .poster {
    width: 62px;
    aspect-ratio: 2 / 3;
    border-radius: var(--radius-sm);
  }
  .poster img,
  .result-poster img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .status {
    width: max-content;
    padding: 3px 8px;
    border-radius: var(--radius-round);
    color: var(--color-danger);
    background: color-mix(in srgb, var(--color-danger) 16%, transparent);
    font-size: var(--text-xs);
  }
  nav {
    gap: var(--space-1);
    padding: 4px;
    border-radius: var(--radius-md);
    background: var(--color-canvas);
  }
  nav button {
    flex: 1;
    min-height: 38px;
    border: 0;
    border-radius: 10px;
    color: var(--color-fg-muted);
    background: transparent;
    cursor: pointer;
  }
  nav button.active {
    color: var(--color-fg);
    background: var(--color-control-hover);
    font-weight: 650;
  }
  label {
    display: grid;
    gap: 7px;
    color: var(--color-fg-muted);
    font-size: var(--text-sm);
  }
  input,
  textarea {
    width: 100%;
    padding: 10px 12px;
    border: 1px solid var(--color-stroke);
    border-radius: var(--radius-sm);
    outline: 0;
    color: var(--color-fg);
    background: var(--color-canvas);
    resize: vertical;
  }
  .search label span {
    gap: var(--space-2);
  }
  .results {
    max-height: min(42vh, 390px);
    display: grid;
    gap: var(--space-2);
    overflow-y: auto;
    margin-top: var(--space-3);
    padding-right: 4px;
  }
  .result {
    width: 100%;
    min-height: 112px;
    display: flex;
    gap: var(--space-3);
    padding: 10px;
    border: 1px solid var(--color-stroke);
    border-radius: var(--radius-md);
    color: inherit;
    background: var(--color-canvas);
    cursor: pointer;
    text-align: left;
  }
  .result:hover {
    border-color: var(--color-stroke-strong);
    background: var(--color-control-hover);
  }
  .result-poster {
    width: 64px;
    aspect-ratio: 2 / 3;
    border-radius: 7px;
  }
  .result-copy {
    min-width: 0;
    display: grid;
    align-content: center;
    gap: 7px;
  }
  .result-copy small {
    display: -webkit-box;
    overflow: hidden;
    color: var(--color-fg-muted);
    line-height: 1.4;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 3;
    line-clamp: 3;
  }
  .manual,
  .grid {
    display: grid;
    gap: var(--space-3);
  }
  .grid {
    grid-template-columns: 1fr 1fr;
  }
  .wide {
    grid-column: 1 / -1;
  }
  .manual footer {
    margin-top: var(--space-2);
  }
  .save {
    min-height: var(--control-height);
    padding: 0 var(--space-4);
    border: 1px solid var(--color-fg);
    border-radius: var(--radius-md);
    color: var(--color-canvas);
    background: var(--color-fg);
    font-weight: 700;
    cursor: pointer;
  }
  .save:disabled {
    opacity: var(--opacity-disabled);
    cursor: default;
  }
  .error {
    color: var(--color-danger);
    font-size: var(--text-sm);
  }
  @media (max-width: 620px) {
    .shell {
      padding: var(--space-4);
    }
    .grid {
      grid-template-columns: 1fr;
    }
    .wide {
      grid-column: auto;
    }
  }
</style>
