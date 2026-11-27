import { useTranslations } from 'next-intl'
import { ChevronRightIcon } from 'lucide-react'

import { Button } from '../components/ui/button'
import { SidebarTrigger } from '../components/ui/sidebar'
import { useIsMobile } from '../hooks/use-mobile'
import {
  dashboardViewTitleKey,
  type DashboardView,
} from './dashboard-shell'
import { useDashboardShell } from './dashboard-shell-provider'

export function DashboardShellHeader({
  view,
}: {
  view: DashboardView
}) {
  const tShell = useTranslations('dashboardShell')
  const { headerOverride } = useDashboardShell()
  const isMobile = useIsMobile()
  const override = isMobile ? headerOverride : null
  return (
    <header className="sticky top-0 z-20 flex min-h-16 shrink-0 items-center border-b border-border bg-background/95 px-4 py-3 backdrop-blur">
      <div className="flex min-w-0 items-center gap-3">
        {override ? (
          <Button
            aria-label={override.backLabel}
            className="-ml-1"
            onClick={override.onBack}
            size="icon-sm"
            type="button"
            variant="ghost"
          >
            <ChevronRightIcon className="rotate-180" />
          </Button>
        ) : (
          <SidebarTrigger className="-ml-1" label={tShell('toggleSidebar')} />
        )}
        <div className="min-w-0">
          <div className="text-xs font-medium text-muted-foreground">
            {override ? override.meta : tShell('brand')}
          </div>
          <h1 className="truncate text-lg font-semibold text-foreground">
            {override ? override.title : tShell(dashboardViewTitleKey(view))}
          </h1>
        </div>
      </div>
    </header>
  )
}
