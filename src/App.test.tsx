import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import App from "./App";
import { dashboardErrorMessage } from "./hooks/useDashboard";

describe("Sanymar mock dashboard", () => {
  it("labels mock mode and generates a test segment", async () => {
    render(<App />);
    expect(await screen.findByText("Mock Spotify")).toBeInTheDocument();
    expect(
      await screen.findByText("Mock script generator"),
    ).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "Generate mock segment" }),
    );
    expect(
      await screen.findByText(/streetlights to get involved/i),
    ).toBeInTheDocument();
  });

  it("preserves typed native command errors", () => {
    expect(
      dashboardErrorMessage(
        {
          code: "provider_unavailable",
          message: "provider operation failed: no active playback",
        },
        "fallback",
      ),
    ).toBe("provider operation failed: no active playback");
  });

  it("shows a Client ID field without exposing token or client-secret fields", async () => {
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    expect(
      await screen.findByRole("heading", { name: "Studio settings" }),
    ).toBeInTheDocument();
    expect(screen.queryByLabelText(/access token/i)).not.toBeInTheDocument();
    expect(screen.getByLabelText("Spotify Client ID")).toBeInTheDocument();
    expect(screen.getByLabelText("MusicBrainz contact")).toBeInTheDocument();
    expect(
      screen.getByLabelText(
        "Automatically prepare and play speech at Spotify transitions",
      ),
    ).toBeDisabled();
    const ttsProvider = screen.getByLabelText("TTS provider");
    expect(ttsProvider).toBeInTheDocument();
    fireEvent.change(ttsProvider, { target: { value: "sherpa_kokoro" } });
    expect(screen.getByLabelText("Kokoro model directory")).toBeInTheDocument();
    fireEvent.change(ttsProvider, { target: { value: "parler_mini" } });
    expect(screen.getByLabelText("Parler local service URL")).toHaveValue(
      "http://127.0.0.1:43822",
    );
    expect(screen.getByLabelText("Parler speaker")).toHaveValue("Jon");
    expect(screen.queryByLabelText(/client secret/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/voice cloning/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/review fact/i)).not.toBeInTheDocument();
  });
});
