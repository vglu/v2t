export type QueueJobStatus =
  | "pending"
  | "running"
  | "done"
  | "error"
  | "cancelled";

export type QueueJobKind = "file" | "url" | "folder";

export type QueueJob = {
  id: string;
  kind: QueueJobKind;
  /** Path or URL — passed to backend pipeline later */
  source: string;
  displayLabel: string;
  status: QueueJobStatus;
  detail?: string;
};
