'use client'

import { useMemo, useState, type ReactNode } from 'react'
import { useTranslations } from 'next-intl'
import { PlusIcon, PrinterIcon } from 'lucide-react'

import {
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from '@/components/ui/empty'
import { Button } from '@/components/ui/button'
import { OFFLINE_PRINTER_STATUSES, statusMeta } from './dashboard-attention'
import type { Agent, Job, Printer, Tenant } from './dashboard-types'
import { PrinterDetail } from './dashboard-printer-detail'
import { PrinterLastSeen } from './printer-last-seen'
import { FilterBar } from './dashboard-filter-bar'
import type { Translator } from './dashboard-runtime-helpers'
import { StatusBadge } from './dashboard-ui'
import { formatRemaining } from './job-format'
import { useIsMobile } from '../hooks/use-mobile'
import { LinkPrinterDialog } from './link-printer-dialog'

export { JobHistory } from './dashboard-job-history'

export type PrinterFilterStatus = 'all' | 'online' | 'attention'

const ACTIVE_GCODE_STATES = new Set(['RUNNING', 'PRINTING', 'PAUSE', 'PAUSED', 'PREPARE', 'SLICING'])

/** Printer console: a searchable list on the left and the selected printer's live detail on the right. */
export function PrinterInventory({
  selectedTenant,
  printers,
  agents,
  jobs,
  nowMs,
  selectedPrinterId,
  onSelectPrinter,
  onOpenReprint,
}: {
  selectedTenant: Tenant | null
  printers: Printer[]
  agents: Agent[]
  jobs: Job[]
  nowMs: number
  selectedPrinterId: string | null
  onSelectPrinter: (printerId: string | null) => void
  onOpenReprint: (job: Job) => void
}) {
  const t = useTranslations('inventory')
  const tLink = useTranslations('linkPrinter')
  const tJob = useTranslations('jobFormat')
  const tTokens = useTranslations('tokens')
  const isMobile = useIsMobile()
  const [query, setQuery] = useState('')
  const [status, setStatus] = useState<PrinterFilterStatus>('all')
  const normalizedQuery = query.trim().toLowerCase()
  const filtered = useMemo(
    () =>
      printers.filter((printer) => {
        const needsAttention = OFFLINE_PRINTER_STATUSES.has(printer.status.toLowerCase())
        if (status === 'online' && needsAttention) {
          return false
        }
        if (status === 'attention' && !needsAttention) {
          return false
        }
        if (normalizedQuery) {
          const haystack = `${printer.name} ${printer.serial_number}`.toLowerCase()
          if (!haystack.includes(normalizedQuery)) {
            return false
          }
        }
        return true
      }),
    [printers, status, normalizedQuery],
  )
  const agentNames = useMemo(
    () => new Map(agents.map((agent) => [agent.id, agent.name])),
    [agents],
  )
  const selected =
    printers.find((printer) => printer.id === selectedPrinterId) ??
    (isMobile ? null : filtered[0]) ??
    null
  const onlineCount = printers.filter(
    (printer) => !OFFLINE_PRINTER_STATUSES.has(printer.status.toLowerCase()),
  ).length
  const tokenTranslator = (k: string) => (tTokens.has(k) ? tTokens(k) : undefined)

  if (!selectedTenant) {
    return <PrinterEmptyState title={t('noTenantTitle')} message={t('noTenantMessage')} />
  }
  if (printers.length === 0) {
    return (
      <PrinterEmptyState
        action={
          <LinkPrinterDialog
            agents={agents}
            mode="add"
            selectedTenant={selectedTenant}
            trigger={
              <Button type="button">
                <PlusIcon />
                {tLink('submit')}
              </Button>
            }
          />
        }
        message={t('noPrintersMessage')}
        title={t('noPrintersTitle')}
      />
    )
  }

  const showList = !isMobile || !selected
  const showDetail = !isMobile || Boolean(selected)

  return (
    <div id="printers" className="grid gap-4 scroll-mt-20 lg:grid-cols-[24rem_minmax(0,1fr)]">
      {showList ? (
        <section
          aria-label={t('printersTitle')}
          className="overflow-hidden rounded-md border border-border bg-card"
        >
          <div className="flex flex-col gap-2 border-b border-border px-4 py-3 sm:flex-row sm:items-center sm:justify-between">
            <div>
              <h2 className="text-base font-semibold">{t('printersTitle')}</h2>
              <p className="mt-0.5 text-sm text-muted-foreground">
                {t('listSummary', { total: printers.length, online: onlineCount })}
              </p>
            </div>
            <div className="flex items-center gap-2 text-sm text-muted-foreground">
              <LinkPrinterDialog
                agents={agents}
                mode="add"
                selectedTenant={selectedTenant}
                trigger={
                  <Button type="button">
                    <PlusIcon />
                    {tLink('submit')}
                  </Button>
                }
              />
            </div>
          </div>
          <FilterBar
            onQueryChange={setQuery}
            onStatusChange={setStatus}
            query={query}
            queryPlaceholder={t('searchName')}
            status={status}
            statusOptions={[
              { value: 'all', label: t('filterAll') },
              { value: 'online', label: t('filterOnline') },
              { value: 'attention', label: t('filterAttention') },
            ]}
          />
          {filtered.length === 0 ? (
            <PrinterEmptyState title={t('noMatchesTitle')} message={t('noMatchesMessage')} />
          ) : (
            <ul className="divide-y divide-border">
              {filtered.map((printer) => {
                const isSelected = printer.id === selected?.id
                const row = printerRow(printer, nowMs, tJob, tokenTranslator)
                return (
                  <li key={printer.id}>
                    <button
                      aria-pressed={isSelected}
                      className={`flex w-full items-center gap-3 px-4 py-3 text-left transition-colors duration-150 ease-out hover:bg-muted/40 ${
                        isSelected ? 'bg-muted/60' : ''
                      }`}
                      onClick={() => onSelectPrinter(printer.id)}
                      type="button"
                    >
                      <span className="flex size-10 shrink-0 items-center justify-center rounded-md bg-muted text-muted-foreground">
                        <PrinterIcon className="size-5" />
                      </span>
                      <span className="min-w-0 flex-1">
                        <span className="flex min-w-0 items-center gap-2">
                          <span className="min-w-0 truncate text-sm font-medium text-foreground">
                            {printer.name}
                          </span>
                          <span className="shrink-0">
                            <StatusBadge value={printer.status} />
                          </span>
                        </span>
                        <span className="mt-1 block truncate text-xs text-muted-foreground">
                          {row.summary} · {agentNames.get(printer.agent_id) ?? t('unknownAgent')}
                        </span>
                      </span>
                      <span className="w-28 shrink-0 text-right text-xs text-muted-foreground">
                        <span className="block text-sm font-medium text-foreground">
                          {row.primary}
                        </span>
                        {row.secondary}
                      </span>
                    </button>
                  </li>
                )
              })}
            </ul>
          )}
        </section>
      ) : null}
      {showDetail && selected ? (
        <PrinterDetail
          agentName={agentNames.get(selected.agent_id) ?? t('unknownAgent')}
          jobs={jobs}
          nowMs={nowMs}
          onOpenReprint={onOpenReprint}
          printer={selected}
          selectedTenant={selectedTenant}
        />
      ) : null}
    </div>
  )
}

function printerRow(
  printer: Printer,
  nowMs: number,
  tJob: Translator,
  tokenTranslator: (token: string) => string | undefined,
) {
  const print = printer.print
  const gcodeState = print?.gcode_state?.trim().toUpperCase() || null
  const active = print && (!gcodeState || ACTIVE_GCODE_STATES.has(gcodeState))
  const progress = print?.progress_percent ?? null
  if (active && print) {
    return {
      summary:
        print.subtask_name?.trim() ||
        print.gcode_file?.split(/[\\/]/).at(-1) ||
        statusMeta(printer.status, tokenTranslator).label,
      primary: progress !== null ? `${Math.min(100, Math.max(0, progress))}%` : '—',
      secondary: formatRemaining(print.remaining_time_minutes, tJob),
    }
  }
  return {
    summary: statusMeta(printer.status, tokenTranslator).label,
    primary: '—',
    secondary: <PrinterLastSeen nowMs={nowMs} value={printer.last_seen_at} />,
  }
}

function PrinterEmptyState({
  title,
  message,
  action,
}: {
  title: string
  message: string
  action?: ReactNode
}) {
  return (
    <Empty className="min-h-64 lg:min-h-80">
      <EmptyHeader>
        <EmptyMedia variant="icon">
          <PrinterIcon />
        </EmptyMedia>
        <EmptyTitle>{title}</EmptyTitle>
        <EmptyDescription>{message}</EmptyDescription>
      </EmptyHeader>
      {action ? <EmptyContent className="flex-row justify-center gap-2">{action}</EmptyContent> : null}
    </Empty>
  )
}
