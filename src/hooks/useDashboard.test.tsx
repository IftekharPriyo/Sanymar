import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { sanymarService } from "../services/sanymar";
import type { Dashboard } from "../types/domain";
import { useDashboard } from "./useDashboard";

const liveDashboard: Dashboard = {
  mockMode: false,
  llmMockMode: false,
  ttsMockMode: true,
  connectionStatus: "Connected to Spotify",
  currentProvider: "Spotify Web API",
  playback: {
    currentTrack: null,
    nextTrack: null,
    progressMs: 0,
    isPlaying: true,
    device: null,
  },
  broadcastState: "Idle",
  djProfile: {
    id: "test",
    name: "Test",
    stationName: "Test",
    personalityTraits: [],
    energyLevel: 1,
    humourStyle: "none",
    formality: 1,
    preferredLanguage: "English",
    banglaEnglishMix: 0,
    averageWords: 10,
    minimumWords: 1,
    maximumWords: 20,
    talkFrequency: 0.5,
    restrictedSubjects: [],
    disallowedPhrases: [],
    stationLore: [],
    runningJokes: [],
    addressesListener: false,
    reactsToTimeOfDay: false,
    mildSarcasm: false,
  },
  talkFrequency: "Normal",
  llmStatus: "Mock",
  ttsStatus: "Mock",
  recentScript: null,
};

describe("useDashboard live refresh", () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("polls live Spotify state every five seconds", async () => {
    vi.useFakeTimers();
    const getDashboard = vi
      .spyOn(sanymarService, "getDashboard")
      .mockResolvedValue(liveDashboard);
    renderHook(() => useDashboard());

    await act(async () => {
      await Promise.resolve();
    });
    expect(getDashboard).toHaveBeenCalledTimes(1);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(5_000);
    });
    expect(getDashboard).toHaveBeenCalledTimes(2);
  });

  it("keeps valid live data through one transient poll failure", async () => {
    vi.useFakeTimers();
    vi.spyOn(sanymarService, "getDashboard")
      .mockResolvedValueOnce(liveDashboard)
      .mockRejectedValue({
        message: "provider operation failed: provider is unavailable",
      });
    const { result } = renderHook(() => useDashboard());

    await act(async () => {
      await Promise.resolve();
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5_000);
    });

    expect(result.current.dashboard).toEqual(liveDashboard);
    expect(result.current.error).toBeNull();
  });
});
