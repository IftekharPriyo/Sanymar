interface ServiceStatusProps {
  name: string;
  detail: string;
}

export function ServiceStatus({ name, detail }: ServiceStatusProps) {
  return (
    <div className="service-row">
      <span className="service-dot" aria-hidden="true" />
      <div>
        <strong>{name}</strong>
        <span>{detail}</span>
      </div>
    </div>
  );
}
