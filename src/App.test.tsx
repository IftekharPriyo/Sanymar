import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import App from "./App";
import { dashboardErrorMessage } from "./hooks/useDashboard";

describe("Sanymar mock dashboard", () => {
  it("labels mock mode and generates a test segment", async () => {
    render(<App />);
    expect(await screen.findByText("Demo playback data")).toBeInTheDocument();
    expect(await screen.findByText("Test dialogue engine")).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "Generate test segment" }),
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

  it("keeps provider plumbing out of the normal settings experience", async () => {
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    expect(
      await screen.findByRole("heading", { name: "Studio settings" }),
    ).toBeInTheDocument();
    expect(screen.queryByLabelText(/access token/i)).not.toBeInTheDocument();
    expect(
      screen.queryByLabelText(/Spotify Client ID/i),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByLabelText(/Registered redirect URI/i),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Connect Spotify" }),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("MusicBrainz contact")).toBeInTheDocument();
    expect(
      screen.getByLabelText(
        "Automatically prepare and play speech at Spotify transitions",
      ),
    ).toBeDisabled();
    expect(
      screen.getByText(/uses its bundled English voice automatically/i),
    ).toBeInTheDocument();
    expect(screen.queryByLabelText("Voice engine")).not.toBeInTheDocument();
    expect(
      screen.queryByLabelText("Kokoro model directory"),
    ).not.toBeInTheDocument();
    expect(screen.queryByText(/Check voice provider/i)).not.toBeInTheDocument();
    expect(screen.getByLabelText(/RJ volume \(75%\)/i)).toHaveValue("75");
    fireEvent.click(screen.getByRole("button", { name: "Mute RJ" }));
    expect(screen.getByLabelText(/RJ volume \(0%\)/i)).toHaveValue("0");
    fireEvent.click(screen.getByLabelText("Enable development debug logging"));
    const ttsProvider = screen.getByLabelText("Voice engine");
    expect(ttsProvider).toBeInTheDocument();
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
