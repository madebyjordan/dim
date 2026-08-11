import contract from "../../../api-contract/openapi.json";

const migratedOperations = [
  "login",
  "logout",
  "register",
  "adminExists",
  "whoAmI",
  "changePassword",
  "deleteAccount",
  "listLibraries",
  "createLibrary",
  "getLibrary",
  "deleteLibrary",
  "getLibraryMedia",
  "getUnmatched",
  "getLibraryScan",
  "retryLibraryScan",
  "getMedia",
  "rematchMedia",
  "saveProgress",
  "searchExternalMedia",
  "inspectPlaybackCapabilities",
  "createPlaybackSession",
  "getPlaybackFailure",
  "killPlaybackSession",
];

test("the checked contract covers every migrated operation with unique IDs", () => {
  const operations = Object.values(contract.paths).flatMap((path) =>
    Object.entries(path)
      .filter(([method]) => ["get", "post", "patch", "delete"].includes(method))
      .map(
        ([, operation]) => (operation as { operationId: string }).operationId
      )
  );
  expect(new Set(operations).size).toBe(operations.length);
  expect(operations.sort()).toEqual(migratedOperations.sort());
});

test("the contract exposes one structured error and typed websocket messages", () => {
  expect(contract.components.schemas.ApiErrorEnvelope.required).toEqual([
    "error",
    "request_id",
  ]);
  expect(
    contract.components.schemas.WebSocketEvent.properties.type.enum
  ).toContain("EventAuthErr");
});
