import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { Transport } from './transport';

/** The real transport: Tauri commands and events. Errors arrive as `AppError` objects. */
export const tauriTransport: Transport = {
  native: true,
  invoke: (command, args) => invoke(command, args as Record<string, unknown>),
  listen: (event, handler) => listen(event, (e) => handler(e.payload as never)),
};
