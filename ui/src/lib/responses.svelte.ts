import { invoke } from '@tauri-apps/api/core';
import type { EventDetail } from './eventdetail';

type Response = 'accepted' | 'tentative' | 'declined';
type Request = { id: number; response: Response; scope: 'this' | 'all'; occurrenceStartMs: number };
type Job = Request & { promise: Promise<EventDetail> };
type Failure = { id: number; message: string };

// Owned by the app session, not by a popover: closing or switching events
// must not cancel an answer or allow the next write to overtake it.
let jobs = $state<Job[]>([]);
let failures = $state<Failure[]>([]);
let tail: Promise<unknown> = Promise.resolve();

export const pendingResponseCount = () => jobs.length;
export const responsePending = (id: number) => jobs.some(job => job.id === id);
export const responseFailures = () => failures;
export function dismissResponseFailure(id: number) {
  failures = failures.filter(failure => failure.id !== id);
}

export function pendingResponse(id: number, startMs: number): Response | undefined {
  for (let i = jobs.length - 1; i >= 0; i--) {
    const job = jobs[i];
    if (job.id === id && (job.scope === 'all' || job.occurrenceStartMs === startMs)) return job.response;
  }
}

/** Wait for the current batch, including replies added while a write waits. */
export async function responsesIdle() {
  let batch;
  do { batch = tail; await batch; } while (batch !== tail);
}

export function queueResponse(request: Request, title = 'Event'): Promise<EventDetail> {
  // A double click or the same action from two surfaces must not mail the
  // guests twice. Different answers are kept in the order the user chose.
  const previous = [...jobs].reverse().find(job => job.id === request.id
    && (job.scope === 'all' || request.scope === 'all'
      || job.occurrenceStartMs === request.occurrenceStartMs));
  if (previous?.scope === request.scope && previous.response === request.response) return previous.promise;
  dismissResponseFailure(request.id);

  const promise = tail.then(() => invoke<EventDetail>('respond_to_event', request));
  const job = { ...request, promise };
  jobs = [...jobs, job];
  // Resolve the queue tail on either outcome. A failed reply restores its
  // own UI, stays visible in the header, and never strands later replies.
  tail = promise.then(
    () => { jobs = jobs.filter(item => item.promise !== promise); },
    error => {
      jobs = jobs.filter(item => item.promise !== promise);
      failures = [...failures.filter(f => f.id !== request.id), {
        id: request.id, message: `${title}: could not save your response. ${String(error)}`,
      }];
    },
  );
  return promise;
}
