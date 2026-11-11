import { useState } from 'react'
import { NextIntlClientProvider } from 'next-intl'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'

import en from '../messages/en.json'
import { JobHistory } from './dashboard-job-history'
import type { Job, Tenant } from './dashboard-types'

function createTestQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  })
}

const tenant: Tenant = {
  id: 'tenant-1',
  slug: 'tenant-one',
  display_name: 'Tenant One',
  created_at: '2026-07-15T00:00:00Z',
}

function job(overrides: Partial<Job> = {}): Job {
  return {
    id: 'job-1',
    tenant_id: tenant.id,
    printer_id: 'printer-1',
    agent_id: 'agent-1',
    artifact_id: 'artifact-1',
    command_id: 'command-1',
    status: 'succeeded',
    error: null,
    created_at: '2026-07-15T00:00:00Z',
    updated_at: '2026-07-15T00:00:00Z',
    print: {
      status: 'completed',
      printer_state: null,
      progress_percent: 100,
      remaining_time_minutes: 0,
      current_layer: 10,
      total_layers: 10,
      active_file: null,
      last_progress_percent: 100,
      last_layer: 10,
      error: null,
      started_at: '2026-07-15T00:00:00Z',
      finished_at: '2026-07-15T01:00:00Z',
      updated_at: '2026-07-15T01:00:00Z',
    },
    command: {
      id: 'command-1',
      kind: 'print_project_file',
      status: 'succeeded',
    uploaded_url: 'brtc://emmc/part.3mf',
    },
    artifact: {
      id: 'artifact-1',
      tenant_id: 'tenant-1',
      filename: 'benchy.3mf',
      content_type: 'model/3mf',
      size_bytes: 42,
      metadata: null,
      created_at: '2026-07-15T00:00:00Z',
    },
    material: {
      ams_mapping: null,
      ams_mapping2: null,
      ams_mapping_info: null,
      filament_usage: [],
    },
    ...overrides,
  }
}

function JobFilterRouteHarness() {
  const [showJobs, setShowJobs] = useState(true)
  const first = job()
  const second = job({
    id: 'job-2',
    artifact_id: 'artifact-2',
    command_id: 'command-2',
    command: { ...first.command, id: 'command-2' },
    artifact: {
      ...first.artifact,
      id: 'artifact-2',
      filename: 'cube.3mf',
    },
  })

  return (
    <>
      <button onClick={() => setShowJobs((current) => !current)} type="button">
        Switch route
      </button>
      {showJobs ? (
        <JobHistory
          agents={[]}
          jobs={[first, second]}
          nowMs={0}
          onOpenDispatch={vi.fn()}
          onOpenReprint={vi.fn()}
          printers={[]}
          selectedTenant={tenant}
        />
      ) : (
        <div>Devices route</div>
      )}
    </>
  )
}

function renderFilterRouteHarness() {
  return render(
    <QueryClientProvider client={createTestQueryClient()}>
      <NextIntlClientProvider locale="en" messages={en}>
        <JobFilterRouteHarness />
      </NextIntlClientProvider>
    </QueryClientProvider>,
  )
}

describe('JobHistory filters', () => {
  it('resets its typed filters after navigating away and back', async () => {
    const user = userEvent.setup()
    renderFilterRouteHarness()

    await user.type(screen.getByRole('searchbox'), 'benchy')
    expect(screen.getByText('benchy.3mf')).toBeInTheDocument()
    expect(screen.queryByText('cube.3mf')).not.toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: 'Switch route' }))
    await user.click(screen.getByRole('button', { name: 'Switch route' }))

    expect(screen.getByText('benchy.3mf')).toBeInTheDocument()
    expect(screen.getByText('cube.3mf')).toBeInTheDocument()
  })
})
