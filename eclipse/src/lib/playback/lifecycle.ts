export type PlaybackOwnership = {
  instanceId: string;
  sourceGeneration: number;
  mediaFileId: string;
};

export type StoredPlaybackOwnership = PlaybackOwnership & { gid: string };

const playbackKey = 'eclipse.playback-session';

type PlaybackCrypto = Pick<Crypto, 'getRandomValues'> &
  Partial<Pick<Crypto, 'randomUUID'>>;

export function createPlaybackInstanceId(
  provider: PlaybackCrypto = globalThis.crypto
): string {
  if (typeof provider?.randomUUID === 'function') {
    return provider.randomUUID();
  }
  if (typeof provider?.getRandomValues !== 'function') {
    throw new Error('This browser cannot create a secure playback identity.');
  }
  const bytes = provider.getRandomValues(new Uint8Array(16));
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0'));
  return [
    hex.slice(0, 4).join(''),
    hex.slice(4, 6).join(''),
    hex.slice(6, 8).join(''),
    hex.slice(8, 10).join(''),
    hex.slice(10).join('')
  ].join('-');
}

export function playbackBrowserContext() {
  const userAgent = globalThis.navigator?.userAgent ?? 'unknown';
  const engine =
    /AppleWebKit/i.test(userAgent) &&
    !/(?:Chrome|Chromium|CriOS)/i.test(userAgent)
      ? 'webkit'
      : /(?:Chrome|Chromium|CriOS)/i.test(userAgent)
        ? 'chromium'
        : /Gecko\//i.test(userAgent)
          ? 'gecko'
          : 'unknown';
  return {
    browserEngine: engine,
    browserUserAgent: userAgent,
    secureContext: globalThis.isSecureContext ?? false
  };
}

export function createPlaybackOwnership(
  mediaFileId: string,
  sourceGeneration: number
): PlaybackOwnership {
  return {
    instanceId: createPlaybackInstanceId(),
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

export function storePlayback(gid: string, ownership: PlaybackOwnership): void {
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
    ...playbackBrowserContext(),
    frontendInstanceId: ownership.instanceId,
    mediaFileId: ownership.mediaFileId,
    sourceGeneration: ownership.sourceGeneration,
    ...details
  });
}
