export type TrackVariant =
  "studio" | "live" | "remix" | "acoustic" | "remaster" | "unknown";

export interface Artist {
  providerId: string | null;
  name: string;
}

export interface Album {
  providerId: string | null;
  title: string;
  releaseDate: string | null;
  artworkUrl: string | null;
}

export interface Track {
  providerId: string;
  title: string;
  artists: Artist[];
  album: Album | null;
  durationMs: number;
  isrc: string | null;
  releaseDate: string | null;
  explicit: boolean;
  variant: TrackVariant;
  artworkUrl: string | null;
}

export interface PlaybackState {
  currentTrack: Track | null;
  nextTrack: Track | null;
  progressMs: number;
  isPlaying: boolean;
  device: unknown | null;
}

export interface DjProfile {
  id: string;
  name: string;
  stationName: string;
  personalityTraits: string[];
  energyLevel: number;
  humourStyle: string;
  formality: number;
  preferredLanguage: string;
  banglaEnglishMix: number;
  averageWords: number;
  minimumWords: number;
  maximumWords: number;
  talkFrequency: number;
  restrictedSubjects: string[];
  disallowedPhrases: string[];
  stationLore: string[];
  runningJokes: string[];
  addressesListener: boolean;
  reactsToTimeOfDay: boolean;
  mildSarcasm: boolean;
}

export interface Dashboard {
  mockMode: boolean;
  llmMockMode: boolean;
  ttsMockMode: boolean;
  connectionStatus: string;
  currentProvider: string;
  playback: PlaybackState;
  broadcastState: string;
  djProfile: DjProfile;
  talkFrequency: string;
  llmStatus: string;
  ttsStatus: string;
  recentScript: string | null;
}

export interface GeneratedSegment {
  jobId: string;
  dialogue: string | null;
  segmentType: string;
  broadcastState: string;
  isMock: boolean;
}

export interface SpeechResult {
  artifactId: string;
  durationMs: number | null;
  isMock: boolean;
  message: string;
}

export interface SpotifyConnectionStatus {
  configured: boolean;
  connected: boolean;
  expiresAt: string | null;
  grantedScopes: string[];
}

export type TalkFrequency = "minimal" | "normal" | "talkative";
export type TtsProvider = "mock" | "sherpa_kokoro" | "parler_mini";

export interface AppSettings {
  mockMode: boolean;
  spotifyClientId: string | null;
  spotifyRedirectUri: string;
  ollamaBaseUrl: string;
  ollamaModel: string | null;
  useOllama: boolean;
  djProfileId: string;
  talkFrequency: TalkFrequency;
  maximumSegmentWords: number;
  musicbrainzContact: string | null;
  cacheRetentionDays: number;
  debugLogging: boolean;
  automaticTransitionSpeech: boolean;
  ttsProvider: TtsProvider;
  ttsModelDirectory: string | null;
  ttsVoiceId: number;
  ttsSpeedPercent: number;
  parlerBaseUrl: string;
  parlerSpeaker: string;
  audioOutputDevice: string | null;
}

export interface OllamaHealth {
  reachable: boolean;
  modelConfigured: boolean;
  modelInstalled: boolean;
  model: string;
  version: string | null;
}

export interface OllamaStatus {
  configured: boolean;
  health: OllamaHealth | null;
  message: string;
}

export interface TtsHealth {
  ready: boolean;
  provider: string;
  sampleRate: number | null;
  availableVoices: number | null;
  model: string | null;
  speaker: string | null;
}

export interface TtsStatus {
  configured: boolean;
  health: TtsHealth | null;
  message: string;
}
