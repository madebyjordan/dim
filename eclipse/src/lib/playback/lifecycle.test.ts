// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
  clearStoredPlayback,
  createPlaybackOwnership,
  creationQuery,
  readStoredPlayback,
  storePlayback,
  teardownQuery
} from './lifecycle';

beforeEach(() => {
  sessionStorage.clear();
  vi.spyOn(crypto, 'randomUUID').mockReturnValue(
    '00000000-0000-4000-8000-000000000001'
  );
});

describe('playback lifecycle ownership', () => {
  it('correlates creation and teardown with one instance and generation', () => {
    const ownership = createPlaybackOwnership('42', 7);
    expect(creationQuery(ownership, 'catalog-track-preparation')).toEqual({
      frontend_instance_id: ownership.instanceId,
      source_generation: 7,
      creation_reason: 'catalog-track-preparation'
    });
    expect(teardownQuery(ownership, 'normal-player-exit', 'player')).toEqual({
      frontend_instance_id: ownership.instanceId,
      source_generation: 7,
      teardown_reason: 'normal-player-exit',
      teardown_caller: 'player'
    });
  });

  it('does not let an old cleanup clear a newer session record', () => {
    const oldOwnership = createPlaybackOwnership('42', 1);
    const newOwnership = { ...oldOwnership, sourceGeneration: 2 };
    storePlayback('old-session', oldOwnership);
    storePlayback('new-session', newOwnership);

    clearStoredPlayback('old-session');

    expect(readStoredPlayback()).toEqual({
      gid: 'new-session',
      ...newOwnership
    });
  });
});
