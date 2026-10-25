// SVG port of the overlay cat (src/cat.rs), so the window and the pill are the same character.

type Mood = "awake" | "asleep" | "happy";

export function Cat({ size = 32, mood = "awake" }: { size?: number; mood?: Mood }) {
  const eye = (x: number) =>
    mood === "awake" ? (
      <g>
        <ellipse cx={x} cy="18.6" rx="2.7" ry="3" fill="var(--cat-ink)" />
        <circle cx={x + 0.7} cy="17.5" r="1" fill="#fff" />
      </g>
    ) : mood === "happy" ? (
      <path d={`M${x - 2.6} 19.6 Q${x} 16.4 ${x + 2.6} 19.6`} fill="none" stroke="var(--cat-ink)" stroke-width="1.6" stroke-linecap="round" />
    ) : (
      <path d={`M${x - 2.6} 18 Q${x} 20.6 ${x + 2.6} 18`} fill="none" stroke="var(--cat-ink)" stroke-width="1.5" stroke-linecap="round" />
    );
  return (
    <svg width={size} height={size} viewBox="-1 -1 34 34" aria-hidden="true" class="cat">
      <path d="M3.6 14.4 L4.9 1.8 L13.5 14.4 Z M28.4 14.4 L27.1 1.8 L18.5 14.4 Z" fill="var(--fur)" />
      <path d="M6.1 13.5 L5.4 5.1 L11 13.5 Z M25.9 13.5 L26.6 5.1 L21 13.5 Z" fill="var(--ear)" />
      <ellipse cx="16" cy="19" rx="12.4" ry="10.5" fill="var(--fur)" />
      <path d="M13 9.4 V12.7 M16 9.4 V12.7 M19 9.4 V12.7" stroke="var(--fur-dark)" stroke-width="1.4" stroke-linecap="round" />
      {eye(10.6)}
      {eye(21.4)}
      <path d="M14.7 21.6 H17.3 L16 23 Z" fill="var(--ear)" />
      <path d="M14.1 22.4 a0.95 0.95 0 0 0 1.9 0 a0.95 0.95 0 0 0 1.9 0" fill="none" stroke="var(--cat-ink)" stroke-width="1.1" stroke-linecap="round" />
      <path d="M7 21.2 L0.6 20 M7 23.6 L0.6 24.8 M25 21.2 L31.4 20 M25 23.6 L31.4 24.8" stroke="var(--whisker)" stroke-width="0.8" stroke-linecap="round" />
    </svg>
  );
}
