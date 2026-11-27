'use client'

import { useRef, useState } from 'react'
import { useTranslations } from 'next-intl'
import { Loader2Icon, MoreVerticalIcon, PencilIcon, RotateCwIcon, TrashIcon } from 'lucide-react'

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from '@/components/ui/popover'
import { inputClasses } from '@/lib/utils'
import { deletePrinter, refreshPrinterMaterials, updatePrinter } from './actions'
import type { Printer } from './dashboard-types'
import { useActionStatusFeedback } from './mutation-feedback'
import { mutationResources } from './mutation-invalidation'
import { ConfirmDialog } from './confirm-dialog'

/**
 * Printer maintenance actions. `menu` keeps everything behind the ⋯ trigger;
 * `split` also surfaces refresh-AMS and edit as buttons next to it.
 */
export function PrinterActions({
  printer,
  layout = 'menu',
}: {
  printer: Printer
  layout?: 'menu' | 'split'
}) {
  const t = useTranslations('inventory')
  const formRef = useRef<HTMLFormElement>(null)
  const [menuOpen, setMenuOpen] = useState(false)
  const [confirmOpen, setConfirmOpen] = useState(false)
  const [editOpen, setEditOpen] = useState(false)
  const refreshAction = useActionStatusFeedback(
    refreshPrinterMaterials,
    'materials_refresh_queued',
    { invalidate: mutationResources.printer },
  )
  const deleteAction = useActionStatusFeedback(deletePrinter, 'printer_deleted', {
    invalidate: mutationResources.printer,
    onSuccess: () => setConfirmOpen(false),
  })
  const editAction = useActionStatusFeedback(updatePrinter, 'printer_updated', {
    invalidate: mutationResources.printer,
    onSuccess: () => setEditOpen(false),
  })
  const refreshForm = refreshAction.formAction

  return (
    <div className="flex shrink-0 items-center gap-2">
      {layout === 'split' ? (
        <>
          <form action={refreshForm}>
            <PrinterIdentityFields printer={printer} />
            <Button disabled={refreshAction.pending} size="sm" type="submit" variant="outline">
              {refreshAction.pending ? <Loader2Icon className="animate-spin" /> : <RotateCwIcon />}
              {t('refreshAms')}
            </Button>
          </form>
          <Button onClick={() => setEditOpen(true)} size="sm" type="button" variant="outline">
            <PencilIcon />
            {t('editPrinter')}
          </Button>
        </>
      ) : null}
      <Popover open={menuOpen} onOpenChange={setMenuOpen}>
        <PopoverTrigger
          render={
            <Button
              aria-haspopup="dialog"
              aria-label={t('detailsFor', { name: printer.name })}
              className="text-muted-foreground hover:text-foreground"
              size={layout === 'split' ? 'icon' : 'icon-xs'}
              type="button"
              variant="ghost"
            />
          }
        >
          <MoreVerticalIcon className="size-4" />
        </PopoverTrigger>
        <PopoverContent
          align="end"
          className="w-auto min-w-36 gap-0 p-1"
          side="bottom"
          sideOffset={4}
        >
          <Button
            className="h-auto w-full justify-start gap-2 rounded-sm px-2 py-1.5 font-normal"
            onClick={() => {
              setMenuOpen(false)
              setEditOpen(true)
            }}
            type="button"
            variant="ghost"
          >
            <PencilIcon className="size-4" />
            {t('editPrinter')}
          </Button>
          <form action={refreshForm}>
            <PrinterIdentityFields printer={printer} />
            <Button
              className="h-auto w-full justify-start gap-2 rounded-sm px-2 py-1.5 font-normal"
              disabled={refreshAction.pending}
              onClick={() => setMenuOpen(false)}
              type="submit"
              variant="ghost"
            >
              {refreshAction.pending ? (
                <Loader2Icon className="size-4 animate-spin" />
              ) : (
                <RotateCwIcon className="size-4" />
              )}
              {t('refreshAms')}
            </Button>
          </form>
          <div className="my-1 border-t border-border" />
          <Button
            className="h-auto w-full justify-start gap-2 rounded-sm px-2 py-1.5 font-normal text-destructive hover:text-destructive"
            onClick={() => {
              setMenuOpen(false)
              setConfirmOpen(true)
            }}
            type="button"
            variant="ghost"
          >
            <TrashIcon className="size-4" />
            {t('deletePrinter')}
          </Button>
        </PopoverContent>
      </Popover>
      <form ref={formRef} action={deleteAction.formAction}>
        <PrinterIdentityFields printer={printer} />
      </form>
      <ConfirmDialog
        open={confirmOpen}
        title={t('deletePrinterTitle')}
        message={t('deletePrinterMessage', { name: printer.name })}
        confirmLabel={t('deletePrinterConfirm')}
        tone="danger"
        pending={deleteAction.pending}
        onConfirm={() => {
          formRef.current?.requestSubmit()
        }}
        onCancel={() => setConfirmOpen(false)}
      />
      <Dialog open={editOpen} onOpenChange={setEditOpen}>
        <DialogContent closeLabel={t('closeDialog')}>
          <DialogHeader>
            <DialogTitle>{t('editPrinterTitle')}</DialogTitle>
            <DialogDescription>{t('editPrinterDescription')}</DialogDescription>
          </DialogHeader>
          <form action={editAction.formAction} className="grid gap-4">
            <PrinterIdentityFields printer={printer} />
            <label className="flex flex-col gap-1 text-sm">
              <span className="text-xs font-medium text-muted-foreground">{t('editPrinterName')}</span>
              <input
                className={inputClasses}
                defaultValue={printer.name}
                name="name"
                required
                type="text"
              />
            </label>
            <label className="flex flex-col gap-1 text-sm">
              <span className="text-xs font-medium text-muted-foreground">{t('editPrinterHost')}</span>
              <input
                className={inputClasses}
                name="host"
                type="text"
              />
            </label>
            <label className="flex flex-col gap-1 text-sm">
              <span className="text-xs font-medium text-muted-foreground">{t(printer.printer_family === 'moonraker' ? 'editPrinterApiKey' : 'editPrinterAccessCode')}</span>
              <input
                autoComplete="off"
                className={inputClasses}
                name="access_code"
                type="password"
              />
            </label>
            <DialogFooter className="-mx-4 -mb-4 mt-2">
              <Button disabled={editAction.pending} size="lg" type="submit">
                {editAction.pending ? <Loader2Icon className="animate-spin" /> : null}
                {t('editPrinterSubmit')}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </div>
  )
}

function PrinterIdentityFields({ printer }: { printer: Printer }) {
  return (
    <>
      <input name="tenant_id" type="hidden" value={printer.tenant_id} />
      <input name="printer_id" type="hidden" value={printer.id} />
    </>
  )
}
