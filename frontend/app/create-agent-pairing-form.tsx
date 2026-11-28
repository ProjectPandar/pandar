'use client'

import { useState } from 'react'

import { CreateAgentPairingFormInner } from './create-agent-pairing-form-inner'

export function CreateAgentPairingForm({
  tenantId,
  inline = false,
}: {
  tenantId: string
  inline?: boolean
}) {
  const [nonce, setNonce] = useState(0)
  return (
    <CreateAgentPairingFormInner
      inline={inline}
      key={nonce}
      onCreateAnother={() => setNonce((value) => value + 1)}
      tenantId={tenantId}
    />
  )
}
