'use client'

import { LogOutIcon } from 'lucide-react'
import { useTranslations } from 'next-intl'

import { Button } from '../components/ui/button'
import type { AuthMetadata } from './dashboard-types'
import { formatAuthSource } from './dashboard-runtime-helpers'
import { logoutHref } from './dashboard-shell'

export function SettingsAccountSection({ auth }: { auth: AuthMetadata }) {
  const t = useTranslations('settingsPage')
  const tAuth = useTranslations('runtime.authSource')
  const signOutHref = logoutHref(auth)

  return (
    <section className="divide-y divide-border" id="account">
      <div className="flex flex-col gap-4 py-5 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <div className="text-sm font-medium text-foreground">
            {t('currentSession')}
          </div>
          <p className="mt-1 text-sm text-muted-foreground">
            {t('authenticatedWith', {
              source: formatAuthSource(auth.source, tAuth),
              provider: auth.provider,
            })}
          </p>
        </div>
        {signOutHref ? (
          <div>
            <Button
              nativeButton={false}
              render={<a aria-label={t('signOut')} href={signOutHref} />}
              size="lg"
              variant="outline"
            >
              <LogOutIcon aria-hidden="true" />
              {t('signOut')}
            </Button>
          </div>
        ) : (
          <span className="text-xs text-muted-foreground">
            {t('signOutUnavailable')}
          </span>
        )}
      </div>
    </section>
  )
}
