import { invoke, isTauri } from '@tauri-apps/api/core';
import type { Command, Snapshot } from './protocol';
export const desktop = isTauri();
export async function connect(): Promise<Snapshot> {
  if (!desktop) throw new Error('Open the installed desktop app to connect to the Rust service.');
  return invoke('connect_service');
}
export async function command<T = unknown>(command: Command): Promise<T> { return invoke('service_request', { command }); }
export { invoke };
export function bytes(value: number | null): string {
  if (value === null) return 'Unknown';
  if (value === 0) return '0 B';
  const unit = Math.min(Math.floor(Math.log(value) / Math.log(1024)), 4);
  return `${(value / 1024 ** unit).toFixed(unit === 0 ? 0 : 1)} ${['B', 'KiB', 'MiB', 'GiB', 'TiB'][unit]}`;
}
export function duration(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return '—';
  if (seconds < 60) return `${Math.ceil(seconds)}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ${Math.ceil(seconds % 60)}s`;
  return `${Math.floor(seconds / 3600)}h ${Math.floor(seconds % 3600 / 60)}m`;
}
