import { describe, expect, it } from "vitest";
import { itemStatus, type QueueItem } from "./queue";

function baseItem(over: Partial<QueueItem> = {}): QueueItem {
  return {
    id: "j1",
    kind: "url",
    source: "https://example.com/a",
    displayLabel: "a",
    language: null,
    acquire: { status: "pending", attempts: 0 },
    transcribe: { status: "pending", attempts: 0 },
    transcriptPath: null,
    ...over,
  };
}

describe("itemStatus", () => {
  it("marks acquire-done + transcribe-pending as waiting", () => {
    expect(
      itemStatus(
        baseItem({
          acquire: { status: "done", attempts: 1 },
          transcribe: { status: "pending", attempts: 0 },
        }),
      ),
    ).toBe("waiting");
  });

  it("keeps running while either stage runs", () => {
    expect(
      itemStatus(
        baseItem({
          acquire: { status: "running", attempts: 1 },
          transcribe: { status: "pending", attempts: 0 },
        }),
      ),
    ).toBe("running");
    expect(
      itemStatus(
        baseItem({
          acquire: { status: "done", attempts: 1 },
          transcribe: { status: "running", attempts: 1 },
        }),
      ),
    ).toBe("running");
  });
});
