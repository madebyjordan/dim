export function isAmbiguousAirPlayPlayRejection(
  cause: unknown
): cause is DOMException {
  return cause instanceof DOMException && cause.name === 'AbortError';
}

export function shouldDeferAirPlayRouteLoss(
  deliveryConfirmed: boolean,
  awaitingDeliveryEvidence: boolean
) {
  return deliveryConfirmed || awaitingDeliveryEvidence;
}
