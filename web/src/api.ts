export interface ApiError {
  code: string
  message: string
}

async function handle<T>(r: Response): Promise<T> {
  if (r.status === 401) {
    window.location.href = '/login'
    throw new Error('unauthorized')
  }
  if (!r.ok) {
    const body = await r.json().catch(() => null)
    const msg = body?.error?.message ?? `${r.status} ${r.statusText}`
    throw new Error(msg)
  }
  return r.json()
}

export const apiGet = <T>(path: string): Promise<T> =>
  fetch(path, { credentials: 'include' }).then((r) => handle<T>(r))

export const apiPost = <T>(path: string, body: unknown): Promise<T> =>
  fetch(path, {
    method: 'POST',
    credentials: 'include',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body),
  }).then((r) => handle<T>(r))

export const apiPut = <T>(path: string, body: unknown): Promise<T> =>
  fetch(path, {
    method: 'PUT',
    credentials: 'include',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body),
  }).then((r) => handle<T>(r))
