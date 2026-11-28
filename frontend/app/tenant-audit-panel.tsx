'use client'

import { useTranslations } from 'next-intl'

import { FormattedDate } from '../components/formatted-date'
import type { AuditEvent } from './dashboard-types'

export function TenantAuditPanel({
  auditEvents,
}: {
  auditEvents: AuditEvent[]
}) {
  const t = useTranslations('admin')

  return (
    <div>
      <div className="flex items-center justify-between pb-4">
        <h3 className="text-sm font-semibold text-foreground">
          {t('auditEvents')}
        </h3>
        <span className="text-xs text-muted-foreground">
          {t('auditMeta', { count: auditEvents.length })}
        </span>
      </div>
      <div className="divide-y divide-border border-t border-border">
        {auditEvents.length === 0 ? (
          <div className="py-3 text-sm text-muted-foreground">
            {t('noAuditEvents')}
          </div>
        ) : (
          auditEvents.map((event) => (
            <div
              key={event.id}
              className="flex flex-col gap-1 py-3 text-sm sm:flex-row sm:items-center sm:justify-between sm:gap-4"
            >
              <span className="font-medium text-foreground">
                {event.action}
              </span>
              <span className="text-xs text-muted-foreground">
                {event.actor_type} · {event.target_type} ·{' '}
                <FormattedDate value={event.created_at} />
              </span>
            </div>
          ))
        )}
      </div>
    </div>
  )
}
