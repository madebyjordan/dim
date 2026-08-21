import { describe, expect, it } from 'vitest';
import {
  isAmbiguousAirPlayPlayRejection,
  shouldDeferAirPlayRouteLoss
} from './airplay-state';

describe('AirPlay handoff state', () => {
  it('treats WebKit AbortError as ambiguous rather than a delivery failure', () => {
    expect(
      isAmbiguousAirPlayPlayRejection(
        new DOMException('The operation was aborted.', 'AbortError')
      )
    ).toBe(true);
    expect(
      isAmbiguousAirPlayPlayRejection(
        new DOMException('Playback is not allowed.', 'NotAllowedError')
      )
    ).toBe(false);
    expect(isAmbiguousAirPlayPlayRejection(new Error('network'))).toBe(false);
  });

  it('preserves remote ownership while initial delivery evidence is pending', () => {
    expect(shouldDeferAirPlayRouteLoss(false, true)).toBe(true);
    expect(shouldDeferAirPlayRouteLoss(true, false)).toBe(true);
    expect(shouldDeferAirPlayRouteLoss(false, false)).toBe(false);
  });
});
