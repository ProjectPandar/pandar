'use client'

import { useTranslations } from 'next-intl'
import { CheckCircle2Icon, Loader2Icon, ThermometerIcon } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
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
  formatTemperatureValue,
  hasActiveTargetTemperature,
  nozzleTitle,
  type PresentedNozzle,
} from './dashboard-printer-nozzle-temperature'
import {
  PrinterControlFields,
  printerControlFieldNames,
  usePrinterControl,
} from './printer-controls'

const NOZZLE_TEMPERATURE_PRESETS = [0, 120, 220, 260] as const

export function NozzleTemperatureCard({
  printer,
  nozzles,
}: {
  printer: Printer
  nozzles: PresentedNozzle[]
}) {
  const t = useTranslations('inventory')
  const nozzle = nozzleTemperature(nozzles)
  if (!nozzle) {
    return null
  }

  return (
    <Popover>
      <PopoverTrigger
        render={
          <Button
            aria-label={nozzles.length > 1 ? t('setNozzleTemperatures') : t('setNozzleTemperature')}
            className="h-auto min-h-16 w-full flex-col rounded-md bg-muted/50 px-3 py-2 text-center font-normal hover:bg-muted dark:hover:bg-muted"
            type="button"
            variant="ghost"
          />
        }
      >
        <ThermometerIcon className="size-4 text-muted-foreground" />
        <div className="mt-1 text-xs font-medium text-muted-foreground">{t('nozzleTemperature')}</div>
        {nozzle.label ? <div className="text-xs font-medium text-muted-foreground">{nozzle.label}</div> : null}
        <div className="text-sm font-semibold text-foreground">{nozzle.value}</div>
      </PopoverTrigger>
      <PopoverContent className={nozzles.length > 1 ? 'w-[min(24rem,calc(100vw-2rem))]' : 'w-72'} sideOffset={8}>
        <PopoverTitle className="text-center text-base font-semibold">
          {nozzles.length > 1 ? t('setNozzleTemperaturesTitle') : t('setNozzleTemperatureTitle')}
        </PopoverTitle>
        <NozzleTemperatureMenu nozzles={nozzles} printer={printer} />
      </PopoverContent>
    </Popover>
  )
}

function NozzleTemperatureMenu({
  printer,
  nozzles,
}: {
  printer: Printer
  nozzles: PresentedNozzle[]
}) {
  const activeNozzle = activeNozzleLabel(printer)
  if (nozzles.length === 1) {
    return <NozzleTemperaturePanel nozzle={nozzles[0]} printer={printer} />
  }

  return (
    <div className="grid gap-2 sm:grid-cols-2">
      {nozzles.map((nozzle) => (
        <NozzleTemperaturePanel
          active={nozzle.label === activeNozzle}
          key={nozzle.label}
          nozzle={nozzle}
          printer={printer}
        />
      ))}
    </div>
  )
}

function NozzleTemperaturePanel({
  printer,
  nozzle,
  active = false,
}: {
  printer: Printer
  nozzle: PresentedNozzle
  active?: boolean
}) {
  const t = useTranslations('inventory')
  const extruderId = extruderIdForNozzle(nozzle.label)
  const title = nozzleTitle(nozzle.label, t)

  return (
    <div className={`space-y-2 rounded-md border p-2 ${active ? 'border-primary' : 'border-border'}`}>
      <span className="flex items-center justify-between text-sm font-semibold">
        {title}
        {active ? (
          <span className="inline-flex items-center gap-1 text-xs font-medium text-success">
            <CheckCircle2Icon aria-hidden="true" className="size-3" />
            {t('activeNozzle')}
          </span>
        ) : null}
      </span>
      <TemperatureReading current={nozzle.current_celsius} target={nozzle.target_celsius} />
      <div className="grid grid-cols-2 gap-1.5">
        {NOZZLE_TEMPERATURE_PRESETS.map((temperature) => (
          <TemperaturePresetButton
            extruderId={extruderId}
            key={temperature}
            printer={printer}
            temperature={temperature}
          />
        ))}
      </div>
      <CustomTemperatureForm extruderId={extruderId} printer={printer} />
    </div>
  )
}

export function TemperatureReading({
  current,
  target,
}: {
  current?: string | null
  target?: string | null
}) {
  const t = useTranslations('inventory')
  return (
    <div className="flex flex-wrap items-center justify-center gap-x-3 gap-y-1 text-xs font-medium text-muted-foreground">
      <span>{`${t('currentTemperature')} ${formatTemperatureValue(current)}`}</span>
      {hasActiveTargetTemperature(target) ? (
        <span>{`${t('targetTemperature')} ${formatTemperatureValue(target)}`}</span>
      ) : null}
    </div>
  )
}

function TemperaturePresetButton({
  printer,
  temperature,
  extruderId,
}: {
  printer: Printer
  temperature: number
  extruderId: number | null
}) {
  const t = useTranslations('inventory')
  const { formAction, pending } = usePrinterControl()
  return (
    <form action={formAction}>
      <PrinterControlFields
        printer={printer}
        intent={{ action: 'set_hotend_temperature', extruderId, temperatureCelsius: temperature }}
      />
      <Button className="w-full" disabled={pending} size="sm" type="submit" variant="outline">
        {pending ? <Loader2Icon className="animate-spin" /> : null}
        {temperature === 0 ? t('temperatureOff') : `${temperature} C`}
      </Button>
    </form>
  )
}

function CustomTemperatureForm({
  printer,
  extruderId,
}: {
  printer: Printer
  extruderId: number | null
}) {
  const t = useTranslations('inventory')
  const { formAction, pending } = usePrinterControl()
  return (
    <form action={formAction} className="flex gap-1.5">
      <PrinterControlFields
        printer={printer}
        intent={{ action: 'set_hotend_temperature', extruderId }}
      />
      <Input
        aria-label={t('customTemperature')}
        inputMode="numeric"
        min="0"
        name={printerControlFieldNames.temperatureCelsius}
        placeholder={t('customTemperature')}
        type="number"
      />
      <Button disabled={pending} size="sm" type="submit" variant="secondary">
        {pending ? <Loader2Icon className="animate-spin" /> : null}
        {t('setTemperature')}
      </Button>
    </form>
  )
}

function nozzleTemperature(nozzles: PresentedNozzle[]) {
  if (nozzles.length === 0) {
    return null
  }
  if (nozzles.length === 1) {
    return {
      label: null,
      value: formatTemperatureValue(nozzles[0].current_celsius, false),
    }
  }
  return {
    label: nozzles.map((nozzle) => nozzle.label).join(' / '),
    value: nozzles.map((nozzle) => formatTemperatureValue(nozzle.current_celsius, false)).join(' / '),
  }
}
