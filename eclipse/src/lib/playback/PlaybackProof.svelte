<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import type {
    PlaybackCapabilityInspection,
    PlaybackSession,
    PlaybackTrack,
    RemotePlaybackState,
    RemotePlaybackStatus
  } from '$lib/api/generated';
  import { session } from '$lib/auth/session.svelte';
  import Button from '$lib/primitives/Button.svelte';
  import Select from '$lib/primitives/Select.svelte';
  import {
    determineCapabilities,
    type BrowserCapabilities
  } from './capabilities';
  import {
    isAmbiguousAirPlayPlayRejection,
    shouldDeferAirPlayRouteLoss
  } from './airplay-state';
  import {
    clearStoredPlayback,
    createPlaybackInstanceId,
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

  interface WebKitPlaybackTargetAvailabilityEvent extends Event {
    availability?: 'available' | 'not-available';
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
  let remoteAirPlayHost: HTMLElement;
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
  let remoteEventCleanup: (() => void) | null = null;
  let airPlayStatusTimer: number | null = null;
  let airPlayMonitorGeneration = 0;
  let airPlayAttempt: { id: string; startedAt: number } | null = null;
  let airPlayHandoffTime = 0;
  let airPlayDeliveryConfirmed = false;
  let airPlayAwaitingDeliveryEvidence = false;
  let airPlayRouteUncertain = false;
  let localPlaybackRestorePromise: Promise<void> | null = null;
  let airPlayTargetAvailability: 'unknown' | 'available' | 'not-available' =
    'unknown';
  let airPlayState = $state<
    | 'unavailable'
    | 'available'
    | 'preparing'
    | 'ready'
    | 'route-selected'
    | 'active'
    | 'failed'
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
  const airPlayMetadataDeadlineMs = 15_000;

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

  function playbackControlTarget(): HTMLVideoElement {
    return remoteVideo &&
      (airPlayState === 'route-selected' || airPlayState === 'active')
      ? remoteVideo
      : video;
  }

  function togglePlayback() {
    const target = playbackControlTarget();
    if (target.paused) void target.play();
    else target.pause();
    showControls();
  }

  function toggleMute() {
    const target = playbackControlTarget();
    target.muted = !target.muted;
    muted = target.muted;
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
      const target = playbackControlTarget();
      beginSeek(Math.max(0, target.currentTime - 10), 'keyboard-arrow-left');
    } else if (event.key === 'ArrowRight') {
      event.preventDefault();
      const target = playbackControlTarget();
      beginSeek(
        Math.min(target.duration || Infinity, target.currentTime + 10),
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
    from = playbackControlTarget().currentTime,
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
    const controlled = playbackControlTarget();
    if (controlled === remoteVideo) {
      logAirPlayStage('remote-seek-requested', { from, target, reason });
      reportRemotePlayerEvent('seek-committed', {
        from,
        target,
        reason,
        inputCount
      });
    } else {
      reportPlayerEvent(
        'seek-committed',
        inputCount === undefined
          ? undefined
          : JSON.stringify({ scrub_input_count: inputCount }),
        trace
      );
    }
    controlled.currentTime = target;
  }

  const timelineSeek = createSeekCommitter(
    () => playbackControlTarget().currentTime,
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
    await session.api.request<void>(`stream/${gid}/state/kill`, {
      method: 'DELETE',
      keepalive: true,
      query: sessionOwnership.instanceId
        ? teardownQuery(sessionOwnership, reason, caller)
        : { teardown_reason: reason, teardown_caller: caller }
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
      (airPlayState === 'route-selected' || airPlayState === 'active') &&
      remoteVideo?.webkitShowPlaybackTargetPicker
    ) {
      try {
        remoteVideo.webkitShowPlaybackTargetPicker();
        logAirPlayStage('route-picker-reopened', remoteMediaState(remoteVideo));
      } catch (cause) {
        logAirPlayStage('route-picker-reopen-failed', {
          failure: cause instanceof Error ? cause.message : String(cause)
        });
      }
      return;
    }
    if (
      airPlayState === 'ready' &&
      remoteVideo?.webkitShowPlaybackTargetPicker
    ) {
      airPlayAttempt = {
        id: createPlaybackInstanceId(),
        startedAt: performance.now()
      };
      logAirPlayStage('attempt-started', {
        playlistPath: new URL(remoteVideo.currentSrc || remoteVideo.src)
          .pathname
      });
      void updateRemoteRouteState('handoff_requested').catch((cause) =>
        logAirPlayStage('route-state-report-failed', {
          routeState: 'handoff_requested',
          failure: cause instanceof Error ? cause.message : String(cause)
        })
      );
      try {
        remoteVideo.webkitShowPlaybackTargetPicker();
        logAirPlayStage('picker-opened');
        // WebKit does not always emit a change event when this element inherits an already
        // selected system route. Re-read the property at the end of the picker invocation so
        // that an existing wireless target follows the same handoff path as a new selection.
        queueMicrotask(() => {
          if (remoteVideo?.webkitCurrentPlaybackTargetIsWireless)
            void handleWirelessTargetChanged(remoteVideo);
        });
      } catch (cause) {
        airPlayState = 'failed';
        error =
          cause instanceof Error
            ? cause.message
            : 'Safari could not open the AirPlay picker.';
        logAirPlayStage('picker-failed', { failure: error });
      }
      return;
    }
    airPlayState = 'preparing';
    try {
      await disposeRemotePlayback('airplay-attempt-replaced');
      const remote = await createSession('airplay');
      if (!remote.remote)
        throw new Error('The backend did not provide an AirPlay resource.');
      remoteSessionId = remote.gid;
      const element = document.createElement('video') as WebKitAirPlayVideo;
      element.playsInline = true;
      element.setAttribute('x-webkit-airplay', 'allow');
      // WebKit on macOS silently returns from webkitShowPlaybackTargetPicker()
      // while readyState is below HAVE_METADATA. Chromium has no equivalent
      // picker precondition, so prepare this native HLS element only in the
      // WebKit AirPlay path.
      element.preload = 'metadata';
      remoteAirPlayHost.append(element);
      const onWirelessChanged = () => void handleWirelessTargetChanged(element);
      const onAvailabilityChanged = (event: Event) => {
        if (remoteVideo !== element) return;
        const availability = (event as WebKitPlaybackTargetAvailabilityEvent)
          .availability;
        airPlayTargetAvailability = availability ?? 'unknown';
        // Safari's availability event is advisory and may report `not-available`
        // until its system picker performs discovery. Keep the picker accessible;
        // only the presence of the picker API determines UI support.
        logAirPlayStage('target-availability-changed', { availability });
      };
      const diagnosticEvents = [
        'abort',
        'canplay',
        'emptied',
        'ended',
        'error',
        'loadeddata',
        'loadedmetadata',
        'loadstart',
        'pause',
        'play',
        'playing',
        'progress',
        'stalled',
        'suspend',
        'waiting'
      ] as const;
      const onRemoteMediaEvent = (event: Event) => {
        if (remoteVideo !== element) return;
        const details = remoteMediaState(element);
        if (event.type === 'play' || event.type === 'playing') paused = false;
        else if (event.type === 'pause') paused = true;
        logAirPlayStage(`media-${event.type}`, details);
        reportRemotePlayerEvent(event.type, details);
        if (
          airPlayDeliveryConfirmed &&
          (event.type === 'error' ||
            ((event.type === 'stalled' || event.type === 'waiting') &&
              !element.paused))
        )
          void reportEstablishedRouteUncertainty(
            'failed',
            `remote-media-${event.type}`
          );
      };
      const onRemoteTime = () => {
        if (remoteVideo !== element) return;
        if (Number.isFinite(element.currentTime))
          airPlayHandoffTime = element.currentTime;
        if (timeOutput) timeOutput.value = formatTime(element.currentTime);
        if (seekInput && !timelineSeek.isPreviewing())
          seekInput.value = String(element.currentTime);
      };
      element.addEventListener(
        'webkitcurrentplaybacktargetiswirelesschanged',
        onWirelessChanged
      );
      element.addEventListener(
        'webkitplaybacktargetavailabilitychanged',
        onAvailabilityChanged
      );
      for (const event of diagnosticEvents)
        element.addEventListener(event, onRemoteMediaEvent);
      element.addEventListener('timeupdate', onRemoteTime);
      remoteEventCleanup = () => {
        element.removeEventListener(
          'webkitcurrentplaybacktargetiswirelesschanged',
          onWirelessChanged
        );
        element.removeEventListener(
          'webkitplaybacktargetavailabilitychanged',
          onAvailabilityChanged
        );
        for (const event of diagnosticEvents)
          element.removeEventListener(event, onRemoteMediaEvent);
        element.removeEventListener('timeupdate', onRemoteTime);
      };
      remoteVideo = element;
      element.src = new URL(remote.remote.url, window.location.origin).href;
      element.load();
      await waitForAirPlayMetadata(element);
      airPlayState = 'ready';
      error = null;
      logPlaybackLifecycle('airplay-resource-ready', ownership, {
        sessionId: remote.gid,
        playbackPlan: remote.playback_plan,
        preload: element.preload,
        readyState: element.readyState
      });
    } catch (cause) {
      airPlayState = 'failed';
      error =
        cause instanceof Error ? cause.message : 'AirPlay preparation failed';
    }
  }

  function waitForAirPlayMetadata(element: WebKitAirPlayVideo) {
    if (element.readyState >= HTMLMediaElement.HAVE_METADATA)
      return Promise.resolve();

    return new Promise<void>((resolve, reject) => {
      const finish = (failure?: Error) => {
        window.clearTimeout(deadline);
        element.removeEventListener('loadedmetadata', onLoadedMetadata);
        element.removeEventListener('abort', onAbort);
        element.removeEventListener('error', onError);
        if (failure) reject(failure);
        else resolve();
      };
      const onLoadedMetadata = () => finish();
      const onAbort = () =>
        finish(new Error('Safari aborted AirPlay media initialization.'));
      const onError = () =>
        finish(
          new Error(
            element.error?.message ||
              'Safari could not initialize the AirPlay media resource.'
          )
        );
      const deadline = window.setTimeout(
        () =>
          finish(
            new Error(
              'Safari did not make the AirPlay media element picker-ready within 15 seconds.'
            )
          ),
        airPlayMetadataDeadlineMs
      );
      element.addEventListener('loadedmetadata', onLoadedMetadata);
      element.addEventListener('abort', onAbort);
      element.addEventListener('error', onError);
    });
  }

  function remoteMediaState(element = remoteVideo) {
    if (!element) return {};
    return {
      wirelessTarget: element.webkitCurrentPlaybackTargetIsWireless ?? false,
      currentTime: finite(element.currentTime),
      duration: finite(element.duration),
      readyState: element.readyState,
      networkState: element.networkState,
      paused: element.paused,
      ended: element.ended,
      errorCode: element.error?.code,
      errorMessage: element.error?.message
    };
  }

  function logAirPlayStage(
    stage: string,
    details: Record<string, unknown> = {}
  ) {
    const entry = {
      attemptId: airPlayAttempt?.id ?? null,
      sessionId: remoteSessionId,
      stage,
      elapsedMs: airPlayAttempt
        ? Math.round(performance.now() - airPlayAttempt.startedAt)
        : null,
      ...details
    };
    logPlaybackLifecycle('airplay-attempt', ownership, entry);
    console.info(
      '[airplay-stage]',
      stage,
      String(
        details.availability ??
          details.wireless ??
          details.routeState ??
          details.failure ??
          ''
      ),
      JSON.stringify(entry)
    );
  }

  function logAirPlayFailureDecision(
    trigger: string,
    status: RemotePlaybackStatus,
    whyEvidenceIsSufficient: string
  ) {
    logAirPlayStage('failure-decision', {
      eventTrigger: trigger,
      webkitRouteState:
        remoteVideo?.webkitCurrentPlaybackTargetIsWireless === true
          ? 'wireless'
          : 'local',
      deliveryConfirmed: airPlayDeliveryConfirmed,
      lastConfirmedReceiverOrProxySegment:
        status.last_remote_segment_path ?? null,
      expectedBufferedCoverageRemainingMs:
        status.delivery_evidence_remaining_ms ?? 0,
      remoteOwnershipState: airPlayState,
      backendState: status.state,
      evidenceSufficientToFail: true,
      whyEvidenceIsSufficient
    });
  }

  function reportRemotePlayerEvent(
    event: string,
    details: Record<string, unknown> = {}
  ) {
    if (!remoteSessionId) return;
    void session.api
      .post(`stream/${remoteSessionId}/state/player-event`, {
        event: `airplay-${event}`,
        frontend_instance_id: ownership.instanceId,
        media_file_id: fileId,
        source_generation: ownership.sourceGeneration,
        detail: JSON.stringify({
          attemptId: airPlayAttempt?.id ?? null,
          ...details
        })
      })
      .catch(() => undefined);
  }

  async function updateRemoteRouteState(state: RemotePlaybackState) {
    if (!remoteSessionId) return;
    await session.api.request<void>(
      `stream/${remoteSessionId}/state/remote-route`,
      { method: 'PUT', body: JSON.stringify({ state }) }
    );
    logAirPlayStage('route-state-reported', { routeState: state });
  }

  function stopAirPlayStatusMonitor() {
    airPlayMonitorGeneration += 1;
    if (airPlayStatusTimer !== null) window.clearTimeout(airPlayStatusTimer);
    airPlayStatusTimer = null;
  }

  function monitorAirPlayDelivery() {
    stopAirPlayStatusMonitor();
    const sessionId = remoteSessionId;
    if (!sessionId) return;
    const monitorGeneration = airPlayMonitorGeneration;
    const isCurrentMonitor = () =>
      airPlayMonitorGeneration === monitorGeneration &&
      remoteSessionId === sessionId;
    const scheduleNextPoll = () => {
      if (!isCurrentMonitor()) return;
      airPlayStatusTimer = window.setTimeout(poll, 500);
    };
    const poll = async () => {
      if (!isCurrentMonitor()) return;
      try {
        const status = await session.api.get<RemotePlaybackStatus>(
          `stream/${sessionId}/state/remote-route`
        );
        if (!isCurrentMonitor()) return;
        logAirPlayStage('delivery-status', status);
        if (status.state === 'media_delivery_confirmed') {
          airPlayState = 'active';
          error = null;
          if (!airPlayDeliveryConfirmed)
            logAirPlayStage('remote-delivery-established', status);
          airPlayDeliveryConfirmed = true;
          airPlayAwaitingDeliveryEvidence = false;
          airPlayRouteUncertain = status.route_loss_reported;
          scheduleNextPoll();
          return;
        }
        if (status.state === 'handoff_stalled' || status.state === 'failed') {
          airPlayAwaitingDeliveryEvidence = false;
          airPlayState = 'failed';
          error =
            status.last_request_stage === 'init_fragment'
              ? 'The AirPlay receiver reached Eclipse, but playback stopped at stream initialization.'
              : status.last_request_stage === 'media_segment'
                ? 'The AirPlay receiver began fetching media, but delivery stopped before playback was established.'
                : 'Safari selected an AirPlay receiver, but Eclipse did not observe remote media delivery.';
          logAirPlayFailureDecision(
            status.state === 'handoff_stalled'
              ? 'backend-handoff-deadline'
              : 'backend-delivery-evidence-expired',
            status,
            status.state === 'handoff_stalled'
              ? 'WebKit reported a wireless route, but the backend did not observe two distinct attributed media segments before the route-selection delivery deadline.'
              : 'Previously confirmed receiver delivery stopped, and the backend-observed segment coverage plus its playlist-derived grace interval has expired.'
          );
          logAirPlayStage('remote-delivery-failed', {
            ...status,
            failure: error
          });
          await disposeRemotePlayback(`airplay-${status.state}`);
          airPlayState = 'failed';
          await restoreLocalPlayback();
          return;
        }
        if (status.state === 'disconnected') {
          logAirPlayStage('remote-termination-decision', {
            eventTrigger: 'backend-confirmed-disconnect',
            webkitRouteState:
              remoteVideo?.webkitCurrentPlaybackTargetIsWireless === true
                ? 'wireless'
                : 'local',
            deliveryConfirmed: airPlayDeliveryConfirmed,
            lastConfirmedReceiverOrProxySegment:
              status.last_remote_segment_path ?? null,
            expectedBufferedCoverageRemainingMs:
              status.delivery_evidence_remaining_ms ?? 0,
            remoteOwnershipState: airPlayState,
            backendState: status.state,
            whyEvidenceIsSufficient:
              'The backend accepted the disconnect only before delivery or after attributed receiver coverage expired.'
          });
          airPlayDeliveryConfirmed = false;
          airPlayAwaitingDeliveryEvidence = false;
          airPlayRouteUncertain = false;
          airPlayAttempt = null;
          airPlayState = 'ready';
          error = null;
          logAirPlayStage('remote-delivery-ended', status);
          await disposeRemotePlayback('airplay-disconnected');
          airPlayState = 'available';
          await restoreLocalPlayback();
          void prepareAirPlay();
          return;
        }
        scheduleNextPoll();
      } catch (cause) {
        if (!isCurrentMonitor()) return;
        // A failed status read is absence of evidence, not evidence that receiver delivery ended.
        // Keep remote ownership and local suspension until the backend can make an evidence-backed
        // terminal decision.
        airPlayRouteUncertain = true;
        logAirPlayStage('delivery-status-uncertain', {
          webkitRouteState:
            remoteVideo?.webkitCurrentPlaybackTargetIsWireless === true
              ? 'wireless'
              : 'local',
          deliveryConfirmed: airPlayDeliveryConfirmed,
          remoteOwnershipState: airPlayState,
          failure: cause instanceof Error ? cause.message : String(cause),
          terminalDecision: 'deferred-no-delivery-evidence'
        });
        scheduleNextPoll();
      }
    };
    void poll();
  }

  async function reportEstablishedRouteUncertainty(
    state: 'failed' | 'disconnected',
    reason: string
  ) {
    if (!airPlayDeliveryConfirmed) return false;
    airPlayRouteUncertain = true;
    logAirPlayStage('established-route-uncertain', {
      reportedState: state,
      reason
    });
    await updateRemoteRouteState(state).catch((cause) =>
      logAirPlayStage('route-state-report-failed', {
        routeState: state,
        failure: cause instanceof Error ? cause.message : String(cause)
      })
    );
    monitorAirPlayDelivery();
    return true;
  }

  async function handleWirelessTargetChanged(element: WebKitAirPlayVideo) {
    if (remoteVideo !== element) {
      logAirPlayStage('stale-wireless-target-callback-ignored', {
        callbackWirelessState:
          element.webkitCurrentPlaybackTargetIsWireless === true,
        callbackSource: element.currentSrc || element.src
      });
      return;
    }
    const wireless = element.webkitCurrentPlaybackTargetIsWireless === true;
    if (Number.isFinite(element.currentTime))
      airPlayHandoffTime = element.currentTime;
    if (
      wireless &&
      (airPlayState === 'route-selected' || airPlayState === 'active')
    ) {
      if (airPlayState === 'active' && airPlayRouteUncertain) {
        airPlayRouteUncertain = false;
        logAirPlayStage('established-route-recovered');
        void updateRemoteRouteState('wireless_route_reported').catch(
          () => undefined
        );
      }
      return;
    }
    logAirPlayStage('wireless-target-changed', {
      wireless,
      ...remoteMediaState(element)
    });
    if (!wireless) {
      if (
        await reportEstablishedRouteUncertainty(
          'disconnected',
          'webkit-wireless-route-false'
        )
      )
        return;
      if (
        shouldDeferAirPlayRouteLoss(
          airPlayDeliveryConfirmed,
          airPlayAwaitingDeliveryEvidence
        )
      ) {
        airPlayRouteUncertain = true;
        logAirPlayStage('handoff-route-signal-uncertain', {
          awaitingDeliveryEvidence: airPlayAwaitingDeliveryEvidence
        });
        monitorAirPlayDelivery();
        return;
      }
      if (airPlayAttempt && remoteSessionId) {
        airPlayRouteUncertain = true;
        await updateRemoteRouteState('disconnected').catch((cause) =>
          logAirPlayStage('route-state-report-failed', {
            routeState: 'disconnected',
            failure: cause instanceof Error ? cause.message : String(cause)
          })
        );
        logAirPlayStage('pre-delivery-route-ended');
        // Let the correlated backend state decide ownership. This also closes the race
        // where receiver traffic confirms delivery between frontend status polls.
        monitorAirPlayDelivery();
        return;
      }
      airPlayAttempt = null;
      airPlayState = 'ready';
      await restoreLocalPlayback();
      return;
    }

    airPlayState = 'route-selected';
    airPlayDeliveryConfirmed = false;
    airPlayAwaitingDeliveryEvidence = true;
    airPlayRouteUncertain = false;
    airPlayHandoffTime = video.currentTime;
    logAirPlayStage('receiver-selected', { localTime: airPlayHandoffTime });
    const selectedSessionId = remoteSessionId;
    const selectedAttemptId = airPlayAttempt?.id;
    const isCurrentSelection = () =>
      remoteVideo === element &&
      remoteSessionId === selectedSessionId &&
      airPlayAttempt?.id === selectedAttemptId;
    try {
      await updateRemoteRouteState('wireless_route_reported');
      if (!isCurrentSelection()) return;
      video.pause();
      dashErrorCleanup?.();
      dashErrorCleanup = null;
      dashTelemetryCleanup?.();
      dashTelemetryCleanup = null;
      dashPlayer?.destroy();
      dashPlayer = null;
      // Delivery monitoring starts before play() settles. WebKit can resolve, reject, or leave the
      // promise pending while the receiver independently fetches HLS; none of those outcomes is an
      // authoritative ownership signal.
      monitorAirPlayDelivery();
      const playRequest = element.play();
      void playRequest.then(
        () => {
          if (!isCurrentSelection()) return;
          logAirPlayStage(
            'remote-play-request-accepted-advisory',
            remoteMediaState(element)
          );
        },
        (cause: unknown) => {
          if (!isCurrentSelection()) return;
          airPlayRouteUncertain = true;
          error = null;
          logAirPlayStage('remote-play-request-rejected-advisory', {
            ambiguousAbort:
              cause instanceof DOMException &&
              isAmbiguousAirPlayPlayRejection(cause),
            exceptionName:
              cause instanceof DOMException ? cause.name : undefined,
            failure: cause instanceof Error ? cause.message : String(cause),
            terminalDecision: 'deferred-to-receiver-delivery-evidence',
            ...remoteMediaState(element)
          });
        }
      );
    } catch (cause) {
      if (!isCurrentSelection()) return;
      airPlayRouteUncertain = true;
      error = null;
      logAirPlayStage('route-selection-processing-uncertain', {
        failure: cause instanceof Error ? cause.message : String(cause),
        terminalDecision: 'deferred-to-backend-delivery-evidence',
        ...remoteMediaState(element)
      });
      monitorAirPlayDelivery();
    }
  }

  function restoreLocalPlayback() {
    if (localPlaybackRestorePromise) return localPlaybackRestorePromise;
    const restore = (async () => {
      if (!playbackSession || dashPlayer || phase !== 'ready') return;
      try {
        await activate();
        if (airPlayHandoffTime > 0) {
          if (video.readyState < HTMLMediaElement.HAVE_METADATA) {
            await new Promise<void>((resolve) =>
              video.addEventListener('loadedmetadata', () => resolve(), {
                once: true
              })
            );
          }
          video.currentTime = airPlayHandoffTime;
        }
        await video.play();
        logAirPlayStage('local-playback-restored', {
          currentTime: video.currentTime
        });
      } catch (cause) {
        error =
          cause instanceof Error
            ? cause.message
            : 'Local playback could not be restored.';
        logAirPlayStage('local-playback-restore-failed', { failure: error });
      }
    })();
    const pending = restore.finally(() => {
      if (localPlaybackRestorePromise === pending)
        localPlaybackRestorePromise = null;
    });
    localPlaybackRestorePromise = pending;
    return pending;
  }

  async function disposeRemotePlayback(reason: string) {
    stopAirPlayStatusMonitor();
    remoteEventCleanup?.();
    remoteEventCleanup = null;
    remoteVideo?.pause();
    remoteVideo?.removeAttribute('src');
    remoteVideo?.load();
    remoteVideo?.remove();
    remoteVideo = null;
    airPlayAttempt = null;
    airPlayDeliveryConfirmed = false;
    airPlayAwaitingDeliveryEvidence = false;
    airPlayRouteUncertain = false;
    const gid = remoteSessionId;
    remoteSessionId = null;
    if (gid) {
      clearStoredPlayback(gid);
      await teardownSession(
        gid,
        reason,
        'PlaybackProof.disposeRemotePlayback'
      ).catch(() => undefined);
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
    const ids = [playbackSession?.gid].filter(Boolean) as string[];
    playbackSession = null;
    for (const gid of ids) clearStoredPlayback(gid);
    // Dispatch both remote and local keepalive teardowns before yielding. Page closure can stop
    // unmount work after the first await; serial cleanup left a restored local transcoder behind
    // and poisoned the next AirPlay preparation at the admission boundary.
    const remoteCleanup = disposeRemotePlayback(reason);
    await Promise.allSettled([
      remoteCleanup,
      ...ids.map((gid) => teardownSession(gid, reason, caller))
    ]);
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
        // Prepare the remote resource before the user gesture. WebKit requires
        // webkitShowPlaybackTargetPicker() to run synchronously from the click;
        // consuming that click while awaiting session creation makes the button
        // appear inert and requires an undocumented second click.
        if (airPlayState === 'available') void prepareAirPlay();
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
  <div
    class="remote-airplay-host"
    class:airplay-active={airPlayState === 'active'}
    bind:this={remoteAirPlayHost}
    aria-hidden="true"
  ></div>

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
            const target = playbackControlTarget();
            target.volume = Number(event.currentTarget.value);
            target.muted = target.volume === 0;
            muted = target.muted;
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
              ? 'AirPlay streaming'
              : airPlayState === 'route-selected'
                ? 'Connecting AirPlay…'
                : airPlayState === 'preparing'
                  ? 'Preparing…'
                  : airPlayState === 'failed'
                    ? 'Retry AirPlay'
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
  .remote-airplay-host {
    position: fixed;
    width: 1px;
    height: 1px;
    left: 0;
    bottom: 0;
    overflow: hidden;
    opacity: 0.001;
    pointer-events: none;
  }
  .remote-airplay-host :global(video) {
    width: 1px;
    height: 1px;
  }
  /* The remote media element is also Safari's native AirPlay status surface. Keep it tiny while
     preparing so it cannot cover local playback, then promote that exact routed element after
     receiver delivery is proven. WebKit supplies the icon, localized copy, and receiver name. */
  .remote-airplay-host.airplay-active {
    inset: 0;
    width: auto;
    height: auto;
    opacity: 1;
    background: #000;
  }
  .remote-airplay-host.airplay-active :global(video) {
    width: 100%;
    height: 100%;
    object-fit: contain;
    background: #000;
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
