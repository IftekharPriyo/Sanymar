interface StatusPillProps {
  label: string;
  tone?: "good" | "warning" | "neutral";
}

export function StatusPill({ label, tone = "neutral" }: StatusPillProps) {
  return <span className={`status-pill status-${tone}`}>{label}</span>;
}
