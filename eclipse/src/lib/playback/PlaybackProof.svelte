<script lang="ts">
  import { onMount } from 'svelte';
  import type {
    PlaybackCapabilityInspection,
    PlaybackSession,
    PlaybackTrack
  } from '$lib/api/generated';
  import { session } from '$lib/auth/session.svelte';
  import {
    determineCapabilities,
    type BrowserCapabilities
  } from './capabilities';

  interface WebKitAirPlayVideo extends HTMLVideoElement {
    webkitShowPlaybackTargetPicker?: () => void;
    webkitCurrentPlaybackTargetIsWireless?: boolean;
  }

  let {
    fileId,
    initialVideo = '',
    initialAudio = '',
    initialSubtitle = ''
  }: {
    fileId: string;
    initialVideo?: string;
    initialAudio?: string;
    initialSubtitle?: string;
  } = $props();
  let video: HTMLVideoElement;
  let timeOutput: HTMLOutputElement;
  let dashPlayer: { destroy(): void; attachSource(url: string): void } | null =
    null;
  let playbackSession = $state<PlaybackSession | null>(null);
  let phase = $state<'loading' | 'ready' | 'error'>('loading');
  let error = $state<string | null>(null);
  let selectedVideo = $state('');
  let selectedAudio = $state('');
  let selectedSubtitle = $state('');
  let capabilities: BrowserCapabilities | null = null;
  let subtitleCleanup: (() => void) | null = null;
  let remoteSessionId: string | null = null;
  let remoteVideo: WebKitAirPlayVideo | null = null;
  let airPlayState = $state<
    'unavailable' | 'available' | 'preparing' | 'ready' | 'active'
  >('unavailable');

  const playbackKey = 'eclipse.playback-session';
  const tracks = (kind: PlaybackTrack['content_type']) =>
    playbackSession?.tracks.filter((track) => track.content_type === kind) ??
    [];
  const preferred = (kind: PlaybackTrack['content_type']) =>
    tracks(kind).find((track) => track.is_default) ?? tracks(kind)[0];

  function manifestUrl(replaceVideo = false) {
    if (!playbackSession) return '';
    const includes = [selectedVideo, selectedAudio].filter(Boolean).join(',');
    const query = new URLSearchParams({ includes });
    if (replaceVideo) query.set('replace_video', 'true');
    return new URL(
      `/api/v1/stream/${playbackSession.gid}/manifest.mpd?${query}`,
      window.location.origin
    ).href;
  }

  async function createSession(target: 'browser' | 'airplay' = 'browser') {
    const inspection = await session.api.get<PlaybackCapabilityInspection>(
      `stream/${fileId}/capabilities`
    );
    capabilities ??= await determineCapabilities(inspection);
    return session.api.get<PlaybackSession>(`stream/${fileId}/manifest`, {
      force_ass: true,
      capabilities: JSON.stringify(capabilities),
      target
    });
  }

  async function activate() {
    const dash = await import('dashjs');
    const player = dash.MediaPlayer().create();
    player.updateSettings({
      debug: { logLevel: dash.Debug.LOG_LEVEL_WARNING },
      streaming: { abr: { autoSwitchBitrate: { video: false } } }
    });
    player.extend(
      'RequestModifier',
      () => ({
        modifyRequestHeader(xhr: XMLHttpRequest) {
          if (session.token)
            xhr.setRequestHeader('Authorization', session.token);
          return xhr;
        },
        modifyRequestURL(url: string) {
          return url;
        }
      }),
      true
    );
    player.initialize(video, manifestUrl(), false);
    dashPlayer = player;
  }

  async function switchTrack(kind: 'video' | 'audio', id: string) {
    if (kind === 'video') selectedVideo = id;
    else selectedAudio = id;
    if (!dashPlayer) return;
    const url = manifestUrl(kind === 'video');
    const response = await fetch(url, {
      credentials: 'same-origin',
      headers: session.token ? { Authorization: session.token } : undefined
    });
    if (!response.ok) {
      error = `Track activation failed (${response.status}); current playback was kept.`;
      return;
    }
    const position = video.currentTime;
    const wasPaused = video.paused;
    video.addEventListener(
      'loadedmetadata',
      () => {
        video.currentTime = position;
        if (!wasPaused) void video.play();
      },
      { once: true }
    );
    dashPlayer.attachSource(url);
  }

  async function switchSubtitle(id: string) {
    selectedSubtitle = id;
    subtitleCleanup?.();
    subtitleCleanup = null;
    const track = tracks('subtitle').find((candidate) => candidate.id === id);
    if (!track?.chunk_path) return;
    try {
      const response = await fetch(`/api/v1/stream/${track.chunk_path}`, {
        credentials: 'same-origin',
        headers: session.token ? { Authorization: session.token } : undefined
      });
      if (!response.ok)
        throw new Error(`Subtitle request failed (${response.status})`);
      const content = await response.text();
      if (track.chunk_path.endsWith('.ass')) {
        const [{ default: JASSUB }, worker, wasm, modernWasm, font] =
          await Promise.all([
            import('jassub'),
            import('jassub/dist/jassub-worker.js?url'),
            import('jassub/dist/jassub-worker.wasm?url'),
            import('jassub/dist/jassub-worker-modern.wasm?url'),
            import('jassub/dist/default.woff2?url')
          ]);
        const renderer = new JASSUB({
          video,
          subContent: content,
          workerUrl: worker.default,
          wasmUrl: wasm.default,
          modernWasmUrl: modernWasm.default,
          availableFonts: { 'liberation sans': font.default },
          fonts: [font.default]
        });
        subtitleCleanup = () => renderer.destroy();
      } else {
        const blobUrl = URL.createObjectURL(
          new Blob([content], { type: 'text/vtt' })
        );
        const element = document.createElement('track');
        element.kind = 'subtitles';
        element.src = blobUrl;
        element.default = true;
        video.append(element);
        element.track.mode = 'showing';
        subtitleCleanup = () => {
          element.remove();
          URL.revokeObjectURL(blobUrl);
        };
      }
    } catch (cause) {
      error = cause instanceof Error ? cause.message : 'Subtitle setup failed';
    }
  }

  async function prepareAirPlay() {
    if (
      airPlayState === 'ready' &&
      remoteVideo?.webkitShowPlaybackTargetPicker
    ) {
      remoteVideo.webkitShowPlaybackTargetPicker();
      return;
    }
    airPlayState = 'preparing';
    try {
      const remote = await createSession('airplay');
      if (!remote.remote)
        throw new Error('The backend did not provide an AirPlay resource.');
      remoteSessionId = remote.gid;
      const element = document.createElement('video') as WebKitAirPlayVideo;
      element.src = new URL(remote.remote.url, window.location.origin).href;
      element.preload = 'auto';
      element.addEventListener(
        'webkitcurrentplaybacktargetiswirelesschanged',
        () => {
          airPlayState = element.webkitCurrentPlaybackTargetIsWireless
            ? 'active'
            : 'ready';
        }
      );
      remoteVideo = element;
      airPlayState = 'ready';
    } catch (cause) {
      airPlayState = 'available';
      error =
        cause instanceof Error ? cause.message : 'AirPlay preparation failed';
    }
  }

  async function cleanup() {
    subtitleCleanup?.();
    subtitleCleanup = null;
    dashPlayer?.destroy();
    dashPlayer = null;
    remoteVideo?.pause();
    remoteVideo?.removeAttribute('src');
    remoteVideo?.load();
    remoteVideo = null;
    const ids = [playbackSession?.gid, remoteSessionId].filter(
      Boolean
    ) as string[];
    playbackSession = null;
    remoteSessionId = null;
    sessionStorage.removeItem(playbackKey);
    await Promise.allSettled(
      ids.map((gid) => session.api.delete(`stream/${gid}/state/kill`))
    );
  }

  onMount(() => {
    airPlayState =
      'webkitShowPlaybackTargetPicker' in document.createElement('video')
        ? 'available'
        : 'unavailable';
    let disposed = false;
    const onTime = () => {
      // Deliberately non-reactive: timeupdate writes only to this local output node.
      if (timeOutput) timeOutput.value = `${Math.floor(video.currentTime)}s`;
    };
    video.addEventListener('timeupdate', onTime);
    (async () => {
      try {
        const stale = sessionStorage.getItem(playbackKey);
        if (stale)
          await session.api
            .delete(`stream/${stale}/state/kill`)
            .catch(() => undefined);
        const created = await createSession();
        if (disposed) {
          await session.api
            .delete(`stream/${created.gid}/state/kill`)
            .catch(() => undefined);
          return;
        }
        playbackSession = created;
        sessionStorage.setItem(playbackKey, created.gid);
        selectedVideo =
          tracks('video').find((track) => track.id === initialVideo)?.id ??
          preferred('video')?.id ??
          '';
        selectedAudio =
          tracks('audio').find((track) => track.id === initialAudio)?.id ??
          preferred('audio')?.id ??
          '';
        await activate();
        if (
          initialSubtitle &&
          tracks('subtitle').some((track) => track.id === initialSubtitle)
        ) {
          await switchSubtitle(initialSubtitle);
        }
        phase = 'ready';
      } catch (cause) {
        error =
          cause instanceof Error
            ? cause.message
            : 'Playback initialization failed';
        phase = 'error';
      }
    })();
    return () => {
      disposed = true;
      video.removeEventListener('timeupdate', onTime);
      void cleanup();
    };
  });
</script>

<section class="proof" aria-label="Playback">
  <header>
    <a href="/" aria-label="Return to library">←</a>
  </header>

  <div class="stage">
    <video bind:this={video} controls playsinline></video>
    {#if phase === 'loading'}<p class="overlay">
        Preparing authenticated stream…
      </p>{/if}
    {#if phase === 'error'}<p class="overlay error">{error}</p>{/if}
  </div>

  <div class="controls">
    <label aria-label="Video quality">
      <span class="visually-hidden">Video quality</span>
      <select
        value={selectedVideo}
        onchange={(event) => switchTrack('video', event.currentTarget.value)}
        disabled={phase !== 'ready'}
      >
        {#each tracks('video') as track}<option value={track.id}
            >{track.label || track.height || track.id}</option
          >{/each}
      </select>
    </label>
    <label aria-label="Audio track">
      <span class="visually-hidden">Audio track</span>
      <select
        value={selectedAudio}
        onchange={(event) => switchTrack('audio', event.currentTarget.value)}
        disabled={phase !== 'ready'}
      >
        {#each tracks('audio') as track}<option value={track.id}
            >{track.label || track.lang || track.id}</option
          >{/each}
      </select>
    </label>
    <label aria-label="Subtitle track">
      <span class="visually-hidden">Subtitle track</span>
      <select
        value={selectedSubtitle}
        onchange={(event) => switchSubtitle(event.currentTarget.value)}
        disabled={phase !== 'ready'}
      >
        <option value="">No Subtitles</option>
        {#each tracks('subtitle') as track}<option value={track.id}
            >{track.label || track.lang || track.id}</option
          >{/each}
      </select>
    </label>
    <output
      class="visually-hidden"
      bind:this={timeOutput}
      aria-label="Elapsed time">0s</output
    >
    <button
      onclick={prepareAirPlay}
      disabled={airPlayState === 'unavailable' || airPlayState === 'preparing'}
    >
      {airPlayState === 'ready'
        ? 'Choose AirPlay target'
        : airPlayState === 'active'
          ? 'AirPlay active'
          : airPlayState === 'preparing'
            ? 'Preparing AirPlay…'
            : airPlayState === 'available'
              ? 'Prepare AirPlay'
              : 'AirPlay unavailable'}
    </button>
  </div>
  {#if error && phase !== 'error'}<p class="notice">{error}</p>{/if}
</section>

<style>
  .proof {
    min-height: 100vh;
    padding: clamp(1rem, 2.2vw, 2rem);
    background: var(--surface-canvas);
  }
  header {
    display: flex;
    align-items: center;
    max-width: 1100px;
    margin: 0 auto 1.5rem;
  }
  p {
    margin: 0;
  }
  header a {
    width: 42px;
    aspect-ratio: 1;
    display: grid;
    place-items: center;
    border-radius: 50%;
    color: var(--text);
    background: var(--surface-raised);
    font-size: 1.25rem;
  }
  .stage {
    position: relative;
    max-width: 1100px;
    aspect-ratio: 16/9;
    margin: auto;
    overflow: hidden;
    border: 1px solid var(--border);
    border-radius: 18px;
    background: #000;
    box-shadow: var(--shadow-lg);
  }
  video {
    width: 100%;
    height: 100%;
    display: block;
  }
  .overlay {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    color: var(--text-muted);
    background: rgba(0, 0, 0, 0.45);
  }
  .error,
  .notice {
    color: var(--danger);
  }
  .controls {
    display: grid;
    grid-template-columns: repeat(3, minmax(130px, 1fr)) auto;
    gap: 0.8rem;
    align-items: end;
    max-width: 1100px;
    margin: 1rem auto;
  }
  label {
    display: grid;
  }
  select,
  button {
    min-height: 42px;
    padding: 0 0.7rem;
    border: 1px solid var(--border);
    border-radius: 9px;
    color: var(--text);
    background: var(--surface-raised);
  }
  button {
    cursor: pointer;
  }
  button:disabled {
    cursor: not-allowed;
    opacity: 0.5;
  }
  .notice {
    max-width: 1100px;
    margin: 0.5rem auto;
    color: var(--text-muted);
    font-size: 0.75rem;
  }
  .notice {
    color: var(--danger);
  }
  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
    border: 0;
  }
  @media (max-width: 800px) {
    .controls {
      grid-template-columns: 1fr 1fr;
    }
  }
</style>
