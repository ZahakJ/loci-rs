/**
 * Wraps an async action so that calls made while one is still running are ignored (not queued).
 * The Add sheet uses it for Ctrl+Enter / the Add button: a key repeat, a double click or two
 * handlers reacting to one keystroke can never add the same note twice.
 */
export interface SingleFlight<A extends unknown[]> {
  /** Runs `fn` unless a previous run has not settled; resolves true when it ran. */
  (...args: A): Promise<boolean>;
  /** True while a run is in progress. */
  readonly pending: boolean;
}

export function singleFlight<A extends unknown[]>(fn: (...args: A) => unknown): SingleFlight<A> {
  let running = false;
  const call = async (...args: A): Promise<boolean> => {
    if (running) return false;
    running = true;
    try { await fn(...args); return true; } finally { running = false; }
  };
  Object.defineProperty(call, 'pending', { get: () => running });
  return call as SingleFlight<A>;
}
