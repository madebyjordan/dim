import { SCAN_FAILED, SCAN_START, SCAN_STOP } from "../actions/types";
import reducer from "./library";

describe("durable library scan progress", () => {
  const progress = {
    status: "scanning",
    stage: "matching",
    discovered: 9,
    processed: 5,
    committed: 3,
    skipped: 1,
    failed: 1,
    elapsed_seconds: 42,
    seconds_since_progress: 2,
  };

  it("hydrates full polling progress and replaces a missed terminal event", () => {
    const scanning = reducer(undefined, {
      type: SCAN_START,
      id: 7,
      payload: progress,
    });
    expect(scanning.scanning).toEqual([7]);
    expect(scanning.scan_progress[7]).toEqual(progress);

    const terminal = reducer(scanning, {
      type: SCAN_FAILED,
      id: 7,
      payload: {
        ...progress,
        status: "failed",
        stage: "failed",
        error_summary: "Reconnect the share and retry",
      },
    });
    expect(terminal.scanning).toEqual([]);
    expect(terminal.scan_status[7]).toBe("failed");
    expect(terminal.scan_progress[7].error_summary).toMatch(/Reconnect/);
  });

  it("keeps durable counts when a WebSocket event arrives without a payload", () => {
    const scanning = reducer(undefined, {
      type: SCAN_START,
      id: 7,
      payload: progress,
    });
    const complete = reducer(scanning, { type: SCAN_STOP, id: 7 });
    expect(complete.scan_progress[7]).toMatchObject({
      status: "complete",
      discovered: 9,
      processed: 5,
    });
  });
});
