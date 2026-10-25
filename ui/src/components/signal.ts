import { useEffect, useState } from "preact/hooks";

/** A tiny shared value that components can subscribe to. */
export function signal<T>(initial: T) {
  let value = initial;
  const subscribers = new Set<(v: T) => void>();
  return {
    get: () => value,
    set(next: T) {
      value = next;
      subscribers.forEach((s) => s(next));
    },
    use(): T {
      const [v, setV] = useState(value);
      useEffect(() => {
        subscribers.add(setV);
        setV(value);
        return () => void subscribers.delete(setV);
      }, []);
      return v;
    },
  };
}
