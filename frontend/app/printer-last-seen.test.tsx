import { NextIntlClientProvider } from 'next-intl'
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import en from '../messages/en.json'
import { PrinterLastSeen } from './printer-last-seen'

const LAST_SEEN = '2026-07-02T00:00:00Z'
const LAST_SEEN_MS = Date.parse(LAST_SEEN)

function renderLastSeen(nowMs: number) {
  return render(
    <NextIntlClientProvider locale="en" messages={en}>
      <PrinterLastSeen nowMs={nowMs} value={LAST_SEEN} />
    </NextIntlClientProvider>,
  )
}

describe('PrinterLastSeen', () => {
  it('clamps future timestamps to Online', () => {
    renderLastSeen(LAST_SEEN_MS - 1)

    expect(screen.getByText('Online')).toBeVisible()
  })

  it('keeps the absolute fallback while nowMs is zero', () => {
    renderLastSeen(0)

    expect(screen.getByText(/Jul 2, 2026/)).toBeVisible()
    expect(screen.queryByText('Online')).not.toBeInTheDocument()
  })
})
