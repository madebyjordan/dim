<script lang="ts">
  import { onMount } from 'svelte';
  import type {
    PlaybackCapabilityInspection,
    PlaybackSession,
    PlaybackTrack
  } from '$lib/api/generated';
  import { session } from '$lib/auth/session.svelte';
  import Button from '$lib/primitives/Button.svelte';
  import Select from '$lib/primitives/Select.svelte';
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
    <Select
      label="Video quality"
      value={selectedVideo}
      options={tracks('video').map((track) => ({
        value: track.id,
        label: track.label || String(track.height || track.id)
      }))}
      onvaluechange={(value) => switchTrack('video', value)}
      disabled={phase !== 'ready'}
    />
    <Select
      label="Audio track"
      value={selectedAudio}
      options={tracks('audio').map((track) => ({
        value: track.id,
        label: track.label || track.lang || track.id
      }))}
      onvaluechange={(value) => switchTrack('audio', value)}
      disabled={phase !== 'ready'}
    />
    <Select
      label="Subtitle track"
      value={selectedSubtitle}
      options={[
        { value: '', label: 'No Subtitles' },
        ...tracks('subtitle').map((track) => ({
          value: track.id,
          label: track.label || track.lang || track.id
        }))
      ]}
      onvaluechange={switchSubtitle}
      disabled={phase !== 'ready'}
    />
    <output
      class="visually-hidden"
      bind:this={timeOutput}
      aria-label="Elapsed time">0s</output
    >
    <Button
      tone="surface"
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
              : 'AirPlay unavailable'}</Button
    >
  </div>
  {#if error && phase !== 'error'}<p class="notice">{error}</p>{/if}
</section>

<style>
  .proof {
    min-height: 100vh;
    padding: var(--space-6);
    background: var(--color-canvas);
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
    border-radius: var(--radius-round);
    color: var(--color-fg);
    background: var(--color-surface);
    font-size: 1.25rem;
  }
  .stage {
    position: relative;
    max-width: 1100px;
    aspect-ratio: 16/9;
    margin: auto;
    overflow: hidden;
    border: 1px solid var(--color-stroke);
    border-radius: var(--radius-lg);
    background: #000;
    box-shadow: var(--shadow-float);
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
    color: var(--color-fg-muted);
    background: rgba(0, 0, 0, 0.45);
  }
  .error,
  .notice {
    color: var(--color-danger);
  }
  .controls {
    display: grid;
    grid-template-columns: repeat(3, minmax(130px, 1fr)) auto;
    gap: 0.8rem;
    align-items: end;
    max-width: 1100px;
    margin: 1rem auto;
  }
  .notice {
    max-width: 1100px;
    margin: 0.5rem auto;
    color: var(--color-fg-muted);
    font-size: var(--text-xs);
  }
  .notice {
    color: var(--color-danger);
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
    .proof {
      padding: var(--space-4);
    }
    .controls {
      grid-template-columns: 1fr 1fr;
    }
  }
</style>
