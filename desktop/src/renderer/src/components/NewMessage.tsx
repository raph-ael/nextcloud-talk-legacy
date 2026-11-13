// „Neue Nachricht“ → „Mitglieder hinzufügen“ → „Gruppe benennen“ (docs/groups.md). Replaces
// the conversation list as steps with a back arrow; Esc goes one step back.

import { useEffect, useRef, useState } from 'react'
import type { UserView } from '@shared/core'
import { closeCompose, openRoom } from '../store'
import { toast } from '../call/store'
import {
  CameraIcon,
  MemberPicker,
  PaneHeader,
  PeopleIcon,
  PersonRow,
  readPhoto,
  RoundIcon,
  useEscape,
  usePhotoInput,
  useRevokeOnChange,
  UserResults,
  useUserSearch,
  type Photo,
} from './People'

type Step = 'contacts' | 'members' | 'name'

export function NewMessage() {
  const [step, setStep] = useState<Step>('contacts')
  const [selected, setSelected] = useState<UserView[]>([])
  const [name, setName] = useState('')
  const [photo, setPhoto] = useState<Photo | null>(null)
  const [busy, setBusy] = useState(false)
  const pane = useRef<HTMLDivElement>(null)
  useRevokeOnChange(photo)

  function back() {
    if (busy) return
    if (step === 'name') setStep('members')
    else if (step === 'members') setStep('contacts')
    else closeCompose()
  }
  useEscape(pane, back)

  async function create() {
    const trimmed = name.trim()
    if (!trimmed || busy) return
    setBusy(true)
    const res = await window.talk
      .createGroup(
        trimmed,
        selected.map((u) => u.id),
        photo?.bytes ?? null,
      )
      .catch(() => ({ token: null, error: 'Keine Verbindung' }))
    setBusy(false)
    if (res.token) {
      closeCompose()
      void openRoom(res.token)
    } else {
      toast(res.error ?? 'Gruppe konnte nicht erstellt werden')
    }
  }

  return (
    <div className="compose" ref={pane}>
      {step === 'contacts' && <Contacts onBack={back} onNewGroup={() => setStep('members')} />}
      {step === 'members' && (
        <>
          <PaneHeader title="Mitglieder hinzufügen" onBack={back} />
          <MemberPicker
            excludeToken={null}
            selected={selected}
            onChange={setSelected}
            action={selected.length > 0 ? 'Weiter' : 'Überspringen'}
            onAction={() => setStep('name')}
          />
        </>
      )}
      {step === 'name' && (
        <NameGroup
          name={name}
          setName={setName}
          photo={photo}
          setPhoto={setPhoto}
          members={selected}
          busy={busy}
          onBack={back}
          onCreate={() => void create()}
        />
      )}
    </div>
  )
}

function Contacts({ onBack, onNewGroup }: { onBack: () => void; onNewGroup: () => void }) {
  const [query, setQuery] = useState('')
  const [canCreate, setCanCreate] = useState(false)
  const [opening, setOpening] = useState<string | null>(null)
  const search = useUserSearch(query, null)

  useEffect(() => {
    let alive = true
    window.talk
      .canCreateGroups()
      .then((ok) => alive && setCanCreate(ok))
      .catch(() => {})
    return () => {
      alive = false
    }
  }, [])

  async function open(u: UserView) {
    if (opening) return
    setOpening(u.id)
    const res = await window.talk.openDirect(u.id).catch(() => ({ token: null, error: 'Keine Verbindung' }))
    setOpening(null)
    if (res.token) {
      closeCompose()
      void openRoom(res.token)
    } else {
      toast(res.error ?? 'Chat konnte nicht geöffnet werden')
    }
  }

  return (
    <>
      <PaneHeader title="Neue Nachricht" onBack={onBack} />
      <div className="search">
        <input
          className="input search-input"
          placeholder="Name suchen"
          value={query}
          autoFocus
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && search.users?.[0]) void open(search.users[0])
          }}
        />
      </div>
      <div className="people-list">
        {canCreate && !query.trim() && (
          <button className="person-row" onClick={onNewGroup}>
            <RoundIcon>
              <PeopleIcon />
            </RoundIcon>
            <span className="person-text">
              <span className="person-name">Neue Gruppe</span>
            </span>
          </button>
        )}
        <h3 className="list-section">Kontakte</h3>
        <UserResults
          {...search}
          row={(u) => (
            <PersonRow
              key={u.id}
              name={u.name}
              userId={u.id}
              subline={u.subline}
              onClick={() => void open(u)}
              disabled={opening !== null}
              trailing={opening === u.id ? <span className="spinner tiny" aria-label="Wird geöffnet" /> : undefined}
            />
          )}
        />
      </div>
    </>
  )
}

function NameGroup({
  name,
  setName,
  photo,
  setPhoto,
  members,
  busy,
  onBack,
  onCreate,
}: {
  name: string
  setName: (s: string) => void
  photo: Photo | null
  setPhoto: (p: Photo | null) => void
  members: UserView[]
  busy: boolean
  onBack: () => void
  onCreate: () => void
}) {
  const input = usePhotoInput((file) => void readPhoto(file).then(setPhoto))
  return (
    <>
      <PaneHeader title="Gruppe benennen" onBack={onBack} />
      <div className="people-list name-group">
        <div className="photo-pick-wrap">
          <button className="photo-pick" onClick={input.pick} aria-label="Gruppenbild wählen" title="Gruppenbild wählen" disabled={busy}>
            {photo ? <img src={photo.url} alt="" /> : <CameraIcon />}
          </button>
          {input.element}
          {photo && (
            <button className="text-btn" onClick={() => setPhoto(null)} disabled={busy}>
              Entfernen
            </button>
          )}
        </div>
        <div className="field-pad">
          <input
            className="input"
            placeholder="Gruppenname (erforderlich)"
            aria-label="Gruppenname (erforderlich)"
            maxLength={255}
            value={name}
            autoFocus
            disabled={busy}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && onCreate()}
          />
        </div>
        <h3 className="list-section">
          {members.length === 1 ? '1 Mitglied' : `${members.length} Mitglieder`}
        </h3>
        {members.map((u) => (
          <PersonRow key={u.id} name={u.name} userId={u.id} subline={u.subline} />
        ))}
      </div>
      <footer className="pane-footer">
        <button className="button-primary" onClick={onCreate} disabled={!name.trim() || busy}>
          {busy ? <span className="spinner tiny light" aria-label="Wird erstellt" /> : 'Erstellen'}
        </button>
      </footer>
    </>
  )
}
