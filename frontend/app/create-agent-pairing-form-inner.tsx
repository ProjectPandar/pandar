'use client'

import { useActionState } from 'react'
import { useTranslations } from 'next-intl'
import { useQueryClient } from '@tanstack/react-query'
import { UserPlusIcon } from 'lucide-react'

import { createAgentPairing } from './admin-actions'
import { Button } from '@/components/ui/button'
import { inputClasses } from '@/lib/utils'
import { mutationResources, useInvalidateOnSuccess } from './mutation-invalidation'
import {
  Input,
  PrimaryButton,
  SecretActionResult,
} from './admin-panel-shared'

export function CreateAgentPairingFormInner({
  tenantId,
  onCreateAnother,
  inline = false,
}: {
  tenantId: string
  onCreateAnother: () => void
  inline?: boolean
}) {
  const t = useTranslations('admin')
  const queryClient = useQueryClient()
  const [state, formAction, pending] = useActionState(createAgentPairing, null)
  useInvalidateOnSuccess(state, queryClient, tenantId, mutationResources.agent)
  const locked = pending || state?.ok === true

  return (
    <form action={formAction} className="grid gap-3">
      <input name="tenant_id" type="hidden" value={tenantId} />
      {inline ? (
        <div className="flex flex-col gap-2 sm:flex-row sm:items-center">
          <input
            aria-label={t('agentName')}
            className={`${inputClasses} sm:max-w-xs`}
            disabled={locked}
            name="name"
            placeholder={t('agentName')}
            required
            type="text"
          />
          {locked ? null : (
            <div className="shrink-0">
              <PrimaryButton label={pending ? t('creating') : t('createPairing')} />
            </div>
          )}
        </div>
      ) : (
        <>
          <div className="text-sm font-semibold text-foreground">
            {t('pairAgent')}
          </div>
          <Input name="name" label={t('agentName')} required disabled={locked} />
          {locked ? null : (
            <PrimaryButton label={pending ? t('creating') : t('createPairing')} />
          )}
        </>
      )}
      <SecretActionResult state={state} />
      {state?.ok ? (
        <Button
          className="h-auto gap-1 self-start px-0 text-xs"
          onClick={onCreateAnother}
          type="button"
          variant="link"
        >
          <UserPlusIcon aria-hidden="true" className="size-3" />
          {t('createAnother')}
        </Button>
      ) : null}
    </form>
  )
}
