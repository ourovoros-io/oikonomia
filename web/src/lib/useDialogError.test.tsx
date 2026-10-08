/** @vitest-environment jsdom */

import { act, renderHook } from '@testing-library/react'
import { describe, expect, test } from 'vitest'

import { useDialogError } from './useDialogError'

describe('useDialogError', () => {
  test('shows the message while its dialog is open', () => {
    const { result } = renderHook(({ open }) => useDialogError(open), {
      initialProps: { open: 'form' as string | null },
    })

    act(() => result.current[1]('Enter a valid amount'))

    expect(result.current[0]).toBe('Enter a valid amount')
  })

  test('closing the dialog clears it, and reopening starts clean', () => {
    const { result, rerender } = renderHook(({ open }) => useDialogError(open), {
      initialProps: { open: 'form' as string | null },
    })
    act(() => result.current[1]('Enter a valid amount'))

    rerender({ open: null })
    expect(result.current[0]).toBeNull()

    rerender({ open: 'form' })
    expect(result.current[0]).toBeNull()
  })

  test('a page error is dropped once a dialog opens over it', () => {
    const { result, rerender } = renderHook(({ open }) => useDialogError(open), {
      initialProps: { open: null as string | null },
    })
    act(() => result.current[1]('Could not read the file'))
    expect(result.current[0]).toBe('Could not read the file')

    rerender({ open: 'form' })

    expect(result.current[0]).toBeNull()
  })

  test('an error addressed to a dialog that is about to open survives its opening', () => {
    const { result, rerender } = renderHook(({ open }) => useDialogError(open), {
      initialProps: { open: null as string | null },
    })
    act(() => result.current[1]('Could not read the file', 'form'))

    rerender({ open: 'form' })

    expect(result.current[0]).toBe('Could not read the file')
  })

  test('a failure is shown where the person is looking when it arrives', () => {
    const { result, rerender } = renderHook(({ open }) => useDialogError(open), {
      initialProps: { open: 'form' as string | null },
    })
    const lateReport = result.current[1]

    rerender({ open: null })
    act(() => lateReport('Could not save'))

    expect(result.current[0]).toBe('Could not save')
  })
})
