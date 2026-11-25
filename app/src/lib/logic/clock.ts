/**
 * The app's "now". The mock sets an offset so the fixture's time (Wed 25 Nov 10:44) is now,
 * which keeps times and screenshots stable.
 */
let offset = 0;

export function setClockOffset(ms: number): void {
  offset = ms;
}

export function now(): number {
  return Date.now() + offset;
}
