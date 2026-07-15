import type { Track } from "../types/domain";

interface TrackCardProps {
  eyebrow: string;
  track: Track | null;
  active?: boolean;
}

export function TrackCard({ eyebrow, track, active = false }: TrackCardProps) {
  if (!track) {
    return (
      <article className="track-card empty">
        <p className="eyebrow">{eyebrow}</p>
        <h3>No track available</h3>
      </article>
    );
  }

  return (
    <article className={`track-card ${active ? "active" : ""}`}>
      <div className="mock-artwork" aria-hidden="true">
        {track.title.slice(0, 1)}
      </div>
      <div>
        <p className="eyebrow">{eyebrow}</p>
        <h3>{track.title}</h3>
        <p className="artist-line">
          {track.artists.map((artist) => artist.name).join(" · ")}
        </p>
        <p className="track-meta">
          {track.album?.title ?? "Album unavailable"} · {track.variant}
        </p>
      </div>
    </article>
  );
}
