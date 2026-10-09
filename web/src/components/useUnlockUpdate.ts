import { useEffect, useRef, useState } from 'react'
import {
  checkingHoldMs,
  readDevUnlockUpdatePreview,
  readDevUpdateNotice,
  reduceUpdate,
  takeStartupUpdateNotice,
  type InstallProgress,
  type ParsedIpcUpdate,
  type UpdateUiState,
} from '../lib/updateCheck'
import {
  updateCancel,
  updateCheck,
  updateInstall,
  updateTakeNotice,
  type InstallCommandResult,
  type UpdateNotice,
} from '../lib/tauri'

const IDLE: UpdateUiState = { kind: 'idle' }

type SetUpdate = (update: UpdateUiState | ((state: UpdateUiState) => UpdateUiState)) => void

type Session = {
  generation: { current: number }
  current: () => UpdateUiState
  setUpdate: SetUpdate
}

function holdFor(ms: number): Promise<void> {
  if (ms <= 0) return Promise.resolve()
  return new Promise((resolve) => {
    window.setTimeout(resolve, ms)
  })
}

/** Checking stays up for the minimum even when the check itself was fast. */
async function publishCheck(
  session: Session,
  generation: number,
  startedAt: number,
  result: ParsedIpcUpdate,
): Promise<void> {
  if (generation !== session.generation.current) return
  await holdFor(checkingHoldMs(startedAt, Date.now()))
  if (generation !== session.generation.current) return
  session.setUpdate((state) => reduceUpdate(state, { type: 'checkResult', result }))
}

async function runCheck(session: Session): Promise<void> {
  const next = reduceUpdate(session.current(), { type: 'startCheck' })
  if (next === session.current()) return

  const generation = ++session.generation.current
  const startedAt = Date.now()
  session.setUpdate(next)
  try {
    const result = await updateCheck()
    await publishCheck(session, generation, startedAt, result)
  } catch {
    await publishCheck(session, generation, startedAt, { kind: 'failed' })
  }
}

/**
 * True means Rust aborted the download and is `available` again.
 * Bump the generation first so this install's later result cannot
 * overwrite a second press of Install.
 */
async function abortDownload(session: Session): Promise<void> {
  const aborted = await updateCancel()
  if (!aborted) return
  session.generation.current += 1
  session.setUpdate((state) => reduceUpdate(state, { type: 'downloadAborted' }))
}

async function dismissUpdate(session: Session): Promise<void> {
  const current = session.current()
  if (current.kind === 'downloading') {
    await abortDownload(session)
    return
  }

  const next = reduceUpdate(current, { type: 'dismiss' })
  if (next === current) return
  session.generation.current += 1
  session.setUpdate(next)
}

function applyProgress(session: Session, generation: number, progress: InstallProgress): void {
  if (generation !== session.generation.current) return
  session.setUpdate((state) => reduceUpdate(state, { type: 'progress', progress }))
}

function applyOutcome(
  session: Session,
  generation: number,
  outcome: InstallCommandResult | undefined,
): void {
  if (generation !== session.generation.current || outcome === undefined) return
  session.setUpdate((state) => reduceUpdate(state, { type: 'installResult', result: outcome }))
}

async function installAvailable(session: Session): Promise<void> {
  const current = session.current()
  if (current.kind !== 'available') return
  const next = reduceUpdate(current, { type: 'beginDownload' })
  if (next === current) return

  const generation = ++session.generation.current
  session.setUpdate(next)
  try {
    const outcome = await updateInstall(current, (progress) => {
      applyProgress(session, generation, progress)
    })
    applyOutcome(session, generation, outcome)
  } catch {
    applyOutcome(session, generation, { kind: 'failed' })
  }
}

function isUpdateBusy(state: UpdateUiState): boolean {
  return (
    state.kind === 'checking' || state.kind === 'downloading' || state.kind === 'installing'
  )
}

/** Dialog state for the unlock screen. One generation per check or install. */
export function useUnlockUpdate() {
  const [update, setUpdate] = useState<UpdateUiState>(
    () => readDevUnlockUpdatePreview() ?? IDLE,
  )
  const [notice, setNotice] = useState<UpdateNotice | null>(() => readDevUpdateNotice())
  const generation = useRef(0)
  const updateRef = useRef(update)

  useEffect(() => {
    updateRef.current = update
  }, [update])

  // A DEV preview paints the notice without reading the marker.
  useEffect(() => {
    if (readDevUpdateNotice()) return
    void takeStartupUpdateNotice(updateTakeNotice).then((next) => {
      if (next) setNotice(next)
    })
  }, [])

  const session: Session = {
    generation,
    current: () => updateRef.current,
    setUpdate,
  }

  return {
    update,
    notice,
    dismissNotice: () => setNotice(null),
    busy: isUpdateBusy(update),
    beginCheck: () => {
      void runCheck(session)
    },
    dismiss: () => {
      void dismissUpdate(session)
    },
    install: () => {
      void installAvailable(session)
    },
    resolvePoll: (result: ParsedIpcUpdate) => {
      setUpdate((state) => reduceUpdate(state, { type: 'pollResult', result }))
    },
    capPoll: () => {
      setUpdate((state) => reduceUpdate(state, { type: 'pollCap' }))
    },
  }
}
