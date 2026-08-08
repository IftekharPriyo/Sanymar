import { ServiceStatus } from "../components/ServiceStatus";
import { StatusPill } from "../components/StatusPill";
import { TrackCard } from "../components/TrackCard";
import { useDashboard } from "../hooks/useDashboard";

export function DashboardPage() {
  const { dashboard, busy, notice, error, generate, speak } = useDashboard();

  if (!dashboard)
    return <main className="loading">{error ?? "Tuning the studio…"}</main>;

  return (
    <main className="dashboard">
      <section className="hero-row">
        <div>
          <p className="eyebrow">Personal radio, locally directed</p>
          <h1>Good evening. The booth is listening.</h1>
          <p className="lede">
            A reviewable mock broadcast chain—ready before any account is
            connected.
          </p>
        </div>
        <div className="badges">
          {dashboard.mockMode && (
            <StatusPill label="Mock Spotify" tone="warning" />
          )}
          {dashboard.llmMockMode && (
            <StatusPill label="Mock script generator" tone="warning" />
          )}
          {!dashboard.llmMockMode && (
            <StatusPill label="Real script model" tone="good" />
          )}
          {dashboard.ttsMockMode ? (
            <StatusPill label="Mock TTS" tone="warning" />
          ) : (
            <StatusPill label="Kokoro WAV" tone="good" />
          )}
          <StatusPill label={dashboard.connectionStatus} tone="good" />
        </div>
      </section>

      {error && <div className="message error">{error}</div>}
      {notice && <div className="message notice">{notice}</div>}

      <section className="track-grid" aria-label="Playback queue">
        <TrackCard
          eyebrow="On air now"
          track={dashboard.playback.currentTrack}
          active
        />
        <TrackCard
          eyebrow="In the wings"
          track={dashboard.playback.nextTrack}
        />
      </section>

      <section className="lower-grid">
        <article className="panel script-panel">
          <div className="panel-heading">
            <div>
              <p className="eyebrow">Latest RJ copy</p>
              <h2>
                {dashboard.djProfile.name} · {dashboard.djProfile.stationName}
              </h2>
            </div>
            <StatusPill label={dashboard.broadcastState} />
          </div>
          <blockquote>
            {dashboard.recentScript ??
              "The microphone is quiet. Generate a test segment when you are ready."}
          </blockquote>
          <div className="actions">
            <button
              className="primary"
              disabled={busy}
              onClick={() => void generate()}
            >
              {busy
                ? "Working…"
                : dashboard.llmMockMode
                  ? "Generate mock segment"
                  : "Generate with model"}
            </button>
            <button
              disabled={busy || !dashboard.recentScript}
              onClick={() => void speak()}
            >
              Speak test segment
            </button>
          </div>
          <p className="fine-print">
            Script mode: {dashboard.llmMockMode ? "mock" : dashboard.llmStatus}.{" "}
            {dashboard.ttsMockMode
              ? "Speech synthesis and playback are simulated."
              : "Kokoro generates a real WAV; device playback is still simulated."}
          </p>
        </article>

        <aside className="panel status-panel">
          <p className="eyebrow">Studio rack</p>
          <ServiceStatus name="Provider" detail={dashboard.currentProvider} />
          <ServiceStatus name="Local LLM" detail={dashboard.llmStatus} />
          <ServiceStatus name="Voice" detail={dashboard.ttsStatus} />
          <ServiceStatus
            name="Talk frequency"
            detail={dashboard.talkFrequency}
          />
        </aside>
      </section>
    </main>
  );
}
