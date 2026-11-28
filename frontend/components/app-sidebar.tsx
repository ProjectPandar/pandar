"use client"

import * as React from "react"
import Link from "next/link"
import { usePathname, useRouter, useSearchParams } from "next/navigation"
import { useTranslations } from "next-intl"
import {
  BotIcon,
  Building2Icon,
  CheckIcon,
  ChevronDownIcon,
  ChevronsUpDownIcon,
  ClipboardListIcon,
  KeyRoundIcon,
  LogOutIcon,
  MonitorIcon,
  PaletteIcon,
  SettingsIcon,
  UserRoundIcon,
  UsersIcon,
} from "lucide-react"

import type { AuthMetadata, Tenant } from "@/app/dashboard-types"
import { Button } from "@/components/ui/button"
import {
  dashboardSidebarHref,
  logoutHref,
  parseSettingsSection,
  SETTINGS_SECTION_META,
  settingsSectionHref,
  type DashboardView,
  type SettingsSectionId,
} from "@/app/dashboard-shell"
import { setTenantCookie } from "@/app/tenant-cookie"
import { cn } from "@/lib/utils"
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  useSidebar,
} from "@/components/ui/sidebar"
import {
  Popover,
  PopoverContent,
  PopoverTitle,
  PopoverTrigger,
} from "@/components/ui/popover"

type AppSidebarProps = React.ComponentProps<typeof Sidebar> & {
  activeView: DashboardView
  auth: AuthMetadata
  selectedTenant: Tenant | null
  tenants: Tenant[]
}

const primaryNavItems: Array<{
  view: Exclude<DashboardView, "settings">
  icon: React.ComponentType<{ className?: string }>
}> = [
  { view: "devices", icon: MonitorIcon },
  { view: "jobs", icon: ClipboardListIcon },
  { view: "agents", icon: BotIcon },
  { view: "users", icon: UsersIcon },
]

const settingsSubItems: Array<{
  id: SettingsSectionId
  icon: React.ComponentType<{ className?: string }>
}> = [
  { id: "workspace", icon: Building2Icon },
  { id: "appearance", icon: PaletteIcon },
  { id: "access", icon: KeyRoundIcon },
  { id: "account", icon: UserRoundIcon },
]

export function AppSidebar({
  activeView,
  auth,
  selectedTenant,
  tenants,
  ...props
}: AppSidebarProps) {
  const t = useTranslations("dashboardShell")
  const tSettings = useTranslations("settingsPage")
  const signOutHref = logoutHref(auth)
  const [tenantAccessOpen, setTenantAccessOpen] = React.useState(false)
  const [settingsToggle, setSettingsToggle] = React.useState<{
    view: DashboardView
    open: boolean
  } | null>(null)
  const { isMobile, setOpenMobile, state } = useSidebar()
  const router = useRouter()
  const pathname = usePathname()
  const searchParams = useSearchParams()
  const isSettingsActive = activeView === "settings"
  const settingsExpanded =
    settingsToggle?.view === activeView ? settingsToggle.open : isSettingsActive
  const activeSettingsSection = parseSettingsSection(searchParams.get("section"))
  const isIconCollapsed = !isMobile && state === "collapsed"

  function closeMobileSidebar() {
    if (isMobile) {
      setOpenMobile(false)
    }
  }

  function closeTenantAccess() {
    setTenantAccessOpen(false)
    closeMobileSidebar()
  }

  function selectTenant(tenantId: string) {
    setTenantCookie(tenantId)
    closeTenantAccess()
    // Drop transient query context (command/status): it belongs to the
    // previously selected tenant.
    if (window.location.search) {
      router.push(pathname)
    } else {
      router.refresh()
    }
  }

  return (
    <Sidebar
      variant="inset"
      collapsible="icon"
      mobileTitle={t("navigation")}
      mobileDescription={t("sidebarDescription")}
      {...props}
    >
      <SidebarHeader>
        <SidebarMenu>
          <SidebarMenuItem>
            <Popover open={tenantAccessOpen} onOpenChange={setTenantAccessOpen}>
              <PopoverTrigger
                render={
                  <SidebarMenuButton
                    aria-expanded={tenantAccessOpen}
                    aria-haspopup="dialog"
                    aria-label={t("selectTenantAccess")}
                    size="lg"
                    tooltip={selectedTenant?.display_name ?? t("noTenant")}
                  />
                }
              >
                <div className="flex aspect-square size-8 items-center justify-center rounded-md bg-sidebar-primary text-sidebar-primary-foreground">
                  <Building2Icon className="size-4" />
                </div>
                <div className="grid min-w-0 flex-1 text-left text-sm leading-tight">
                  <span className="truncate font-semibold">
                    {selectedTenant ? selectedTenant.display_name : t("noTenant")}
                  </span>
                  <span className="truncate text-xs text-sidebar-foreground/70">
                    {t("tenantAccess")}
                  </span>
                </div>
                <ChevronsUpDownIcon className="ml-auto size-4 text-sidebar-foreground/70" />
              </PopoverTrigger>
              <PopoverContent align="start" className="w-64 p-0" sideOffset={6}>
                <PopoverTitle className="px-2 py-1.5 text-xs text-muted-foreground">
                  {t("tenantAccess")}
                </PopoverTitle>
                <div className="pb-1">
                  {tenants.length === 0 ? (
                    <div className="px-2 py-2 text-sm text-muted-foreground">
                      {t("noTenant")}
                    </div>
                  ) : (
                    tenants.map((tenant) => {
                      const isSelected = tenant.id === selectedTenant?.id
                      return (
                        <Button
                          aria-current={isSelected ? "true" : undefined}
                          className="h-auto w-full justify-start gap-2 rounded-md px-2 py-2 font-normal text-foreground"
                          key={tenant.id}
                          onClick={() => selectTenant(tenant.id)}
                          type="button"
                          variant="ghost"
                        >
                          <span className="flex size-5 items-center justify-center">
                            {isSelected ? <CheckIcon className="size-4" /> : null}
                          </span>
                          <span className="truncate">{tenant.display_name}</span>
                        </Button>
                      )
                    })
                  )}
                </div>
              </PopoverContent>
            </Popover>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarHeader>
      <SidebarContent>
        <nav aria-label={t("navigation")}>
          <SidebarGroup>
            <SidebarGroupLabel>{t("navigation")}</SidebarGroupLabel>
            <SidebarMenu>
              {primaryNavItems.map((item) => {
                const Icon = item.icon
                const isActive = activeView === item.view
                return (
                  <SidebarMenuItem key={item.view}>
                    <SidebarMenuButton
                      isActive={isActive}
                      tooltip={t(item.view)}
                      render={
                        <Link
                          aria-current={isActive ? "page" : undefined}
                          href={dashboardSidebarHref(item.view)}
                          onClick={closeMobileSidebar}
                          prefetch={false}
                        >
                          <Icon />
                          <span>{t(item.view)}</span>
                        </Link>
                      }
                    />
                  </SidebarMenuItem>
                )
              })}
              <SidebarMenuItem>
                {isIconCollapsed ? (
                  <SidebarMenuButton
                    isActive={isSettingsActive}
                    tooltip={t("settings")}
                    render={
                      <Link
                        aria-current={isSettingsActive ? "page" : undefined}
                        href={dashboardSidebarHref("settings")}
                        prefetch={false}
                      >
                        <SettingsIcon />
                        <span>{t("settings")}</span>
                      </Link>
                    }
                  />
                ) : (
                  <>
                    <SidebarMenuButton
                      aria-controls="sidebar-settings-submenu"
                      aria-expanded={settingsExpanded}
                      className={cn(
                        settingsExpanded &&
                          !isSettingsActive &&
                          "bg-sidebar-foreground/[0.045] text-sidebar-foreground [&_svg]:text-sidebar-foreground",
                      )}
                      isActive={isSettingsActive}
                      onClick={() =>
                        setSettingsToggle({
                          view: activeView,
                          open: !settingsExpanded,
                        })
                      }
                      tooltip={t("settings")}
                      type="button"
                    >
                      <SettingsIcon />
                      <span className="truncate">{t("settings")}</span>
                      <ChevronDownIcon
                        aria-hidden="true"
                        className={cn(
                          "ml-auto size-3.5! text-sidebar-foreground/48! transition-transform duration-150",
                          !settingsExpanded && "-rotate-90",
                        )}
                      />
                    </SidebarMenuButton>
                    {settingsExpanded ? (
                      <ul
                        aria-label={tSettings("sectionNavigation")}
                        className="relative mt-1 mb-0.5 ml-[1.125rem] flex flex-col gap-px pl-2.5 before:absolute before:top-0.5 before:bottom-0.5 before:left-0 before:w-px before:bg-sidebar-border"
                        id="sidebar-settings-submenu"
                      >
                        {settingsSubItems.map((sub) => {
                          const SubIcon = sub.icon
                          const isSubActive =
                            isSettingsActive && activeSettingsSection === sub.id
                          return (
                            <li key={sub.id}>
                              <Link
                                aria-current={isSubActive ? "location" : undefined}
                                className={cn(
                                  "flex h-[1.875rem] w-full items-center gap-2 rounded-[calc(var(--radius)*0.8)] px-2.5 text-[0.8125rem] leading-5 transition-colors [&_svg]:size-[0.9375rem] [&_svg]:shrink-0",
                                  isSubActive
                                    ? "bg-sidebar-foreground/[0.075] font-medium text-sidebar-foreground [&_svg]:text-sidebar-foreground"
                                    : "text-sidebar-foreground/65 hover:bg-sidebar-foreground/[0.045] hover:text-sidebar-foreground [&_svg]:text-sidebar-foreground/55 hover:[&_svg]:text-sidebar-foreground",
                                )}
                                href={settingsSectionHref(sub.id)}
                                onClick={closeMobileSidebar}
                                prefetch={false}
                              >
                                <SubIcon aria-hidden="true" />
                                <span className="truncate">
                                  {tSettings(SETTINGS_SECTION_META[sub.id].titleKey)}
                                </span>
                              </Link>
                            </li>
                          )
                        })}
                      </ul>
                    ) : null}
                  </>
                )}
              </SidebarMenuItem>
            </SidebarMenu>
          </SidebarGroup>
        </nav>

      </SidebarContent>
      {signOutHref ? (
        <SidebarFooter>
          <SidebarMenu>
            <SidebarMenuItem>
              <SidebarMenuButton
                render={
                  <a href={signOutHref}>
                    <LogOutIcon />
                    <span>{t("logout")}</span>
                  </a>
                }
              />
            </SidebarMenuItem>
          </SidebarMenu>
        </SidebarFooter>
      ) : null}
    </Sidebar>
  )
}
