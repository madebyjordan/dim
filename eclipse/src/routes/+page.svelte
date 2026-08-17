<script lang="ts">
  import { goto } from '$app/navigation';
  import { onMount } from 'svelte';
  import type {
    Library,
    Media,
    MediaFile,
    MediaSummary,
    PlaybackCapabilityInspection,
    PlaybackSession,
    ScanStatus,
    SearchResult,
    WebSocketEvent
  } from '$lib/api/generated';
  import { ApiError } from '$lib/api/transport';
  import { session } from '$lib/auth/session.svelte';
  import {
    flattenLibraryMedia,
    fromSearchResult,
    playableFile,
    type CatalogItem
  } from '$lib/catalog/catalog';
  import AddLibraryDialog from '$lib/components/AddLibraryDialog.svelte';
  import AppHeader from '$lib/components/AppHeader.svelte';
  import MediaCarousel from '$lib/components/MediaCarousel.svelte';
  import MediaPresentation from '$lib/components/MediaPresentation.svelte';
  import { determineCapabilities } from '$lib/playback/capabilities';
  import { realtime } from '$lib/realtime/socket.svelte';

  let libraries = $state<Array<Library>>([]);
  let activeLibraryId = $state<number | null>(null);
  let items = $state<Array<CatalogItem>>([]);
  let selectedId = $state<number | null>(null);
  let selectedMedia = $state<Media | null>(null);
  let selectedFile = $state<MediaFile | null>(null);
  let preparedPlayback = $state<PlaybackSession | null>(null);
  let selectedVideo = $state('');
  let selectedAudio = $state('');
  let selectedSubtitle = $state('');
  let scans = $state<Record<number, ScanStatus>>({});
  let catalogLoading = $state(true);
  let detailLoading = $state(false);
  let searching = $state(false);
  let addLibraryOpen = $state(false);
  let error = $state<string | null>(null);
  let playbackError = $state<string | null>(null);
  let selectionVersion = 0;
  let searchVersion = 0;

  const scanText = $derived.by(() => {
    const active = libraries.filter(
      (library) => scans[library.id]?.status === 'scanning'
    );
    if (active.length === 0) return null;
    const library = active[0];
    const scan = scans[library.id];
    const progress =
      scan.discovered && scan.discovered > 0
        ? ` ${scan.processed ?? 0}/${scan.discovered}`
        : '';
    const remaining = active.length > 1 ? ` +${active.length - 1}` : '';
    return `Scanning ${library.name}${progress}${remaining}`;
  });

  function isMissing(cause: unknown) {
    return cause instanceof ApiError && cause.status === 404;
  }

  async function loadLibraryMedia(libraryId: number) {
    catalogLoading = true;
    error = null;
    try {
      const groups = await session.api.get<Record<string, Array<MediaSummary>>>(
        `library/${libraryId}/media`
      );
      if (activeLibraryId === libraryId && !searching) {
        items = flattenLibraryMedia(groups);
      }
    } catch (cause) {
      if (activeLibraryId === libraryId && !searching) {
        if (isMissing(cause)) items = [];
        else
          error =
            cause instanceof Error ? cause.message : 'Media could not load';
      }
    } finally {
      if (activeLibraryId === libraryId) catalogLoading = false;
    }
  }

  async function loadScan(libraryId: number) {
    try {
      const previous = scans[libraryId];
      const status = await session.api.get<ScanStatus>(
        `library/${libraryId}/scan`
      );
      scans = { ...scans, [libraryId]: status };
      if (
        previous?.status === 'scanning' &&
        status.status === 'complete' &&
        activeLibraryId === libraryId &&
        !searching
      ) {
        await loadLibraryMedia(libraryId);
      }
    } catch {
      // Scanner status is supplementary and must not block browsing.
    }
  }

  async function refreshLibraries(preferredId?: number) {
    const loaded = await session.api.get<Array<Library>>('library');
    libraries = loaded;
    void Promise.all(loaded.map((library) => loadScan(library.id)));
    const next =
      loaded.find((library) => library.id === preferredId) ??
      loaded.find((library) => library.id === activeLibraryId) ??
      loaded[0];
    if (!next) {
      activeLibraryId = null;
      items = [];
      catalogLoading = false;
      clearSelection();
      return;
    }
    if (activeLibraryId !== next.id || preferredId !== undefined) {
      await chooseLibrary(next);
    }
  }

  function clearSelection() {
    selectionVersion += 1;
    selectedId = null;
    selectedMedia = null;
    selectedFile = null;
    detailLoading = false;
    playbackError = null;
    void disposePreparedPlayback();
  }

  async function chooseLibrary(library: Library) {
    searching = false;
    activeLibraryId = library.id;
    items = [];
    clearSelection();
    await loadLibraryMedia(library.id);
  }

  async function disposePreparedPlayback() {
    const gid = preparedPlayback?.gid;
    preparedPlayback = null;
    selectedVideo = '';
    selectedAudio = '';
    selectedSubtitle = '';
    if (gid) {
      await session.api
        .delete(`stream/${gid}/state/kill`)
        .catch(() => undefined);
    }
  }

  function preferredTrack(playback: PlaybackSession, type: 'video' | 'audio') {
    const tracks = playback.tracks.filter(
      (track) => track.content_type === type
    );
    return tracks.find((track) => track.is_default) ?? tracks[0];
  }

  async function preparePlayback(file: MediaFile, version: number) {
    try {
      const inspection = await session.api.get<PlaybackCapabilityInspection>(
        `stream/${file.id}/capabilities`
      );
      const capabilities = await determineCapabilities(inspection);
      const prepared = await session.api.get<PlaybackSession>(
        `stream/${file.id}/manifest`,
        {
          force_ass: true,
          capabilities: JSON.stringify(capabilities),
          target: 'browser'
        }
      );
      if (version !== selectionVersion) {
        await session.api
          .delete(`stream/${prepared.gid}/state/kill`)
          .catch(() => undefined);
        return;
      }
      preparedPlayback = prepared;
      selectedVideo = preferredTrack(prepared, 'video')?.id ?? '';
      selectedAudio = preferredTrack(prepared, 'audio')?.id ?? '';
    } catch (cause) {
      if (version === selectionVersion) {
        playbackError =
          cause instanceof Error
            ? cause.message
            : 'Playback options are unavailable';
      }
    }
  }

  async function selectItem(item: CatalogItem) {
    if (selectedId === item.id) return;
    const version = ++selectionVersion;
    void disposePreparedPlayback();
    selectedId = item.id;
    selectedMedia = null;
    selectedFile = null;
    playbackError = null;
    detailLoading = true;
    try {
      const [media, files] = await Promise.all([
        session.api.get<Media>(`media/${item.id}`),
        session.api.get<Array<MediaFile>>(`media/${item.id}/files`)
      ]);
      if (version !== selectionVersion) return;
      selectedMedia = media;
      selectedFile = playableFile(media, files);
      detailLoading = false;
      if (selectedFile) void preparePlayback(selectedFile, version);
    } catch (cause) {
      if (version === selectionVersion) {
        detailLoading = false;
        error =
          cause instanceof Error
            ? cause.message
            : 'Media details could not load';
      }
    }
  }

  async function search(query: string) {
    const version = ++searchVersion;
    const normalized = query.trim();
    if (!normalized) {
      searching = false;
      clearSelection();
      if (activeLibraryId !== null) await loadLibraryMedia(activeLibraryId);
      return;
    }
    searching = true;
    catalogLoading = true;
    clearSelection();
    try {
      const results = await session.api.get<Array<SearchResult>>('search', {
        query: normalized,
        quick: true
      });
      if (searching && version === searchVersion) {
        items = results.map(fromSearchResult);
      }
    } catch (cause) {
      if (searching && version === searchVersion) {
        if (isMissing(cause)) items = [];
        else
          error =
            cause instanceof Error ? cause.message : 'Search could not load';
      }
    } finally {
      if (version === searchVersion) catalogLoading = false;
    }
  }

  function closeSearch() {
    searchVersion += 1;
    searching = false;
    clearSelection();
    if (activeLibraryId !== null) void loadLibraryMedia(activeLibraryId);
  }

  function launchPlayback() {
    if (!selectedFile) return;
    const query = new URLSearchParams();
    if (selectedVideo) query.set('video', selectedVideo);
    if (selectedAudio) query.set('audio', selectedAudio);
    if (selectedSubtitle) query.set('subtitle', selectedSubtitle);
    void goto(`/play/${selectedFile.id}?${query}`);
  }

  function handleRealtime(event: WebSocketEvent) {
    if (
      event.type === 'EventNewLibrary' ||
      event.type === 'EventRemoveLibrary'
    ) {
      void refreshLibraries();
      return;
    }
    if (
      event.type === 'EventStartedScanning' ||
      event.type === 'EventStoppedScanning' ||
      event.type === 'EventScanFailed' ||
      event.type === 'EventScanCancelled'
    ) {
      void loadScan(event.id);
    }
    if (
      (event.type === 'EventNewCard' || event.type === 'EventRemoveCard') &&
      activeLibraryId !== null &&
      !searching
    ) {
      void loadLibraryMedia(activeLibraryId);
    }
  }

  onMount(() => {
    const unsubscribe = realtime.subscribe(handleRealtime);
    const scanTimer = window.setInterval(() => {
      for (const library of libraries) {
        if (scans[library.id]?.status === 'scanning') {
          void loadScan(library.id);
        }
      }
    }, 2_000);
    void refreshLibraries().catch((cause) => {
      catalogLoading = false;
      error =
        cause instanceof Error ? cause.message : 'Libraries could not load';
    });
    return () => {
      unsubscribe();
      window.clearInterval(scanTimer);
      selectionVersion += 1;
      void disposePreparedPlayback();
    };
  });
</script>

<svelte:head>
  <title>Eclipse</title>
  <meta
    name="description"
    content="Browse and play your personal media library."
  />
</svelte:head>

<main class:selected={selectedMedia !== null} class="experience">
  <AppHeader
    {libraries}
    {activeLibraryId}
    user={session.user}
    {scanText}
    onlibrary={(library) => void chooseLibrary(library)}
    onsearch={(query) => void search(query)}
    onsearchclose={closeSearch}
    onaddlibrary={() => (addLibraryOpen = true)}
    onlogout={() => void session.logout()}
  />

  {#if selectedMedia}
    {#key selectedMedia.id}
      <MediaPresentation
        media={selectedMedia}
        playback={preparedPlayback}
        playbackLoading={detailLoading ||
          (!!selectedFile && !preparedPlayback && !playbackError)}
        {playbackError}
        {selectedVideo}
        {selectedAudio}
        {selectedSubtitle}
        onvideo={(id) => (selectedVideo = id)}
        onaudio={(id) => (selectedAudio = id)}
        onsubtitle={(id) => (selectedSubtitle = id)}
      />
    {/key}
  {/if}

  <div class="floor" aria-hidden="true"></div>
  {#if items.length > 0}
    <div class="browse">
      <MediaCarousel
        {items}
        {selectedId}
        playable={selectedFile !== null}
        onselect={(item) => void selectItem(item)}
        onplay={launchPlayback}
      />
    </div>
  {/if}

  {#if error}
    <button class="error" type="button" onclick={() => (error = null)}
      >{error}</button
    >
  {:else if catalogLoading}
    <span class="loading" role="status" aria-label="Loading library"></span>
  {/if}
</main>

<AddLibraryDialog
  open={addLibraryOpen}
  onclose={() => (addLibraryOpen = false)}
  oncreated={(id) => void refreshLibraries(id)}
/>

<style>
  .experience {
    position: relative;
    min-height: 100svh;
    overflow: hidden;
    isolation: isolate;
    background: var(--color-canvas);
  }
  .floor {
    position: absolute;
    inset: 47% 0 0;
    z-index: 3;
    background: linear-gradient(
      0deg,
      var(--color-canvas) 0 76%,
      transparent 100%
    );
    pointer-events: none;
  }
  .selected .floor {
    inset-block-start: 57%;
  }
  .browse {
    position: absolute;
    inset: auto 0 0;
    z-index: 5;
  }
  .error {
    position: fixed;
    z-index: 30;
    right: 24px;
    bottom: 24px;
    max-width: min(420px, calc(100vw - 48px));
    padding: 12px 16px;
    border: 1px solid rgba(255, 148, 148, 0.38);
    border-radius: 10px;
    color: #ffb1b1;
    background: rgba(28, 14, 14, 0.94);
    text-align: left;
    cursor: pointer;
  }
  .loading {
    position: fixed;
    z-index: 12;
    left: 50%;
    bottom: 42px;
    width: 18px;
    aspect-ratio: 1;
    border: 2px solid var(--color-stroke);
    border-top-color: var(--color-fg-muted);
    border-radius: var(--radius-round);
    animation: spin 800ms linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  @media (max-width: 700px) {
    .experience {
      min-height: 100dvh;
    }
    .floor {
      inset-block-start: 45%;
    }
    .selected .floor {
      inset-block-start: 60%;
    }
  }
</style>
