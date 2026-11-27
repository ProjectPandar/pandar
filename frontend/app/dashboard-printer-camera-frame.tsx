'use client'

import { useRef } from 'react'
import { useTranslations } from 'next-intl'
import { MaximizeIcon, PictureInPicture2Icon, WifiIcon } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { apiIdSegment } from './api-path'
import type { Printer } from './dashboard-types'
import { formatLayers, formatRemaining } from './job-format'

const INACTIVE_GCODE_STATES = new Set(['IDLE', 'FINISH', 'FAILED', 'OFFLINE'])

/** Inline camera of the selected printer with the active print summarized over the video. */
export function PrinterCameraFrame({ printer }: { printer: Printer }) {
  const t = useTranslations('inventory')
  const tPrint = useTranslations('printMonitor')
  const tJob = useTranslations('jobFormat')
  const frameRef = useRef<HTMLDivElement>(null)
  const videoRef = useRef<HTMLVideoElement>(null)
  const pictureInPictureSupported =
    typeof document !== 'undefined' &&
    document.pictureInPictureEnabled &&
    typeof HTMLVideoElement !== 'undefined' &&
    typeof HTMLVideoElement.prototype.requestPictureInPicture === 'function'

  const print = printer.print
  const gcodeState = print?.gcode_state?.trim().toUpperCase() || null
  const activePrint = print && (!gcodeState || !INACTIVE_GCODE_STATES.has(gcodeState))
  const task = activePrint
    ? print.subtask_name?.trim() ||
      print.gcode_file?.split(/[\\/]/).at(-1) ||
      tPrint('unknownTask')
    : null
  const progress =
    activePrint && print.progress_percent !== null
      ? Math.min(100, Math.max(0, print.progress_percent))
      : null
  const summaryParts = print && activePrint
    ? [
        progress !== null ? `${progress}%` : null,
        formatLayers({ print }, tJob),
        formatRemaining(print.remaining_time_minutes, tJob),
      ].filter((part) => part !== null && !part.endsWith('-'))
    : []

  return (
    <>
      <div className="mb-2 flex items-center justify-between gap-2 text-xs font-medium text-muted-foreground">
        <span>{t('cameraTitle')}</span>
        <span className="inline-flex items-center gap-1">
          <WifiIcon className="size-3.5" />
          {t('cameraLive')}
        </span>
      </div>
      <div ref={frameRef} className="relative aspect-video w-full overflow-hidden rounded-md bg-black">
        <video
          ref={videoRef}
          aria-label={t('cameraTitle')}
          autoPlay
          className="aspect-video w-full bg-black object-contain"
          muted
          playsInline
          src={`/api/tenants/${apiIdSegment(printer.tenant_id, 'tenant_id')}/printers/${apiIdSegment(printer.id, 'printer_id')}/camera.mp4`}
        />
        <div className="absolute right-3 top-3 flex gap-2">
          {pictureInPictureSupported ? (
            <Button
              aria-label={t('cameraPictureInPicture')}
              className="rounded-md bg-black/70 text-white hover:bg-black/90 hover:text-white dark:hover:bg-black/90"
              onClick={() => void videoRef.current?.requestPictureInPicture()}
              size="icon"
              title={t('cameraPictureInPicture')}
              type="button"
              variant="ghost"
            >
              <PictureInPicture2Icon className="size-4" />
            </Button>
          ) : null}
          <Button
            aria-label={t('cameraFullscreen')}
            className="rounded-md bg-black/70 text-white hover:bg-black/90 hover:text-white dark:hover:bg-black/90"
            onClick={() => void frameRef.current?.requestFullscreen()}
            size="icon"
            title={t('cameraFullscreen')}
            type="button"
            variant="ghost"
          >
            <MaximizeIcon className="size-4" />
          </Button>
        </div>
        {task ? (
          <div className="absolute inset-x-3 bottom-3 rounded-md bg-black/55 px-3 py-2 text-xs text-white">
            <div className="flex items-center justify-between gap-2">
              <span className="truncate font-medium">{task}</span>
              {summaryParts.length > 0 ? (
                <span className="shrink-0">{summaryParts.join(' · ')}</span>
              ) : null}
            </div>
            {progress !== null ? (
              <div
                aria-label={tPrint('progress')}
                aria-valuemax={100}
                aria-valuemin={0}
                aria-valuenow={progress}
                className="mt-2 h-1.5 overflow-hidden rounded-full bg-white/25"
                role="progressbar"
              >
                <div className="h-full rounded-full bg-white" style={{ width: `${progress}%` }} />
              </div>
            ) : null}
          </div>
        ) : null}
      </div>
    </>
  )
}
