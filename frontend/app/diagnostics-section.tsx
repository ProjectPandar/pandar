import { useTranslations } from 'next-intl'

import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

import {
  diagnosePrinter,
} from './actions'
import { statusMeta } from './dashboard-attention'
import { StatusIcon } from './dashboard-status'
import { DetailLine, EmptyState, HelpTip, StatusBadge } from './dashboard-ui'
import type {
  Command,
  CommandResultData,
  DiagnosticResultData,
  PrinterLinkResultData,
  Printer,
  Tenant,
} from './dashboard-types'

export function DiagnosticsSection({
  selectedTenant,
  printers,
  selectedCommand,
  commandData,
}: {
  selectedTenant: Tenant | null
  printers: Printer[]
  selectedCommand: Command | null
  commandData: CommandResultData | null
}) {
  const t = useTranslations('diagnostics')
  return (
    <section className="overflow-hidden rounded-md border border-border bg-card">
      <div className="flex flex-col gap-2 border-b border-border px-4 py-3 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h2 className="text-base font-semibold">{t('title')}</h2>
          <p className="mt-0.5 text-sm text-muted-foreground">
            {selectedCommand
              ? `${selectedCommand.kind} · ${selectedCommand.status}`
              : t('noCommand')}
          </p>
        </div>
        {selectedCommand ? (
          <div className="font-mono text-xs text-muted-foreground">{selectedCommand.id}</div>
        ) : null}
      </div>

      {selectedTenant && printers.length > 0 ? (
        <div className="border-b border-border">
          <div className="divide-y divide-border">
            {printers.map((printer) => (
              <form
                key={printer.id}
                action={diagnosePrinter}
                className="flex flex-col gap-3 px-4 py-3 sm:flex-row sm:items-center sm:justify-between"
              >
                <input name="tenant_id" type="hidden" value={selectedTenant.id} />
                <input name="agent_id" type="hidden" value={printer.agent_id} />
                <input name="serial_number" type="hidden" value={printer.serial_number} />
                <div className="min-w-0">
                  <div className="truncate text-sm font-medium text-foreground">
                    {printer.name}
                  </div>
                  <div className="truncate font-mono text-xs text-muted-foreground">
                    {printer.serial_number}
                  </div>
                </div>
                <Button
                  aria-label={t('diagnoseFor', { name: printer.name })}
                  size="lg"
                  type="submit"
                  variant="outline"
                >
                  {t('diagnose')}
                </Button>
              </form>
            ))}
          </div>
        </div>
      ) : null}

      {!selectedCommand ? (
        <EmptyState title={t('noCommandTitle')} message={t('noCommandMessage')} />
      ) : commandData?.type === 'printer_diagnostic' ? (
        <DiagnosticResult
          printerName={
            printers.find((printer) => printer.serial_number === commandData.serial_number)?.name ?? null
          }
          result={commandData}
        />
      ) : commandData?.type === 'printer_link' ? (
        <PrinterLinkResult result={commandData} />
      ) : (
        <EmptyState
          title={t('noStructuredTitle')}
          message={selectedCommand.error ?? t('noStructuredMessage')}
        />
      )}
    </section>
  )
}

function PrinterLinkResult({ result }: { result: PrinterLinkResultData }) {
  const t = useTranslations('diagnostics')
  return (
    <div className="grid gap-3 px-4 py-3 text-sm sm:grid-cols-2 lg:grid-cols-5">
      <DetailLine label={t('colHost')} value={result.host} mono />
      <DetailLine label={t('colSerial')} value={result.serial_number} mono />
      <DetailLine label={t('colName')} value={result.name ?? '-'} />
      <DetailLine label={t('colModel')} value={result.model ?? '-'} />
      <DetailLine label={t('colStatus')} value={<StatusBadge value={result.status} />} />
    </div>
  )
}

function DiagnosticResult({
  result,
  printerName,
}: {
  result: DiagnosticResultData
  printerName: string | null
}) {
  const t = useTranslations('diagnostics')
  const tTokens = useTranslations('tokens')
  const tokenTranslator = (k: string) => (tTokens.has(k) ? tTokens(k) : undefined)
  const compatibility = result.compatibility
  const features = compatibility?.features ?? {}
  const checks = result.checks.map((check) => ({
    ...check,
    ...statusMeta(check.status, tokenTranslator),
  }))
  const problems = checks.filter((check) => check.severity !== 'success').length

  return (
    <div className="grid gap-0 lg:grid-cols-[minmax(0,1.3fr)_minmax(20rem,0.7fr)]">
      <div className="border-b border-border lg:border-b-0 lg:border-r">
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-border px-4 py-3">
          <StatusBadge value={result.overall} />
          {printerName ? (
            <span className="text-sm font-medium text-foreground">{printerName}</span>
          ) : null}
          <span className="text-xs text-muted-foreground">
            {result.model ? `${result.model} · ` : ''}
            <span className="font-mono">{result.serial_number}</span>
            {result.host ? (
              <>
                {' · '}
                <span className="font-mono">{result.host}</span>
              </>
            ) : null}
          </span>
          <span className="ml-auto text-xs text-muted-foreground">
            {t('checkSummary', { total: checks.length, passed: checks.length - problems, problems })}
          </span>
        </div>
        <ul aria-label={t('colCheck')} className="divide-y divide-border">
          {checks.map((check) => (
            <li
              className={cn(
                'flex items-start gap-3 px-4 py-3',
                check.severity === 'warning' && 'bg-warning/10',
                check.severity === 'critical' && 'bg-destructive/10',
              )}
              key={check.id}
            >
              <StatusIcon severity={check.severity} className="mt-0.5 h-4 w-4 shrink-0" />
              <div className="min-w-0 flex-1">
                <div className="text-sm font-medium text-foreground">{check.message}</div>
                <div className="mt-0.5 flex flex-wrap items-center gap-x-2 text-xs text-muted-foreground">
                  <span className="font-mono">{check.id}</span>
                  {check.details ? (
                    <>
                      <span aria-hidden="true">·</span>
                      <span className="font-mono">{check.details}</span>
                    </>
                  ) : null}
                </div>
              </div>
              {check.severity === 'success' ? null : <StatusBadge value={check.status} />}
            </li>
          ))}
        </ul>
      </div>
      <div aria-label={t('compatibility')} className="px-4 py-3">
        <h3 className="text-sm font-semibold text-foreground">{t('compatibility')}</h3>
        <dl className="mt-1 grid gap-2 text-sm">
          <CompatibilityRow
            available={null}
            label={t('model')}
            value={compatibility?.normalized_model ?? '-'}
          />
          <CompatibilityRow
            available={compatibility?.external_storage === 'supported'}
            help={t('externalStorageHelp')}
            label={t('externalStorage')}
            value={compatibility?.external_storage ?? t('unknown')}
          />
          <CompatibilityRow
            available={compatibility?.ftps_tls_1_2_cap === true}
            help={t('ftpsCapHelp')}
            label={t('ftpsCap')}
            value={compatibility?.ftps_tls_1_2_cap ? 'available' : 'unavailable'}
          />
          {Object.entries(features).map(([name, value]) => (
            <CompatibilityRow
              available={value === 'supported'}
              featureName={name}
              key={name}
              label={formatCapabilityName(name)}
              value={value}
            />
          ))}
        </dl>
      </div>
    </div>
  )
}

/** Label with optional help tip; `available` null shows the raw value instead of a capability verdict. */
export function CompatibilityRow({
  label,
  value,
  available,
  help,
  featureName,
}: {
  label: string
  value: string
  available: boolean | null
  help?: string
  featureName?: string
}) {
  const t = useTranslations('diagnostics')
  return (
    <div className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-3 border-b border-border py-2 last:border-b-0">
      <dt className="flex min-w-0 items-center gap-1 text-muted-foreground">
        <span className="min-w-0">
          <span className="block truncate">{label}</span>
          {featureName ? (
            <span className="block truncate font-mono text-xs">{featureName}</span>
          ) : null}
        </span>
        {help ? <HelpTip label={label}>{help}</HelpTip> : null}
      </dt>
      <dd className="flex items-center gap-1.5 text-right text-xs font-medium">
        {available === null ? (
          <span className="text-foreground">{value}</span>
        ) : (
          <>
            <StatusIcon
              className={cn('h-3.5 w-3.5', available ? 'text-success' : 'text-muted-foreground')}
              severity={available ? 'success' : 'info'}
            />
            <span className="text-foreground">
              {available ? t('available') : t('unavailable')}
            </span>
          </>
        )}
      </dd>
    </div>
  )
}

function formatCapabilityName(value: string) {
  return value
    .split('_')
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(' ')
}
