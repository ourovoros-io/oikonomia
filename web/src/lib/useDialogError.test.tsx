/** @vitest-environment jsdom */

import { act, renderHook } from '@testing-library/react'
import { describe, expect, test } from 'vitest'

import { copyError } from './uiError'
import { useDialogError } from './useDialogError'

const invalidAmount = copyError('tx.invalidAmount')
const unreadable = copyError('tx.couldNotReadDoc')
const saveFailed = copyError('error.saveFailed')

describe('useDialogError', () => {
  test('shows the error while its dialog is open', () => {
    const { result } = renderHook(({ open }) => useDialogError(open), {
      initialProps: { open: 'form' as string | null },
    })

    act(() => result.current[1](invalidAmount))

    expect(result.current[0]).toBe(invalidAmount)
  })

  test('closing the dialog clears it, and reopening starts clean', () => {
    const { result, rerender } = renderHook(({ open }) => useDialogError(open), {
      initialProps: { open: 'form' as string | null },
    })
    act(() => result.current[1](invalidAmount))

    rerender({ open: null })
    expect(result.current[0]).toBeNull()

    rerender({ open: 'form' })
    expect(result.current[0]).toBeNull()
  })

  test('a page error is dropped once a dialog opens over it', () => {
    const { result, rerender } = renderHook(({ open }) => useDialogError(open), {
      initialProps: { open: null as string | null },
    })
    act(() => result.current[1](unreadable))
    expect(result.current[0]).toBe(unreadable)

    rerender({ open: 'form' })

    expect(result.current[0]).toBeNull()
  })

  test('an error addressed to a dialog that is about to open survives its opening', () => {
    const { result, rerender } = renderHook(({ open }) => useDialogError(open), {
      initialProps: { open: null as string | null },
    })
    act(() => result.current[1](unreadable, 'form'))

    rerender({ open: 'form' })

    expect(result.current[0]).toBe(unreadable)
  })

  test('a failure is shown where the person is looking when it arrives', () => {
    const { result, rerender } = renderHook(({ open }) => useDialogError(open), {
      initialProps: { open: 'form' as string | null },
    })
    const lateReport = result.current[1]

    rerender({ open: null })
    act(() => lateReport(saveFailed))

    expect(result.current[0]).toBe(saveFailed)
  })
})
