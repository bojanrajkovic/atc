/**
 * Format an ISO-8601 datetime string to a human-readable date and time.
 *
 * Output shape: "Apr 26, 2026, 2:32 PM" (en-US, dateStyle: 'medium', timeStyle: 'short').
 * Uses Intl.DateTimeFormat for locale-safe formatting.
 */
export function formatTimestamp(iso: string): string {
  return new Intl.DateTimeFormat('en-US', {
    dateStyle: 'medium',
    timeStyle: 'short',
  }).format(new Date(iso))
}
