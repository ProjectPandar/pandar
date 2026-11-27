'use client'

import Link from 'next/link'
import { useFormatter, useTranslations } from 'next-intl'
import { toast } from 'sonner'

import { dashboardSidebarHref } from './dashboard-shell'
import type { Job, Printer, Tenant } from './dashboard-types'
import { isClearableJob, isRetryDispatchSafe } from './dashboard-job-status'
import { JobRowActions } from './dashboard-job-row-actions'
import { formatJobRecoveryState } from './dashboard-runtime-helpers'
import { EmptyState, StatusBadge } from './dashboard-ui'
import { DeleteJobDialog } from './delete-job-dialog'
import { formatLayers, formatRemaining } from './job-format'
import { useJobDelete } from './use-job-delete'

const QUEUE_LIMIT = 5

/** Newest jobs of one printer, mirroring the job list rows in a compact form. */
export function PrinterJobQueue({
  selectedTenant,
  printer,
  jobs,
  canManageJobs = true,
  onOpenReprint,
}: {
  selectedTenant: Tenant | null
  printer: Printer
  jobs: Job[]
  canManageJobs?: boolean
  onOpenReprint: (job: Job) => void
}) {
  const t = useTranslations('inventory')
  const tStatus = useTranslations('runtime.actionStatus')
  const jobDelete = useJobDelete(selectedTenant, () =>
    toast.success(tStatus('job_deleted')),
  )
  const printerJobs = jobs
    .filter((job) => job.printer_id === printer.id)
    .sort((left, right) => Date.parse(right.updated_at) - Date.parse(left.updated_at))
    .slice(0, QUEUE_LIMIT)

  return (
    <section
      aria-label={t('queueTitle')}
      className="overflow-hidden rounded-md border border-border bg-card"
    >
      <div className="flex flex-col gap-2 border-b border-border px-4 py-3 sm:flex-row sm:items-center sm:justify-between">
        <h2 className="text-base font-semibold">{t('queueTitle')}</h2>
        <Link
          className="text-xs font-medium text-primary hover:underline"
          href={dashboardSidebarHref('jobs')}
        >
          {t('queueAll')}
        </Link>
      </div>
      {printerJobs.length === 0 ? (
        <EmptyState title={t('queueEmpty')} message={t('queueEmptyMessage')} />
      ) : (
        <ul className="divide-y divide-border">
          {printerJobs.map((job) => (
            <QueueRow
              canDelete={canManageJobs && isClearableJob(job)}
              job={job}
              key={job.id}
              onDelete={jobDelete.request}
              onOpenReprint={onOpenReprint}
            />
          ))}
        </ul>
      )}
      <DeleteJobDialog
        target={jobDelete.target}
        deleting={jobDelete.deleting}
        error={jobDelete.error}
        onClose={jobDelete.close}
        onConfirm={jobDelete.confirm}
      />
    </section>
  )
}

function QueueRow({
  job,
  canDelete,
  onDelete,
  onOpenReprint,
}: {
  job: Job
  canDelete: boolean
  onDelete: (job: Job) => void
  onOpenReprint: (job: Job) => void
}) {
  const t = useTranslations('inventory')
  const tRecovery = useTranslations('recovery.state')
  const tJob = useTranslations('jobFormat')
  const format = useFormatter()
  const progressValue =
    job.print.progress_percent ?? job.print.last_progress_percent ?? null
  const progress = progressValue === null ? null : Math.min(100, Math.max(0, progressValue))
  const remaining = job.print.remaining_time_minutes
  const summary = [
    progress !== null ? `${format.number(progress)}%` : null,
    formatLayers(job, tJob),
    formatRemaining(remaining, tJob),
  ].filter((part): part is string => part !== null && !part.endsWith('-'))
  const meta = summary.length > 0 ? summary.join(' · ') : formatJobRecoveryState(job, tRecovery)

  return (
    <li className="px-4 py-3">
      <div className="flex min-w-0 items-center gap-2">
        <span className="truncate text-sm font-medium text-foreground">
          {job.artifact.filename}
        </span>
        <span className="shrink-0">
          <StatusBadge value={job.print.status} />
        </span>
      </div>
      <div className="mt-1.5 text-xs text-muted-foreground">{meta}</div>
      {progress !== null ? (
        <div
          aria-label={t('print')}
          aria-valuemax={100}
          aria-valuemin={0}
          aria-valuenow={progress}
          className="mt-2 h-1.5 overflow-hidden rounded-full bg-muted"
          role="progressbar"
        >
          <div className="h-full rounded-full bg-primary" style={{ width: `${progress}%` }} />
        </div>
      ) : null}
      <div className="mt-2 flex flex-wrap justify-end gap-2">
        <JobRowActions
          job={job}
          canDelete={canDelete}
          canRetryDispatch={isRetryDispatchSafe(job)}
          deleteUnavailableReason={t('deleteJobUnavailable')}
          onDelete={() => onDelete(job)}
          onReprint={() => onOpenReprint(job)}
        />
      </div>
    </li>
  )
}
