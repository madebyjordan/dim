export type PlaybackOwnership = {
  instanceId: string;
  sourceGeneration: number;
  mediaFileId: string;
};

export type StoredPlaybackOwnership = PlaybackOwnership & { gid: string };

const playbackKey = 'eclipse.playback-session';

export function createPlaybackOwnership(
  mediaFileId: string,
  sourceGeneration: number
): PlaybackOwnership {
  return {
    instanceId: crypto.randomUUID(),
    sourceGeneration,
    mediaFileId
  };
}

export function creationQuery(
  ownership: PlaybackOwnership,
  creationReason: string
) {
  return {
    frontend_instance_id: ownership.instanceId,
    source_generation: ownership.sourceGeneration,
    creation_reason: creationReason
  };
}

export function teardownQuery(
  ownership: PlaybackOwnership,
  teardownReason: string,
  teardownCaller: string
) {
  return {
    frontend_instance_id: ownership.instanceId,
    source_generation: ownership.sourceGeneration,
    teardown_reason: teardownReason,
    teardown_caller: teardownCaller
  };
}

export function readStoredPlayback(): StoredPlaybackOwnership | null {
  const value = sessionStorage.getItem(playbackKey);
  if (!value) return null;
  try {
    const parsed = JSON.parse(value) as Partial<StoredPlaybackOwnership>;
    if (
      typeof parsed.gid === 'string' &&
      typeof parsed.instanceId === 'string' &&
      typeof parsed.sourceGeneration === 'number' &&
      typeof parsed.mediaFileId === 'string'
    )
      return parsed as StoredPlaybackOwnership;
  } catch {
    // Legacy versions stored only a gid. It remains safe to reap without an
    // ownership assertion because UUIDs are never reused by another session.
    return {
      gid: value,
      instanceId: '',
      sourceGeneration: 0,
      mediaFileId: ''
    };
  }
  return null;
}

export function storePlayback(
  gid: string,
  ownership: PlaybackOwnership
): void {
  sessionStorage.setItem(playbackKey, JSON.stringify({ gid, ...ownership }));
}

export function clearStoredPlayback(gid: string): void {
  if (readStoredPlayback()?.gid === gid) sessionStorage.removeItem(playbackKey);
}

export function logPlaybackLifecycle(
  event: string,
  ownership: PlaybackOwnership,
  details: Record<string, unknown> = {}
) {
  console.info('[playback-lifecycle]', {
    event,
    frontendInstanceId: ownership.instanceId,
    mediaFileId: ownership.mediaFileId,
    sourceGeneration: ownership.sourceGeneration,
    ...details
  });
}
