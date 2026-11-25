/*
  Toast wording, verb first: "Marked 144 done", "Moved 146 to In progress", "Dropped 144".
*/

import type { Status, TaskId } from '../types';
import { SHAPE_LABEL } from './views';

export function statusToast(id: TaskId, status: Status): string {
  if (status === 'done' || status === 'blocked') return `Marked ${id} ${status}`;
  if (status === 'dropped') return `Dropped ${id}`;
  return `Moved ${id} to ${SHAPE_LABEL[status]}`;
}
