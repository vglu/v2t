import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { ReadinessPanel } from "./ReadinessPanel";

describe("ReadinessPanel", () => {
  it("shows unknown message when report is null", () => {
    render(
      <ReadinessPanel
        report={null}
        settings={{
          outputDir: null,
          apiKey: "",
          transcriptionMode: "httpApi",
          whisperCliPath: null,
        }}
        onOpenSettings={() => {}}
      />,
    );
    expect(screen.getByTestId("deps-unknown")).toBeInTheDocument();
  });

  it("shows ok when both tools found", () => {
    render(
      <ReadinessPanel
        report={{
          ffmpegFound: true,
          ffmpegPath: "/bin/ffmpeg",
          ytDlpFound: true,
          ytDlpPath: "/bin/yt-dlp",
          whisperCliFound: false,
          whisperCliPath: null,
        }}
        settings={{
          outputDir: "/out",
          apiKey: "sk-test",
          transcriptionMode: "httpApi",
          whisperCliPath: null,
        }}
        onOpenSettings={() => {}}
      />,
    );
    expect(screen.getByTestId("deps-status")).toHaveClass("deps-ok");
    expect(screen.getByTestId("ffmpeg-status")).toHaveTextContent("ok");
    expect(screen.getByTestId("ytdlp-status")).toHaveTextContent("ok");
  });

  it("calls onOpenSettings when button clicked", async () => {
    const user = userEvent.setup();
    const fn = vi.fn();
    render(
      <ReadinessPanel
        report={{
          ffmpegFound: false,
          ffmpegPath: null,
          ytDlpFound: true,
          ytDlpPath: "/bin/yt-dlp",
          whisperCliFound: false,
          whisperCliPath: null,
        }}
        settings={{
          outputDir: null,
          apiKey: "",
          transcriptionMode: "httpApi",
          whisperCliPath: null,
        }}
        onOpenSettings={fn}
      />,
    );
    expect(screen.getByTestId("readiness-tool-hint")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Open Settings" }));
    expect(fn).toHaveBeenCalledOnce();
  });
});
