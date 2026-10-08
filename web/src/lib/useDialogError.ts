import { useCallback, useEffect, useRef, useState } from 'react'

type Stored = { dialog: string | null; text: string | null }

const NONE: Stored = { dialog: null, text: null }

/**
 * An error that belongs to whichever dialog is open, so it is drawn inside
 * that dialog instead of on the page behind its scrim. With no dialog open it
 * is the page's own error.
 *
 * `openDialog` names the dialog that is open now (null when none). Opening or
 * closing a dialog clears an error that was raised for another one, so no call
 * site needs cleanup code. A failure is shown where the person is looking when
 * it arrives: in the dialog open then, or on the page.
 *
 * `setMessage(text, dialog)` can address a dialog that is about to open, for an
 * error raised in the same step that opens it. `setMessage` keeps one identity
 * for the life of the component, so effects may depend on it freely.
 */
export function useDialogError(openDialog: string | null) {
  const [stored, setStored] = useState<Stored>(NONE)
  const [seen, setSeen] = useState(openDialog)

  // Adjusting state while rendering is React's way to reset on a prop change
  // without a render that still shows the old message.
  if (seen !== openDialog) {
    setSeen(openDialog)
    if (stored.dialog !== openDialog) setStored(NONE)
  }

  // Read when the error is set, not when the handler that sets it was created.
  const openRef = useRef(openDialog)
  useEffect(() => {
    openRef.current = openDialog
  }, [openDialog])

  const text = stored.dialog === openDialog ? stored.text : null
  const setMessage = useCallback((next: string | null, dialog?: string | null) => {
    setStored({ dialog: dialog === undefined ? openRef.current : dialog, text: next })
  }, [])

  return [text, setMessage] as const
}
