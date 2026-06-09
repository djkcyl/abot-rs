const TOKEN_KEY = 'abot_token'
const AUTHORITY_KEY = 'abot_authority'

export function getToken(): string | null {
  return localStorage.getItem(TOKEN_KEY)
}

export function setToken(token: string, authority: number) {
  localStorage.setItem(TOKEN_KEY, token)
  localStorage.setItem(AUTHORITY_KEY, String(authority))
}

export function clearToken() {
  localStorage.removeItem(TOKEN_KEY)
  localStorage.removeItem(AUTHORITY_KEY)
}

export function getAuthority(): number {
  return Number(localStorage.getItem(AUTHORITY_KEY) ?? '0')
}

export interface ChallengeResult {
  code: string
  hint: string
}

export async function challenge(): Promise<ChallengeResult> {
  const resp = await fetch('/api/login/challenge', { method: 'POST' })
  if (!resp.ok) {
    throw new Error(`challenge 失败: ${resp.status}`)
  }
  return resp.json() as Promise<ChallengeResult>
}

export interface PollResult {
  token: string | null
  authority?: number
}

export async function poll(code: string): Promise<PollResult> {
  const resp = await fetch('/api/login/poll', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ code }),
  })
  if (!resp.ok) {
    throw new Error(`poll 失败: ${resp.status}`)
  }
  return resp.json() as Promise<PollResult>
}
