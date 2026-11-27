'use client'

import { useTranslations } from 'next-intl'
import { ClockIcon, PrinterIcon } from 'lucide-react'

import type { Job, Printer, Tenant } from './dashboard-types'
import { StatusBadge } from './dashboard-ui'
import { CompatibilityRow } from './diagnostics-section'
import { PrinterActions } from './dashboard-printer-actions'
import { PrinterAxisControls } from './dashboard-printer-axis-controls'
import { PrinterCameraFrame } from './dashboard-printer-camera-frame'
import { PrinterCoolingSystem } from './dashboard-printer-cooling'
import { PrinterHmsPanel } from './dashboard-printer-hms'
import { PrinterMaterialsPanel } from './dashboard-printer-materials'
import { PrinterJobQueue } from './dashboard-printer-queue'
import { PrinterRackPanel } from './dashboard-printer-rack'
import { PrinterControlsPanel, PrinterTemperatureControls } from './dashboard-printer-temperature-controls'
import { PrinterLastSeen } from './printer-last-seen'

export function PrinterDetail({
  selectedTenant,
  printer,
  agentName,
  jobs,
  nowMs,
  onOpenReprint,
}: {
  selectedTenant: Tenant
  printer: Printer
  agentName: string
  jobs: Job[]
  nowMs: number
  onOpenReprint: (job: Job) => void
}) {
  const t = useTranslations('inventory')
  const tDiagnostics = useTranslations('diagnostics')
  const compatibility = printer.compatibility

  return (
    <article aria-label={printer.name} className="flex min-w-0 flex-col gap-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex min-w-0 items-center gap-3">
          <span className="flex size-12 shrink-0 items-center justify-center rounded-md bg-muted text-muted-foreground">
            <PrinterIcon className="size-6" />
          </span>
          <div className="min-w-0">
            <h2 className="flex items-center gap-2 text-base font-semibold text-foreground">
              <span className="truncate">{printer.name}</span>
              <StatusBadge value={printer.status} />
            </h2>
            <p className="mt-0.5 flex flex-wrap items-center gap-x-2 text-xs text-muted-foreground">
              <span>{printer.model ?? t('unknownModel')}</span>
              <span aria-hidden="true">·</span>
              <span className="font-mono">{printer.serial_number}</span>
              <span aria-hidden="true">·</span>
              <span>{agentName}</span>
              <span aria-hidden="true">·</span>
              <span className="inline-flex items-center gap-1">
                <ClockIcon className="size-3.5" />
                <PrinterLastSeen nowMs={nowMs} value={printer.last_seen_at} />
              </span>
            </p>
          </div>
        </div>
        <PrinterActions layout="split" printer={printer} />
      </div>

      <div className="grid gap-4 xl:grid-cols-2">
        <div className="rounded-md border border-border bg-card p-4">
          <PrinterCameraFrame printer={printer} />
          <PrinterControlsPanel printer={printer} />
          <PrinterAxisControls printer={printer} />
        </div>

        <div className="rounded-md border border-border bg-card p-4">
          <div className="-mt-4">
            <PrinterHmsPanel printer={printer} />
            <PrinterTemperatureControls printer={printer} />
            <PrinterCoolingSystem printer={printer} />
          </div>
          <dl className="mt-4 grid gap-2 text-sm">
            <CompatibilityRow
              available={null}
              label={tDiagnostics('model')}
              value={compatibility.normalized_model ?? printer.model ?? '-'}
            />
            <CompatibilityRow
              available={compatibility.external_storage === 'supported'}
              help={tDiagnostics('externalStorageHelp')}
              label={tDiagnostics('externalStorage')}
              value={compatibility.external_storage}
            />
            <CompatibilityRow
              available={compatibility.ftps_tls_1_2_cap}
              help={tDiagnostics('ftpsCapHelp')}
              label={tDiagnostics('ftpsCap')}
              value={
                compatibility.ftps_tls_1_2_cap
                  ? tDiagnostics('available')
                  : tDiagnostics('unavailable')
              }
            />
          </dl>
        </div>
      </div>

      <div className="grid gap-4 xl:grid-cols-[minmax(0,0.85fr)_minmax(0,1.15fr)]">
        <div className="rounded-md border border-border bg-card p-4">
          <div className="-mt-4">
            <PrinterMaterialsPanel printer={printer} />
            <PrinterRackPanel printer={printer} />
          </div>
        </div>
        <PrinterJobQueue
          jobs={jobs}
          onOpenReprint={onOpenReprint}
          printer={printer}
          selectedTenant={selectedTenant}
        />
      </div>
    </article>
  )
}
