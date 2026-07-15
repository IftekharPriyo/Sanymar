import { useEffect, useState } from "react";
import { sanymarService } from "../services/sanymar";
import type {
  AppSettings,
  SpotifyConnectionStatus,
  OllamaStatus,
  TalkFrequency,
  TtsProvider,
  TtsStatus,
} from "../types/domain";

function errorMessage(reason: unknown): string {
  if (reason instanceof Error) return reason.message;
  if (
    typeof reason === "object" &&
    reason !== null &&
    "message" in reason &&
    typeof reason.message === "string"
  )
    return reason.message;
  return "The provider operation failed.";
}

export function SettingsPage() {
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [spotify, setSpotify] = useState<SpotifyConnectionStatus | null>(null);
  const [spotifyBusy, setSpotifyBusy] = useState(false);
  const [ollama, setOllama] = useState<OllamaStatus | null>(null);
  const [ollamaBusy, setOllamaBusy] = useState(false);
  const [tts, setTts] = useState<TtsStatus | null>(null);
  const [ttsBusy, setTtsBusy] = useState(false);

  useEffect(() => {
    void sanymarService.getSettings().then(setSettings);
    void sanymarService.getSpotifyConnection().then(setSpotify);
  }, []);

  if (!settings)
    return <main className="loading">Loading local settings…</main>;

  const save = async () => {
    try {
      const saved = await sanymarService.saveSettings(settings);
      setSettings(saved);
      setMessage("Settings saved locally.");
      setError(null);
      return saved;
    } catch (reason) {
      setError(errorMessage(reason));
      return null;
    }
  };

  const connectSpotify = async () => {
    setSpotifyBusy(true);
    setMessage(null);
    try {
      const saved = await save();
      if (!saved) return;
      setSpotify(await sanymarService.connectSpotify());
      setMessage(
        "Spotify connected. Tokens are stored in Windows Credential Manager.",
      );
      setError(null);
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setSpotifyBusy(false);
    }
  };

  const disconnectSpotify = async () => {
    setSpotifyBusy(true);
    try {
      setSpotify(await sanymarService.disconnectSpotify());
      setSettings((current) =>
        current ? { ...current, mockMode: true } : current,
      );
      setMessage("Spotify disconnected and its stored tokens were removed.");
      setError(null);
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setSpotifyBusy(false);
    }
  };

  const checkOllama = async () => {
    setOllamaBusy(true);
    setMessage(null);
    try {
      const saved = await save();
      if (!saved) return;
      const status = await sanymarService.getOllamaStatus();
      setOllama(status);
      setMessage(status.message);
      setError(null);
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setOllamaBusy(false);
    }
  };

  const checkTts = async () => {
    setTtsBusy(true);
    setMessage(null);
    try {
      const saved = await save();
      if (!saved) return;
      const status = await sanymarService.getTtsStatus();
      setTts(status);
      setMessage(status.message);
      setError(null);
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setTtsBusy(false);
    }
  };

  return (
    <main className="settings-page">
      <p className="eyebrow">Local configuration</p>
      <h1>Studio settings</h1>
      <p className="lede">
        OAuth tokens are never displayed or stored in this form.
      </p>
      {error && <div className="message error settings-message">{error}</div>}
      <section className="panel settings-form spotify-settings">
        <div className="panel-heading">
          <div>
            <p className="eyebrow">Music provider</p>
            <h2>Spotify connection</h2>
          </div>
          <span
            className={
              spotify?.connected ? "connection-good" : "connection-muted"
            }
          >
            {spotify?.connected ? "Connected" : "Not connected"}
          </span>
        </div>
        <label>
          Spotify Client ID
          <input
            autoComplete="off"
            value={settings.spotifyClientId ?? ""}
            placeholder="Paste the Client ID from Spotify for Developers"
            onChange={(event) =>
              setSettings({
                ...settings,
                spotifyClientId: event.target.value.trim() || null,
              })
            }
          />
        </label>
        <label>
          Registered redirect URI
          <input value={settings.spotifyRedirectUri} readOnly />
        </label>
        <p className="fine-print">
          The Client ID is not secret. Do not enter a Spotify Client Secret.
          Authorization uses PKCE, and tokens are kept in Windows Credential
          Manager.
        </p>
        <label className="checkbox-row">
          <input
            type="checkbox"
            checked={!settings.mockMode}
            disabled={!spotify?.connected}
            onChange={(event) =>
              setSettings({ ...settings, mockMode: !event.target.checked })
            }
          />
          Use live Spotify playback on the dashboard
        </label>
        <div className="actions">
          <button
            className="primary"
            disabled={spotifyBusy || !settings.spotifyClientId}
            onClick={() => void connectSpotify()}
          >
            {spotifyBusy ? "Waiting for Spotify…" : "Connect Spotify"}
          </button>
          {spotify?.connected && (
            <button
              disabled={spotifyBusy}
              onClick={() => void disconnectSpotify()}
            >
              Disconnect Spotify
            </button>
          )}
        </div>
      </section>
      <section className="panel settings-form">
        <label className="checkbox-row">
          <input
            type="checkbox"
            checked={settings.automaticTransitionSpeech}
            disabled={settings.mockMode}
            onChange={(event) =>
              setSettings({
                ...settings,
                automaticTransitionSpeech: event.target.checked,
              })
            }
          />
          Automatically prepare and play speech at Spotify transitions
        </label>
        <p className="fine-print">
          Sanymar prepares dialogue and voice when the current and next songs
          are known, then plays it over the closing seconds. Automatic mode
          speaks at every stable transition; Spotify pause and resume are not
          controlled yet.
        </p>
        <label>
          Talk frequency
          <select
            value={settings.talkFrequency}
            onChange={(event) =>
              setSettings({
                ...settings,
                talkFrequency: event.target.value as TalkFrequency,
              })
            }
          >
            <option value="minimal">Minimal</option>
            <option value="normal">Normal</option>
            <option value="talkative">Talkative</option>
          </select>
        </label>
        <label>
          Maximum segment words
          <input
            type="number"
            min="1"
            max="150"
            value={settings.maximumSegmentWords}
            onChange={(event) =>
              setSettings({
                ...settings,
                maximumSegmentWords: Number(event.target.value),
              })
            }
          />
        </label>
        <label>
          MusicBrainz contact
          <input
            autoComplete="email"
            value={settings.musicbrainzContact ?? ""}
            placeholder="Email address or HTTPS contact URL"
            onChange={(event) =>
              setSettings({
                ...settings,
                musicbrainzContact: event.target.value.trim() || null,
              })
            }
          />
        </label>
        <p className="fine-print">
          Used only in MusicBrainz's required identifying User-Agent. Live
          lookups are automatic, cached, and skipped when this is empty.
        </p>
      </section>
      <section className="panel settings-form ollama-settings">
        <div className="panel-heading">
          <div>
            <p className="eyebrow">Script generator</p>
            <h2>Local Ollama</h2>
          </div>
          <span
            className={
              ollama?.health?.modelInstalled
                ? "connection-good"
                : "connection-muted"
            }
          >
            {ollama?.health?.modelInstalled ? "Ready" : "Not checked"}
          </span>
        </div>
        <label>
          Ollama base URL
          <input
            value={settings.ollamaBaseUrl}
            onChange={(event) =>
              setSettings({ ...settings, ollamaBaseUrl: event.target.value })
            }
          />
        </label>
        <label>
          Ollama model
          <input
            value={settings.ollamaModel ?? ""}
            placeholder="Not selected"
            onChange={(event) =>
              setSettings({
                ...settings,
                ollamaModel: event.target.value || null,
              })
            }
          />
        </label>
        <p className="fine-print">
          Sanymar connects only to a local loopback URL. It will never download
          or install a model for you.
        </p>
        <label className="checkbox-row">
          <input
            type="checkbox"
            checked={settings.useOllama}
            onChange={(event) =>
              setSettings({ ...settings, useOllama: event.target.checked })
            }
          />
          Use real local Ollama instead of the mock script generator
        </label>
        <div className="actions">
          <button
            disabled={ollamaBusy || !settings.ollamaModel}
            onClick={() => void checkOllama()}
          >
            {ollamaBusy ? "Checking Ollama..." : "Check Ollama"}
          </button>
        </div>
      </section>
      <section className="panel settings-form">
        <div className="panel-heading">
          <div>
            <p className="eyebrow">Voice synthesis</p>
            <h2>Local English voice</h2>
          </div>
          <span
            className={
              tts?.health?.ready ? "connection-good" : "connection-muted"
            }
          >
            {tts?.health?.ready ? "Ready" : "Not checked"}
          </span>
        </div>
        <label>
          TTS provider
          <select
            value={settings.ttsProvider}
            onChange={(event) =>
              setSettings({
                ...settings,
                ttsProvider: event.target.value as TtsProvider,
              })
            }
          >
            <option value="mock">Mock TTS</option>
            <option value="sherpa_kokoro">Sherpa-ONNX Kokoro</option>
            <option value="parler_mini">Parler-TTS Mini local service</option>
          </select>
        </label>
        {settings.ttsProvider === "sherpa_kokoro" && (
          <>
            <label>
              Kokoro model directory
              <input
                value={settings.ttsModelDirectory ?? ""}
                placeholder="Absolute path containing model.onnx and voices.bin"
                onChange={(event) =>
                  setSettings({
                    ...settings,
                    ttsModelDirectory: event.target.value.trim() || null,
                  })
                }
              />
            </label>
            <label>
              Kokoro voice ID
              <input
                type="number"
                min="0"
                value={settings.ttsVoiceId}
                onChange={(event) =>
                  setSettings({
                    ...settings,
                    ttsVoiceId: Number(event.target.value),
                  })
                }
              />
            </label>
          </>
        )}
        {settings.ttsProvider === "parler_mini" && (
          <>
            <label>
              Parler local service URL
              <input
                value={settings.parlerBaseUrl}
                onChange={(event) =>
                  setSettings({
                    ...settings,
                    parlerBaseUrl: event.target.value,
                  })
                }
              />
            </label>
            <label>
              Parler speaker
              <select
                value={settings.parlerSpeaker}
                onChange={(event) =>
                  setSettings({
                    ...settings,
                    parlerSpeaker: event.target.value,
                  })
                }
              >
                <option value="Jon">Jon</option>
                <option value="Gary">Gary</option>
                <option value="Mike">Mike</option>
                <option value="Lea">Lea</option>
                <option value="Jenna">Jenna</option>
              </select>
            </label>
          </>
        )}
        <label>
          Speech speed ({settings.ttsSpeedPercent}%)
          <input
            type="range"
            min="50"
            max="200"
            value={settings.ttsSpeedPercent}
            onChange={(event) =>
              setSettings({
                ...settings,
                ttsSpeedPercent: Number(event.target.value),
              })
            }
          />
        </label>
        <p className="fine-print">
          {settings.ttsProvider === "parler_mini"
            ? "The manually started loopback service keeps the user-installed Parler model loaded. No reference audio is accepted."
            : "The Kokoro directory must contain model.onnx, voices.bin, tokens.txt, and espeak-ng-data."}{" "}
          Sanymar never downloads voice models.
          {tts?.health?.availableVoices != null &&
            tts.health.sampleRate != null &&
            " Provider reports " +
              tts.health.availableVoices +
              " voices at " +
              tts.health.sampleRate +
              " Hz."}
        </p>
        <button
          disabled={
            ttsBusy ||
            settings.ttsProvider === "mock" ||
            (settings.ttsProvider === "sherpa_kokoro" &&
              !settings.ttsModelDirectory) ||
            (settings.ttsProvider === "parler_mini" && !settings.parlerBaseUrl)
          }
          onClick={() => void checkTts()}
        >
          {ttsBusy ? "Checking voice provider..." : "Check voice provider"}
        </button>
      </section>
      <section className="panel settings-form">
        <label className="checkbox-row">
          <input
            type="checkbox"
            checked={settings.debugLogging}
            onChange={(event) =>
              setSettings({ ...settings, debugLogging: event.target.checked })
            }
          />
          Enable development debug logging
        </label>
        <button className="primary" onClick={() => void save()}>
          Save settings
        </button>
        {message && <span className="saved-message">{message}</span>}
      </section>
    </main>
  );
}
