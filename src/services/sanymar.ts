import { invoke } from "@tauri-apps/api/core";
import type {
  AppSettings,
  Dashboard,
  DjProfile,
  GeneratedSegment,
  OllamaStatus,
  SpeechResult,
  SpotifyConnectionStatus,
  TtsStatus,
} from "../types/domain";

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown;
  }
}

const profile: DjProfile = {
  id: "mira-vale",
  name: "Mira Vale",
  stationName: "Night Current",
  personalityTraits: ["curious", "warm", "observant"],
  energyLevel: 4,
  humourStyle: "lightly dry, never cruel",
  formality: 2,
  preferredLanguage: "English",
  banglaEnglishMix: 1,
  averageWords: 26,
  minimumWords: 8,
  maximumWords: 42,
  talkFrequency: 0.55,
  restrictedSubjects: ["private listener assumptions"],
  disallowedPhrases: ["Did you know", "without further ado", "coming up next"],
  stationLore: [
    "The Night Current studio sits above a tea shop that never closes.",
  ],
  runningJokes: [],
  addressesListener: true,
  reactsToTimeOfDay: true,
  mildSarcasm: true,
};

let browserScript: string | null = null;
let browserSettings: AppSettings = {
  mockMode: true,
  spotifyClientId: null,
  spotifyRedirectUri: "http://127.0.0.1:43821/callback",
  ollamaBaseUrl: "http://127.0.0.1:11434",
  ollamaModel: null,
  useOllama: false,
  djProfileId: "mira-vale",
  talkFrequency: "normal",
  maximumSegmentWords: 42,
  musicbrainzContact: null,
  cacheRetentionDays: 90,
  debugLogging: false,
  automaticTransitionSpeech: false,
  ttsProvider: "mock",
  ttsModelDirectory: null,
  ttsVoiceId: 0,
  ttsSpeedPercent: 100,
  parlerBaseUrl: "http://127.0.0.1:43822",
  parlerSpeaker: "Jon",
  audioOutputDevice: null,
};

function isTauri(): boolean {
  return (
    typeof window !== "undefined" && window.__TAURI_INTERNALS__ !== undefined
  );
}

const tracks = {
  currentTrack: {
    providerId: "mock-current-001",
    title: "Glass Satellites",
    artists: [{ providerId: "mock-artist-01", name: "Harbour Static" }],
    album: {
      providerId: "mock-album-01",
      title: "Signals After Rain",
      releaseDate: "2024-10-18",
      artworkUrl: null,
    },
    durationMs: 238_000,
    isrc: null,
    releaseDate: "2024-10-18",
    explicit: false,
    variant: "studio" as const,
    artworkUrl: null,
  },
  nextTrack: {
    providerId: "mock-next-002",
    title: "Lanterns on the Flyover",
    artists: [
      { providerId: "mock-artist-02", name: "June Meridian" },
      { providerId: "mock-artist-03", name: "Tariq North" },
    ],
    album: {
      providerId: "mock-album-02",
      title: "City Weather",
      releaseDate: null,
      artworkUrl: null,
    },
    durationMs: 204_000,
    isrc: null,
    releaseDate: null,
    explicit: false,
    variant: "unknown" as const,
    artworkUrl: null,
  },
};

export const sanymarService = {
  async getDashboard(): Promise<Dashboard> {
    if (isTauri()) return invoke<Dashboard>("get_dashboard");
    return {
      mockMode: true,
      llmMockMode: !browserSettings.useOllama,
      ttsMockMode: true,
      connectionStatus: "Connected to browser mock",
      currentProvider: "Spotify (mock)",
      playback: {
        ...tracks,
        progressMs: 184_000,
        isPlaying: true,
        device: null,
      },
      broadcastState: browserScript ? "Waiting for transition" : "Idle",
      djProfile: { ...profile },
      talkFrequency: "Normal",
      llmStatus: "Mock generator ready (Ollama not connected)",
      ttsStatus: "Mock TTS ready (no audio generated)",
      recentScript: browserScript,
    };
  },

  async generateTestSegment(): Promise<GeneratedSegment> {
    if (isTauri()) return invoke<GeneratedSegment>("generate_test_segment");
    browserScript =
      "That bassline left just enough room for the streetlights to get involved. Next, Lanterns on the Flyover.";
    return {
      jobId: crypto.randomUUID(),
      dialogue: browserScript,
      segmentType: "one_line_reaction",
      broadcastState: "Waiting for transition",
      isMock: true,
    };
  },

  async speakTestSegment(): Promise<SpeechResult> {
    if (isTauri()) return invoke<SpeechResult>("speak_test_segment");
    if (!browserScript) throw new Error("Generate a test segment first.");
    return {
      artifactId: crypto.randomUUID(),
      durationMs: 4_200,
      isMock: true,
      message: "Mock speech completed; no sound was produced.",
    };
  },

  async getSettings(): Promise<AppSettings> {
    if (isTauri()) return invoke<AppSettings>("get_settings");
    return { ...browserSettings };
  },

  async saveSettings(settings: AppSettings): Promise<AppSettings> {
    if (isTauri()) return invoke<AppSettings>("save_settings", { settings });
    browserSettings = { ...settings };
    return { ...browserSettings };
  },

  async getSpotifyConnection(): Promise<SpotifyConnectionStatus> {
    if (isTauri())
      return invoke<SpotifyConnectionStatus>("get_spotify_connection");
    return {
      configured: Boolean(browserSettings.spotifyClientId),
      connected: false,
      expiresAt: null,
      grantedScopes: [],
    };
  },

  async getOllamaStatus(): Promise<OllamaStatus> {
    if (isTauri()) return invoke<OllamaStatus>("get_ollama_status");
    return {
      configured: Boolean(browserSettings.ollamaModel),
      health: null,
      message: "Ollama health checks are available only in the desktop app.",
    };
  },

  async getTtsStatus(): Promise<TtsStatus> {
    if (isTauri()) return invoke<TtsStatus>("get_tts_status");
    return {
      configured: false,
      health: null,
      message: "Mock TTS is active in the browser preview.",
    };
  },

  async connectSpotify(): Promise<SpotifyConnectionStatus> {
    if (isTauri()) return invoke<SpotifyConnectionStatus>("connect_spotify");
    throw new Error("Spotify connection is available only in the desktop app.");
  },

  async disconnectSpotify(): Promise<SpotifyConnectionStatus> {
    if (isTauri()) return invoke<SpotifyConnectionStatus>("disconnect_spotify");
    return {
      configured: Boolean(browserSettings.spotifyClientId),
      connected: false,
      expiresAt: null,
      grantedScopes: [],
    };
  },
};
