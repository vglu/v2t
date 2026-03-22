import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { DependencyBar } from "./DependencyBar";

describe("DependencyBar", () => {
  it("shows unknown message when report is null", () => {
    render(<DependencyBar report={null} />);
    expect(screen.getByTestId("deps-unknown")).toBeInTheDocument();
  });

  it("shows ok when both tools found", () => {
    render(
      <DependencyBar
        report={{
          ffmpegFound: true,
          ffmpegPath: "/bin/ffmpeg",
          ytDlpFound: true,
          ytDlpPath: "/bin/yt-dlp",
          whisperCliFound: true,
          whisperCliPath: "/bin/whisper-cli",
        }}
      />,
    );
    expect(screen.getByTestId("deps-status")).toHaveClass("deps-ok");
    expect(screen.getByTestId("ffmpeg-status")).toHaveTextContent("ok");
    expect(screen.getByTestId("ytdlp-status")).toHaveTextContent("ok");
    expect(screen.getByTestId("whisper-status")).toHaveTextContent("ok");
  });
});
