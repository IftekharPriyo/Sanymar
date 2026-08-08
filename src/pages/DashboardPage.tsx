import { ServiceStatus } from "../components/ServiceStatus";
import { StatusPill } from "../components/StatusPill";
import { TrackCard } from "../components/TrackCard";
import { useDashboard } from "../hooks/useDashboard";

function currentGreeting(date = new Date()): string {
  const hour = date.getHours();
  if (hour < 12) return "Good morning.";
  if (hour < 18) return "Good afternoon.";
  return "Good evening.";
}

export function DashboardPage() {
  const { dashboard, busy, notice, error, generate, speak } = useDashboard();
  const greeting = currentGreeting();

  if (!dashboard)
    return <main className="loading">{error ?? "Tuning The Swell..."}</main>;

  return (
    <main className="dashboard">
      <section className="hero-row">
        <div>
          <p className="eyebrow">The Swell</p>
          <h1>{greeting} Sanymar is listening.</h1>
          <p className="lede">
            Your personal radio host watches the queue, prepares a short
            handoff, and speaks between songs.
          </p>
        </div>
        <div className="badges">
          {dashboard.mockMode && (
            <StatusPill label="Demo playback data" tone="warning" />
          )}
          {dashboard.llmMockMode && (
            <StatusPill label="Test dialogue engine" tone="warning" />
          )}
          {!dashboard.llmMockMode && (
            <StatusPill label="Groq Qwen dialogue" tone="good" />
          )}
          {dashboard.ttsMockMode ? (
            <StatusPill label="Silent voice preview" tone="warning" />
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
                ? "Working..."
                : dashboard.llmMockMode
                  ? "Generate test segment"
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
            Dialogue:{" "}
            {dashboard.llmMockMode ? "test engine" : dashboard.llmStatus}.{" "}
            {dashboard.ttsMockMode
              ? "Voice playback is silent in this preview mode."
              : "Kokoro generates and plays the RJ voice through the default device."}
          </p>
        </article>

        <aside className="panel status-panel">
          <p className="eyebrow">Studio rack</p>
          <ServiceStatus name="Music" detail={dashboard.currentProvider} />
          <ServiceStatus name="Dialogue model" detail={dashboard.llmStatus} />
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
