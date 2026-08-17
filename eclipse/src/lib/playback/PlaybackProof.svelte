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
    initialSubtitle = '',
    autoplay = false,
    onexit = () => undefined
  }: {
    fileId: string;
    initialVideo?: string;
    initialAudio?: string;
    initialSubtitle?: string;
    autoplay?: boolean;
    onexit?: () => void;
  } = $props();
  let surface: HTMLElement;
  let controlsPanel: HTMLElement;
  let video: HTMLVideoElement;
  let timeOutput: HTMLOutputElement;
  let durationOutput: HTMLOutputElement;
  let seekInput: HTMLInputElement;
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
  let controlsVisible = $state(true);
  let paused = $state(true);
  let muted = $state(false);
  let controlsTimer: number | null = null;
  let keyboardInteraction = false;
  const controlsIdleMs = 1_250;

  const playbackKey = 'eclipse.playback-session';
  const tracks = (kind: PlaybackTrack['content_type']) =>
    playbackSession?.tracks.filter((track) => track.content_type === kind) ??
    [];
  const preferred = (kind: PlaybackTrack['content_type']) =>
    tracks(kind).find((track) => track.is_default) ?? tracks(kind)[0];

  function formatTime(seconds: number) {
    if (!Number.isFinite(seconds)) return '0:00';
    const whole = Math.max(0, Math.floor(seconds));
    const hours = Math.floor(whole / 3600);
    const minutes = Math.floor((whole % 3600) / 60);
    const rest = String(whole % 60).padStart(2, '0');
    return hours > 0
      ? `${hours}:${String(minutes).padStart(2, '0')}:${rest}`
      : `${minutes}:${rest}`;
  }

  function showControls(keepOpen = false) {
    controlsVisible = true;
    if (controlsTimer !== null) window.clearTimeout(controlsTimer);
    controlsTimer = null;
    if (!keepOpen) {
      controlsTimer = window.setTimeout(() => {
        const focused = document.activeElement;
        const popoutOpen =
          focused instanceof HTMLElement &&
          focused.closest('[data-popout-surface]') !== null &&
          controlsPanel?.contains(focused);
        if (
          !popoutOpen &&
          (!keyboardInteraction || !controlsPanel?.contains(focused))
        )
          controlsVisible = false;
      }, controlsIdleMs);
    }
  }

  function togglePlayback() {
    if (video.paused) void video.play();
    else video.pause();
    showControls();
  }

  function toggleMute() {
    video.muted = !video.muted;
    muted = video.muted;
    showControls();
  }

  function toggleFullscreen() {
    if (document.fullscreenElement) void document.exitFullscreen();
    else void surface.requestFullscreen();
    showControls(true);
  }

  function handleKeydown(event: KeyboardEvent) {
    keyboardInteraction = true;
    showControls();
    if (
      event.target instanceof HTMLButtonElement ||
      event.target instanceof HTMLInputElement
    )
      return;
    if (event.key === 'Escape') {
      if (!document.fullscreenElement) onexit();
      return;
    }
    if (event.key === ' ' || event.key.toLowerCase() === 'k') {
      event.preventDefault();
      togglePlayback();
    } else if (event.key === 'ArrowLeft') {
      event.preventDefault();
      video.currentTime = Math.max(0, video.currentTime - 10);
    } else if (event.key === 'ArrowRight') {
      event.preventDefault();
      video.currentTime = Math.min(
        video.duration || Infinity,
        video.currentTime + 10
      );
    } else if (event.key.toLowerCase() === 'm') toggleMute();
    else if (event.key.toLowerCase() === 'f') toggleFullscreen();
  }

  function handlePointerDown() {
    keyboardInteraction = false;
    showControls();
  }

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
    player.initialize(video, manifestUrl(), autoplay);
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
      if (timeOutput) timeOutput.value = formatTime(video.currentTime);
      if (seekInput) seekInput.value = String(video.currentTime);
    };
    const onDuration = () => {
      if (durationOutput) durationOutput.value = formatTime(video.duration);
      if (seekInput) seekInput.max = String(video.duration || 0);
    };
    const onPlay = () => {
      paused = false;
      showControls();
    };
    const onPause = () => {
      paused = true;
      showControls();
    };
    video.addEventListener('timeupdate', onTime);
    video.addEventListener('durationchange', onDuration);
    video.addEventListener('play', onPlay);
    video.addEventListener('pause', onPause);
    // Begin the idle countdown on entry; pointer movement is not required to arm it.
    showControls();
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
      video.removeEventListener('durationchange', onDuration);
      video.removeEventListener('play', onPlay);
      video.removeEventListener('pause', onPause);
      if (controlsTimer !== null) window.clearTimeout(controlsTimer);
      void cleanup();
    };
  });
</script>

<svelte:window
  onpointermove={() => showControls()}
  onpointerdown={handlePointerDown}
  onkeydown={handleKeydown}
/>

<section
  class:controls-visible={controlsVisible}
  class="player"
  aria-label="Playback"
  bind:this={surface}
>
  <video bind:this={video} playsinline onclick={togglePlayback}></video>

  {#if phase === 'loading'}
    <p class="status">Preparing authenticated stream…</p>
  {:else if phase === 'error'}
    <div class="status failure">
      <p>{error}</p>
      <Button tone="surface" onclick={onexit}>Return to Eclipse</Button>
    </div>
  {/if}

  <div
    class="controls"
    bind:this={controlsPanel}
    aria-hidden={!controlsVisible}
    onfocusin={() => showControls(keyboardInteraction)}
    onfocusout={() => showControls()}
  >
    <div class="topbar">
      <button
        class="round"
        type="button"
        onclick={onexit}
        aria-label="Exit playback">←</button
      >
    </div>

    <div class="control-deck">
      {#if error && phase !== 'error'}<p class="notice" role="status">
          {error}
        </p>{/if}
      <div class="timeline">
        <input
          bind:this={seekInput}
          aria-label="Seek"
          type="range"
          min="0"
          max="0"
          step="0.1"
          value="0"
          oninput={(event) =>
            (video.currentTime = Number(event.currentTarget.value))}
        />
        <span class="time">
          <output bind:this={timeOutput} aria-label="Current time">0:00</output>
          <span aria-hidden="true">/</span>
          <output bind:this={durationOutput} aria-label="Duration">0:00</output>
        </span>
      </div>

      <div class="control-row">
        <button
          class="round primary"
          type="button"
          onclick={togglePlayback}
          aria-label={paused ? 'Play' : 'Pause'}
        >
          {paused ? '▶' : 'Ⅱ'}
        </button>
        <button
          class="round"
          type="button"
          onclick={toggleMute}
          aria-label={muted ? 'Unmute' : 'Mute'}
        >
          {muted ? '🔇' : '🔊'}
        </button>
        <input
          class="volume"
          aria-label="Volume"
          type="range"
          min="0"
          max="1"
          step="0.05"
          value="1"
          oninput={(event) => {
            video.volume = Number(event.currentTarget.value);
            video.muted = video.volume === 0;
            muted = video.muted;
          }}
        />

        <div class="track-controls">
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
            label="Subtitles"
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
        </div>

        {#if airPlayState !== 'unavailable'}
          <button
            class="text-control"
            type="button"
            onclick={prepareAirPlay}
            disabled={airPlayState === 'preparing'}
          >
            {airPlayState === 'active'
              ? 'AirPlay active'
              : airPlayState === 'preparing'
                ? 'Preparing…'
                : 'AirPlay'}
          </button>
        {/if}
        <button
          class="round"
          type="button"
          onclick={toggleFullscreen}
          aria-label="Toggle fullscreen">⛶</button
        >
      </div>
    </div>
  </div>
</section>

<style>
  .player {
    position: fixed;
    inset: 0;
    overflow: hidden;
    color: var(--color-fg);
    background: var(--color-canvas);
    cursor: none;
  }
  .player.controls-visible {
    cursor: default;
  }
  p {
    margin: 0;
  }
  video {
    width: 100%;
    height: 100%;
    display: block;
    object-fit: contain;
    background: var(--color-canvas);
  }
  .status {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    color: rgba(255, 255, 255, 0.72);
    pointer-events: none;
  }
  .failure {
    align-content: center;
    gap: 18px;
    pointer-events: auto;
  }
  .controls {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    opacity: 0;
    pointer-events: none;
    transition: opacity 180ms ease;
    background: linear-gradient(
      to bottom,
      rgba(0, 0, 0, 0.48),
      transparent 24%,
      transparent 58%,
      rgba(0, 0, 0, 0.82)
    );
  }
  .controls-visible .controls {
    opacity: 1;
    pointer-events: auto;
  }
  .topbar {
    padding: max(20px, env(safe-area-inset-top))
      max(24px, env(safe-area-inset-right));
  }
  .control-deck {
    display: grid;
    gap: 12px;
    padding: 20px max(24px, env(safe-area-inset-right))
      max(22px, env(safe-area-inset-bottom));
  }
  .timeline {
    display: flex;
    align-items: center;
    gap: 14px;
  }
  .timeline input {
    flex: 1;
  }
  .time {
    min-width: 8.5rem;
    display: flex;
    justify-content: flex-end;
    gap: 6px;
    color: rgba(255, 255, 255, 0.76);
    font-variant-numeric: tabular-nums;
    font-size: 13px;
  }
  .control-row {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .track-controls {
    margin-left: auto;
    display: flex;
    gap: 8px;
  }
  .round,
  .text-control {
    height: 42px;
    border: 1px solid var(--color-control-subtle);
    color: var(--color-fg);
    background: rgba(12, 12, 12, 0.58);
    backdrop-filter: blur(var(--blur-control));
    cursor: pointer;
  }
  .round {
    width: 42px;
    display: grid;
    place-items: center;
    border-radius: 50%;
  }
  .round.primary {
    color: var(--color-on-accent);
    background: var(--color-accent);
  }
  .text-control {
    padding: 0 14px;
    border-radius: 10px;
  }
  .volume {
    width: 92px;
  }
  input[type='range'] {
    accent-color: var(--color-accent);
  }
  .notice {
    color: var(--color-danger);
    font-size: var(--text-xs);
  }
  @media (max-width: 800px) {
    .track-controls {
      position: absolute;
      right: 20px;
      bottom: 76px;
    }
    .volume {
      display: none;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .controls {
      transition-duration: 0ms;
    }
  }
</style>
