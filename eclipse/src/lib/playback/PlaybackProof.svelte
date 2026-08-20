<script lang="ts">
  import { onMount, untrack } from 'svelte';
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
  import {
    clearStoredPlayback,
    createPlaybackOwnership,
    creationQuery,
    logPlaybackLifecycle,
    readStoredPlayback,
    storePlayback,
    teardownQuery,
    type PlaybackOwnership
  } from './lifecycle';
  import { createSeekCommitter } from './seek';

  interface WebKitAirPlayVideo extends HTMLVideoElement {
    webkitShowPlaybackTargetPicker?: () => void;
    webkitCurrentPlaybackTargetIsWireless?: boolean;
  }

  interface DashPlayer {
    destroy(): void;
    attachSource(url: string): void;
    on(type: string, listener: (event: unknown) => void): void;
    off(type: string, listener: (event: unknown) => void): void;
  }

  interface SeekTrace {
    id: string;
    reason: string;
    from: number;
    target: number;
    startedAt: number;
    inputCount?: number;
  }

  let {
    fileId,
    initialVideo = '',
    initialAudio = '',
    initialSubtitle = '',
    initialSession = null,
    initialOwnership = null,
    autoplay = false,
    onexit = () => undefined
  }: {
    fileId: string;
    initialVideo?: string;
    initialAudio?: string;
    initialSubtitle?: string;
    initialSession?: PlaybackSession | null;
    initialOwnership?: PlaybackOwnership | null;
    autoplay?: boolean;
    onexit?: () => void;
  } = $props();
  const ownership = untrack(
    () => initialOwnership ?? createPlaybackOwnership(fileId, 1)
  );
  let surface: HTMLElement;
  let controlsPanel: HTMLElement;
  let video: HTMLVideoElement;
  let timeOutput: HTMLOutputElement;
  let durationOutput: HTMLOutputElement;
  let seekInput: HTMLInputElement;
  let dashPlayer: DashPlayer | null = null;
  let dashErrorCleanup: (() => void) | null = null;
  let dashTelemetryCleanup: (() => void) | null = null;
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
  let currentSourceGeneration = 0;
  let seekSequence = 0;
  let activeSeek: SeekTrace | null = null;
  let initialPlaybackComplete = false;
  let unmountReason = 'component-unmounted';
  let unmountCaller = 'PlaybackProof.onMount.cleanup';
  const controlsIdleMs = 1_250;

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
      if (!document.fullscreenElement) requestExit('escape-key');
      return;
    }
    if (event.key === ' ' || event.key.toLowerCase() === 'k') {
      event.preventDefault();
      togglePlayback();
    } else if (event.key === 'ArrowLeft') {
      event.preventDefault();
      beginSeek(Math.max(0, video.currentTime - 10), 'keyboard-arrow-left');
    } else if (event.key === 'ArrowRight') {
      event.preventDefault();
      beginSeek(
        Math.min(video.duration || Infinity, video.currentTime + 10),
        'keyboard-arrow-right'
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

  function finite(value: number) {
    return Number.isFinite(value) ? value : null;
  }

  function bufferedRanges() {
    if (!video) return [];
    return Array.from({ length: video.buffered.length }, (_, index) => [
      video.buffered.start(index),
      video.buffered.end(index)
    ]);
  }

  function reportPlayerEvent(
    event: string,
    detail?: string,
    seekTrace: SeekTrace | null = activeSeek
  ) {
    const gid = playbackSession?.gid;
    if (!gid || !video) return;
    const mediaError = video.error;
    void fetch(`/api/v1/stream/${gid}/state/player-event`, {
      method: 'POST',
      credentials: 'same-origin',
      keepalive: true,
      headers: {
        'Content-Type': 'application/json',
        ...(session.token ? { Authorization: session.token } : {})
      },
      body: JSON.stringify({
        event,
        frontend_instance_id: ownership.instanceId,
        media_file_id: fileId,
        source_generation: currentSourceGeneration,
        current_time: finite(video.currentTime),
        duration: finite(video.duration),
        ready_state: video.readyState,
        network_state: video.networkState,
        paused: video.paused,
        ended: video.ended,
        buffered: bufferedRanges(),
        error_code: mediaError?.code,
        error_message: mediaError?.message,
        detail,
        seek_id: seekTrace?.id,
        seek_reason: seekTrace?.reason,
        seek_from: seekTrace ? finite(seekTrace.from) : undefined,
        seek_target: seekTrace ? finite(seekTrace.target) : undefined,
        seek_elapsed_ms: seekTrace
          ? Math.round(performance.now() - seekTrace.startedAt)
          : undefined
      })
    }).catch(() => undefined);
  }

  function beginSeek(
    target: number,
    reason: string,
    from = video.currentTime,
    inputCount?: number
  ) {
    const trace: SeekTrace = {
      id: `${ownership.instanceId}:${++seekSequence}`,
      reason,
      from,
      target,
      startedAt: performance.now(),
      inputCount
    };
    activeSeek = trace;
    reportPlayerEvent(
      'seek-committed',
      inputCount === undefined
        ? undefined
        : JSON.stringify({ scrub_input_count: inputCount }),
      trace
    );
    video.currentTime = target;
  }

  const timelineSeek = createSeekCommitter(
    () => video.currentTime,
    (target) => {
      if (timeOutput) timeOutput.value = formatTime(target);
    },
    ({ from, target, inputCount }) =>
      beginSeek(target, 'timeline-commit', from, inputCount)
  );

  function describeDashError(event: unknown) {
    if (!event || typeof event !== 'object') return String(event);
    const value = event as Record<string, unknown>;
    return [value.type, value.event, value.error, value.message]
      .filter((part) => part !== undefined)
      .map(String)
      .join(' | ');
  }

  function describeDashFragment(event: unknown) {
    if (!event || typeof event !== 'object') return String(event);
    const value = event as Record<string, unknown>;
    const request =
      value.request && typeof value.request === 'object'
        ? (value.request as Record<string, unknown>)
        : {};
    const response = value.response;
    return JSON.stringify({
      media_type: request.mediaType,
      request_type: request.type,
      index: request.index,
      start_time: request.startTime,
      duration: request.duration,
      url: request.url,
      response_bytes:
        response instanceof ArrayBuffer ? response.byteLength : undefined
    });
  }

  async function createSession(target: 'browser' | 'airplay' = 'browser') {
    logPlaybackLifecycle('preparation-state', ownership, {
      state: 'capability-inspection-requested',
      target,
      pendingPromise: 'GET /capabilities'
    });
    const inspection = await session.api.get<PlaybackCapabilityInspection>(
      `stream/${fileId}/capabilities`
    );
    logPlaybackLifecycle('preparation-state', ownership, {
      state: 'capability-inspection-completed',
      target,
      probeSource: inspection.probe_source,
      audioStreamCount: inspection.audio.length
    });
    capabilities ??= await determineCapabilities(inspection, {
      onEvent: (probe) =>
        logPlaybackLifecycle(`capability-probe-${probe.phase}`, ownership, {
          ...probe,
          target,
          pendingPromise:
            probe.phase === 'start'
              ? 'navigator.mediaCapabilities.decodingInfo'
              : null
        })
    });
    logPlaybackLifecycle('preparation-state', ownership, {
      state: 'browser-capability-probe-completed',
      target,
      capabilities
    });
    logPlaybackLifecycle('preparation-state', ownership, {
      state: 'planner-requested',
      target,
      pendingPromise: 'GET /manifest'
    });
    const created = await session.api.get<PlaybackSession>(
      `stream/${fileId}/manifest`,
      {
        force_ass: true,
        capabilities: JSON.stringify(capabilities),
        target,
        ...creationQuery(
          ownership,
          target === 'airplay' ? 'airplay-preparation' : 'player-initialization'
        )
      }
    );
    logPlaybackLifecycle('preparation-state', ownership, {
      state: 'planner-completed',
      target,
      sessionId: created.gid,
      playbackPlan: created.playback_plan
    });
    return created;
  }

  function requestExit(caller: string) {
    unmountReason = 'normal-player-exit';
    unmountCaller = caller;
    logPlaybackLifecycle('player-exit-requested', ownership, {
      caller,
      sessionId: playbackSession?.gid,
      sourceGeneration: currentSourceGeneration
    });
    onexit();
  }

  async function teardownSession(
    gid: string,
    reason: string,
    caller: string,
    sessionOwnership = ownership
  ) {
    logPlaybackLifecycle('session-teardown-requested', sessionOwnership, {
      sessionId: gid,
      reason,
      caller
    });
    await session.api.delete(
      `stream/${gid}/state/kill`,
      sessionOwnership.instanceId
        ? teardownQuery(sessionOwnership, reason, caller)
        : { teardown_reason: reason, teardown_caller: caller }
    );
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
    const source = manifestUrl();
    const sourceGeneration = ++currentSourceGeneration;
    logPlaybackLifecycle('source-attached', ownership, {
      sessionId: playbackSession?.gid,
      source,
      autoplay,
      sourceGeneration
    });
    reportPlayerEvent('source-attached', source);
    const onDashError = (event: unknown) =>
      reportPlayerEvent('dash-error', describeDashError(event));
    player.on(dash.MediaPlayer.events.ERROR, onDashError);
    dashErrorCleanup = () =>
      player.off(dash.MediaPlayer.events.ERROR, onDashError);
    const fragmentEvents = [
      dash.MediaPlayer.events.FRAGMENT_LOADING_STARTED,
      dash.MediaPlayer.events.FRAGMENT_LOADING_COMPLETED
    ];
    const onDashFragment = (event: unknown) => {
      if (initialPlaybackComplete && !activeSeek) return;
      const eventType =
        event && typeof event === 'object' && 'type' in event
          ? String((event as { type: unknown }).type)
          : 'fragment';
      reportPlayerEvent(`dash-${eventType}`, describeDashFragment(event));
    };
    for (const event of fragmentEvents) player.on(event, onDashFragment);
    dashTelemetryCleanup = () => {
      for (const event of fragmentEvents) player.off(event, onDashFragment);
    };
    player.initialize(video, source, autoplay);
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
    logPlaybackLifecycle('source-reassigned', ownership, {
      sessionId: playbackSession?.gid,
      trackKind: kind,
      trackId: id,
      sourceGeneration: ++currentSourceGeneration
    });
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

  async function cleanup(
    reason = 'component-unmounted',
    caller = 'PlaybackProof.onMount.cleanup'
  ) {
    subtitleCleanup?.();
    subtitleCleanup = null;
    dashErrorCleanup?.();
    dashErrorCleanup = null;
    dashTelemetryCleanup?.();
    dashTelemetryCleanup = null;
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
    for (const gid of ids) clearStoredPlayback(gid);
    await Promise.allSettled(
      ids.map((gid) => teardownSession(gid, reason, caller))
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
      if (seekInput && !timelineSeek.isPreviewing())
        seekInput.value = String(video.currentTime);
    };
    const onDuration = () => {
      if (durationOutput) durationOutput.value = formatTime(video.duration);
      if (seekInput) seekInput.max = String(video.duration || 0);
    };
    const onPlay = () => {
      paused = false;
      showControls();
      reportPlayerEvent('play');
    };
    const onPause = () => {
      paused = true;
      showControls();
      reportPlayerEvent('pause');
    };
    const onPlaying = () => {
      reportPlayerEvent('playing');
      initialPlaybackComplete = true;
      activeSeek = null;
    };
    const diagnosticEvents = [
      'abort',
      'canplay',
      'emptied',
      'ended',
      'error',
      'loadeddata',
      'loadedmetadata',
      'seeked',
      'seeking',
      'stalled',
      'suspend',
      'waiting'
    ] as const;
    const onDiagnosticEvent = (event: Event) => {
      if (event.type === 'seeking' && !activeSeek) {
        activeSeek = {
          id: `${ownership.instanceId}:${++seekSequence}`,
          reason: 'media-element-or-dash',
          from: video.currentTime,
          target: video.currentTime,
          startedAt: performance.now()
        };
      }
      reportPlayerEvent(event.type);
    };
    video.addEventListener('timeupdate', onTime);
    video.addEventListener('durationchange', onDuration);
    video.addEventListener('play', onPlay);
    video.addEventListener('pause', onPause);
    video.addEventListener('playing', onPlaying);
    for (const event of diagnosticEvents)
      video.addEventListener(event, onDiagnosticEvent);
    // Begin the idle countdown on entry; pointer movement is not required to arm it.
    showControls();
    (async () => {
      try {
        const stale = readStoredPlayback();
        if (stale && stale.gid !== initialSession?.gid) {
          await teardownSession(
            stale.gid,
            'stale-session-recovery',
            'PlaybackProof.onMount',
            stale
          ).catch(() => undefined);
        }
        const created = initialSession ?? (await createSession());
        logPlaybackLifecycle(
          initialSession ? 'prepared-session-adopted' : 'session-created',
          ownership,
          { sessionId: created.gid }
        );
        if (disposed) {
          await teardownSession(
            created.gid,
            'initialization-completed-after-dispose',
            'PlaybackProof.onMount.initialize'
          ).catch(() => undefined);
          return;
        }
        playbackSession = created;
        storePlayback(created.gid, ownership);
        selectedVideo =
          tracks('video').find((track) => track.id === initialVideo)?.id ??
          preferred('video')?.id ??
          '';
        selectedAudio =
          tracks('audio').find((track) => track.id === initialAudio)?.id ??
          preferred('audio')?.id ??
          '';
        await activate();
        logPlaybackLifecycle('preparation-state', ownership, {
          state: 'source-assigned',
          sessionId: created.gid,
          sourceGeneration: currentSourceGeneration
        });
        if (
          initialSubtitle &&
          tracks('subtitle').some((track) => track.id === initialSubtitle)
        ) {
          await switchSubtitle(initialSubtitle);
        }
        phase = 'ready';
        logPlaybackLifecycle('preparation-state', ownership, {
          state: 'player-ready',
          sessionId: created.gid,
          sourceGeneration: currentSourceGeneration
        });
      } catch (cause) {
        error =
          cause instanceof Error
            ? cause.message
            : 'Playback initialization failed';
        phase = 'error';
        logPlaybackLifecycle('preparation-failed', ownership, {
          stage: 'player-initialization',
          failure: cause instanceof Error ? cause.message : String(cause)
        });
      }
    })();
    return () => {
      disposed = true;
      video.removeEventListener('timeupdate', onTime);
      video.removeEventListener('durationchange', onDuration);
      video.removeEventListener('play', onPlay);
      video.removeEventListener('pause', onPause);
      video.removeEventListener('playing', onPlaying);
      for (const event of diagnosticEvents)
        video.removeEventListener(event, onDiagnosticEvent);
      if (controlsTimer !== null) window.clearTimeout(controlsTimer);
      void cleanup(unmountReason, unmountCaller);
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
      <Button tone="surface" onclick={() => requestExit('initialization-error')}
        >Return to Eclipse</Button
      >
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
        onclick={() => requestExit('back-button')}
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
            timelineSeek.preview(Number(event.currentTarget.value))}
          onchange={(event) =>
            timelineSeek.commit(Number(event.currentTarget.value))}
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
