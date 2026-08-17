<script lang="ts">
  import { goto, pushState } from '$app/navigation';
  import { page } from '$app/state';
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
    UpdateLibrary,
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
  import MediaBackdrop from '$lib/components/MediaBackdrop.svelte';
  import MediaCarousel from '$lib/components/MediaCarousel.svelte';
  import MediaPresentation from '$lib/components/MediaPresentation.svelte';
  import { determineCapabilities } from '$lib/playback/capabilities';
  import { realtime } from '$lib/realtime/socket.svelte';

  type PlaybackComponent =
    typeof import('$lib/playback/PlaybackProof.svelte').default;
  type PlaybackHistoryState = {
    playback?: {
      fileId: string;
      video: string;
      audio: string;
      subtitle: string;
    };
  };

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
  let PlaybackSurface = $state<PlaybackComponent | null>(null);
  let playbackActive = $state(false);
  let playbackRevealed = $state(false);
  let playbackWasActive = false;
  let playbackRevealTimer: number | null = null;
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
  const activeLibraryScanning = $derived(
    activeLibraryId !== null && scans[activeLibraryId]?.status === 'scanning'
  );

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

  async function updateLibraryAutoScan(library: Library, enabled: boolean) {
    error = null;
    try {
      const updated = await session.api.patch<Library>(
        `library/${library.id}`,
        { auto_scan: enabled } satisfies UpdateLibrary
      );
      libraries = libraries.map((current) =>
        current.id === updated.id ? updated : current
      );
    } catch (cause) {
      error =
        cause instanceof Error
          ? cause.message
          : 'Auto scan could not be updated';
      throw cause;
    }
  }

  async function scanLibrary(library: Library) {
    error = null;
    try {
      const status = await session.api.post<ScanStatus>(
        `library/${library.id}/scan`
      );
      scans = { ...scans, [library.id]: status };
    } catch (cause) {
      error =
        cause instanceof Error ? cause.message : 'The scan could not start';
      throw cause;
    }
  }

  async function deleteLibrary(library: Library) {
    error = null;
    try {
      await session.api.delete(`library/${library.id}`);
      await refreshLibraries();
    } catch (cause) {
      error =
        cause instanceof Error
          ? cause.message
          : 'The library could not be deleted';
      throw cause;
    }
  }

  async function disposePreparedPlayback(resetTracks = true) {
    const gid = preparedPlayback?.gid;
    preparedPlayback = null;
    if (resetTracks) {
      selectedVideo = '';
      selectedAudio = '';
      selectedSubtitle = '';
    }
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
      selectedVideo = prepared.tracks.some(
        (track) => track.content_type === 'video' && track.id === selectedVideo
      )
        ? selectedVideo
        : (preferredTrack(prepared, 'video')?.id ?? '');
      selectedAudio = prepared.tracks.some(
        (track) => track.content_type === 'audio' && track.id === selectedAudio
      )
        ? selectedAudio
        : (preferredTrack(prepared, 'audio')?.id ?? '');
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

  async function launchPlayback() {
    if (!selectedFile) return;
    const playback = {
      fileId: String(selectedFile.id),
      video: selectedVideo,
      audio: selectedAudio,
      subtitle: selectedSubtitle
    };
    const query = new URLSearchParams();
    if (playback.video) query.set('video', playback.video);
    if (playback.audio) query.set('audio', playback.audio);
    if (playback.subtitle) query.set('subtitle', playback.subtitle);

    playbackActive = true;
    schedulePlaybackReveal();
    pushState(`/play/${selectedFile.id}?${query}`, {
      ...page.state,
      playback
    });
    const componentPromise = import('$lib/playback/PlaybackProof.svelte');
    await disposePreparedPlayback(false);
    try {
      PlaybackSurface = (await componentPromise).default;
    } catch (cause) {
      playbackActive = false;
      playbackError =
        cause instanceof Error ? cause.message : 'The player could not load';
      history.back();
    }
  }

  function exitPlayback() {
    if (!playbackActive) return;
    history.back();
  }

  function schedulePlaybackReveal() {
    playbackRevealed = false;
    if (playbackRevealTimer !== null) {
      window.clearTimeout(playbackRevealTimer);
    }
    // Streaming starts immediately while Eclipse's chrome clears the viewport.
    playbackRevealTimer = window.setTimeout(() => {
      playbackRevealed = true;
      playbackRevealTimer = null;
    }, 210);
  }

  $effect(() => {
    const state = page.state as PlaybackHistoryState;
    const nextActive = Boolean(state.playback);
    if (!playbackWasActive && nextActive) schedulePlaybackReveal();
    if (playbackWasActive && !nextActive) {
      playbackRevealed = false;
      if (playbackRevealTimer !== null) {
        window.clearTimeout(playbackRevealTimer);
        playbackRevealTimer = null;
      }
    }
    if (!playbackWasActive && nextActive && preparedPlayback) {
      void disposePreparedPlayback(false);
    }
    if (playbackWasActive && !nextActive && selectedFile && !preparedPlayback) {
      void preparePlayback(selectedFile, selectionVersion);
    }
    playbackWasActive = nextActive;
    playbackActive = nextActive;
    if (playbackActive && !PlaybackSurface) {
      void import('$lib/playback/PlaybackProof.svelte').then(
        (module) => (PlaybackSurface = module.default)
      );
    }
  });

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
      if (playbackRevealTimer !== null) {
        window.clearTimeout(playbackRevealTimer);
      }
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

<main
  class:watching={playbackActive}
  class="experience"
  aria-hidden={playbackActive}
>
  {#if selectedMedia}
    {#key selectedMedia.id}<MediaBackdrop media={selectedMedia} />{/key}
  {/if}

  <AppHeader
    {libraries}
    {activeLibraryId}
    user={session.user}
    {scanText}
    {activeLibraryScanning}
    onlibraryautoscan={updateLibraryAutoScan}
    onlibraryscan={scanLibrary}
    onlibrarydelete={deleteLibrary}
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

{#if playbackActive}
  <div
    class:ready={PlaybackSurface !== null && playbackRevealed}
    class="playback-layer"
  >
    {#if PlaybackSurface}
      {@const playback = (page.state as PlaybackHistoryState).playback}
      {#if playback}
        <PlaybackSurface
          fileId={playback.fileId}
          initialVideo={playback.video}
          initialAudio={playback.audio}
          initialSubtitle={playback.subtitle}
          autoplay
          onexit={exitPlayback}
        />
      {/if}
    {:else}
      <span class="player-loading" role="status" aria-label="Opening playback"
      ></span>
    {/if}
  </div>
{/if}

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
  .experience.watching {
    pointer-events: none;
  }
  .experience :global(.header),
  .experience :global(.presentation),
  .experience .browse,
  .experience :global(.backdrop) {
    will-change: opacity, transform;
  }
  .experience :global(.header) {
    transition:
      opacity 170ms ease,
      transform 210ms cubic-bezier(0.22, 1, 0.36, 1);
  }
  .experience :global(.presentation) {
    transition:
      opacity 170ms 25ms ease,
      transform 220ms 25ms cubic-bezier(0.22, 1, 0.36, 1);
  }
  .experience .browse {
    transition:
      opacity 160ms 45ms ease,
      transform 230ms 45ms cubic-bezier(0.22, 1, 0.36, 1);
  }
  .experience :global(.backdrop) {
    transition:
      opacity 210ms 120ms ease,
      transform 330ms cubic-bezier(0.22, 1, 0.36, 1);
  }
  .experience.watching :global(.header) {
    opacity: 0;
    transform: translate3d(0, -14px, 0);
  }
  .experience.watching :global(.presentation) {
    opacity: 0;
    transform: translate3d(-10px, 6px, 0);
  }
  .experience.watching .browse {
    opacity: 0;
    transform: translate3d(0, 28px, 0);
  }
  .experience.watching :global(.backdrop) {
    opacity: 0;
    transform: scale(1.018);
  }
  .playback-layer {
    position: fixed;
    z-index: 100;
    inset: 0;
    overflow: hidden;
    background: var(--color-canvas);
    opacity: 0;
    pointer-events: none;
    transition: opacity 240ms ease;
  }
  .playback-layer.ready {
    opacity: 1;
    pointer-events: auto;
  }
  .player-loading {
    position: absolute;
    inset: 50% auto auto 50%;
    width: 24px;
    aspect-ratio: 1;
    border: 2px solid rgba(255, 255, 255, 0.25);
    border-top-color: var(--color-fg);
    border-radius: 50%;
    animation: spin 800ms linear infinite;
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
  @media (prefers-reduced-motion: reduce) {
    .experience :global(.header),
    .experience :global(.presentation),
    .experience .browse,
    .experience :global(.backdrop),
    .playback-layer {
      transition-duration: 0ms;
    }
  }
</style>
