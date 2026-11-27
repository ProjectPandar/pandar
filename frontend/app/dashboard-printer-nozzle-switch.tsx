'use client'

import { useTranslations } from 'next-intl'
import { ArrowLeftRightIcon, CheckCircle2Icon, Loader2Icon } from 'lucide-react'

import { Button } from '@/components/ui/button'
import {
  Popover,
  PopoverContent,
  PopoverTitle,
  PopoverTrigger,
} from '@/components/ui/popover'

import type { Printer } from './dashboard-types'
import {
  activeNozzleLabel,
  extruderIdForNozzle,
  presentNozzles,
  type PresentedNozzle,
} from './dashboard-printer-nozzle-temperature'
import { PrinterControlFields, usePrinterControl } from './printer-controls'

/** Two nozzles toggle between each other; tool changers pick the active tool explicitly. */
export function NozzleSwitchControl({ printer }: { printer: Printer }) {
  const t = useTranslations('inventory')
  const nozzles = presentNozzles(printer.nozzle_temperatures ?? [])
  const { formAction, pending } = usePrinterControl()
  if (nozzles.length < 2) {
    return null
  }

  const activeNozzle = activeNozzleLabel(printer)
  if (nozzles.length > 2) {
    return <NozzlePicker activeNozzle={activeNozzle} nozzles={nozzles} printer={printer} />
  }

  const [target] = nozzles.flatMap((nozzle) => {
    const extruderId = extruderIdForNozzle(nozzle.label)
    return nozzle.label !== activeNozzle && extruderId !== null ? [{ nozzle, extruderId }] : []
  })
  if (!target) {
    return null
  }
  return (
    <form action={formAction} className="h-full">
      <PrinterControlFields
        printer={printer}
        intent={{ action: 'select_extruder', extruderId: target.extruderId }}
      />
      <Button
        aria-label={t('switchNozzleTo', { nozzle: target.nozzle.label })}
        className="h-full min-h-16 w-full flex-col rounded-md bg-muted/50 px-3 py-2 text-center disabled:text-muted-foreground"
        disabled={pending}
        type="submit"
        variant="ghost"
      >
        {pending ? (
          <Loader2Icon className="size-4 animate-spin text-muted-foreground" />
        ) : (
          <ArrowLeftRightIcon className="size-4 text-muted-foreground" />
        )}
        <div className="mt-1 flex items-start justify-center gap-2 text-xs font-semibold">
          {nozzles.map((nozzle) => (
            <span
              className={`flex flex-col items-center ${nozzle.label === activeNozzle ? 'text-primary' : 'text-muted-foreground'}`}
              key={nozzle.label}
            >
              <span className="flex items-center gap-1">
                <span>{nozzle.label}</span>
                {nozzle.label === activeNozzle ? (
                  <>
                    <CheckCircle2Icon aria-hidden="true" className="size-3 text-success" />
                    <span className="sr-only">{t('activeNozzle')}</span>
                  </>
                ) : null}
              </span>
              <NozzleInfo nozzle={nozzle} />
            </span>
          ))}
        </div>
        <div className="text-xs font-medium text-muted-foreground">{t('nozzleTemperature')}</div>
      </Button>
    </form>
  )
}

function NozzlePicker({
  nozzles,
  activeNozzle,
  printer,
}: {
  nozzles: PresentedNozzle[]
  activeNozzle: string | null
  printer: Printer
}) {
  const t = useTranslations('inventory')
  const { formAction, pending } = usePrinterControl()
  const title = t('selectNozzle')
  const options = nozzles.flatMap((nozzle) => {
    const extruderId = extruderIdForNozzle(nozzle.label)
    return extruderId === null ? [] : [{ nozzle, extruderId }]
  })
  return (
    <Popover>
      <PopoverTrigger
        render={
          <Button
            aria-label={title}
            className="h-full min-h-16 w-full flex-col rounded-md bg-muted/50 px-3 py-2 text-center font-normal hover:bg-muted dark:hover:bg-muted"
            type="button"
            variant="ghost"
          />
        }
      >
        <ArrowLeftRightIcon className="size-4 text-muted-foreground" />
        <div className="mt-1 text-xs font-medium text-muted-foreground">{title}</div>
        <div className="text-sm font-semibold text-foreground">{activeNozzle ?? '—'}</div>
      </PopoverTrigger>
      <PopoverContent className="w-72" sideOffset={8}>
        <PopoverTitle className="text-center text-base font-semibold">{title}</PopoverTitle>
        <div aria-label={title} className="grid grid-cols-2 gap-1.5" role="group">
          {options.map(({ nozzle, extruderId }) => {
            const active = nozzle.label === activeNozzle
            return (
              <form action={formAction} key={nozzle.label}>
                <PrinterControlFields
                  printer={printer}
                  intent={{ action: 'select_extruder', extruderId }}
                />
                <Button
                  aria-label={t('switchNozzleTo', { nozzle: nozzle.label })}
                  aria-pressed={active}
                  className="h-auto w-full flex-col gap-0.5 py-1.5"
                  disabled={pending || active}
                  type="submit"
                  variant="outline"
                >
                  <span className="flex items-center gap-1">
                    {active ? (
                      <CheckCircle2Icon aria-hidden="true" className="size-4 text-success" />
                    ) : null}
                    {nozzle.label}
                  </span>
                  <NozzleInfo nozzle={nozzle} />
                </Button>
              </form>
            )
          })}
        </div>
      </PopoverContent>
    </Popover>
  )
}

function NozzleInfo({ nozzle }: { nozzle: PresentedNozzle }) {
  if (!nozzle.diameter_mm && !nozzle.nozzle_type) {
    return null
  }
  return (
    <span className="mt-0.5 flex flex-col text-xs font-medium leading-tight text-muted-foreground">
      {nozzle.diameter_mm ? <span>{`${nozzle.diameter_mm} mm`}</span> : null}
      {nozzle.nozzle_type ? <span>{nozzle.nozzle_type}</span> : null}
    </span>
  )
}
