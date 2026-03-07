interface TranscriptEntry {
  id: string;
  role: 'assistant' | 'user' | 'system';
  text: string;
  ts: string;
}

interface TranscriptPanelProps {
  entries: TranscriptEntry[];
}

export function TranscriptPanel({ entries }: TranscriptPanelProps): JSX.Element {
  return (
    <section className="transcript-panel" aria-label="Live transcript">
      <h2>Transcript</h2>
      <ul>
        {entries.slice(-8).map((entry) => (
          <li key={entry.id} className={`transcript-entry ${entry.role}`}>
            <span className="entry-role">{entry.role}</span>
            <p>{entry.text}</p>
            <time dateTime={entry.ts}>{new Date(entry.ts).toLocaleTimeString()}</time>
          </li>
        ))}
      </ul>
    </section>
  );
}
