// Group settings (docs/groups.md „Gruppeneinstellungen“): replace the chat area, back arrow
// and Esc return to the chat.

import { useCallback, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react'
import type { MemberView, RoomView, UserView } from '@shared/core'
import { closeGroupInfo, leaveOrDelete, onParticipantsChanged, openRoom, useStore } from '../store'
import { toast } from '../call/store'
import { Avatar } from './Avatar'
import { ConfirmDialog, Modal } from './Modal'
import {
  CameraIcon,
  LoadError,
  MemberPicker,
  PaneHeader,
  PersonRow,
  PlaceholderRows,
  PlusIcon,
  readPhoto,
  RoundIcon,
  useEscape,
  usePhotoInput,
  useRevokeOnChange,
  type Photo,
} from './People'

/** Group settings exist for room types 2 (invite only) and 3 (with link). */
export function isGroupRoom(room: RoomView): boolean {
  return room.roomType === 2 || room.roomType === 3
}

const LEVELS: { level: number; label: string }[] = [
  { level: 1, label: 'Alle Nachrichten' },
  { level: 2, label: 'Nur Erwähnungen' },
  { level: 3, label: 'Aus' },
]

function levelLabel(level: number): string {
  return LEVELS.find((l) => l.level === level)?.label ?? 'Alle Nachrichten'
}

const FIRST_MEMBERS = 10

function memberSubline(m: MemberView): string | undefined {
  switch (m.actorType) {
    case 'guests':
      return 'Gast'
    case 'groups':
      return 'Nextcloud-Gruppe'
    case 'emails':
      return 'E-Mail'
    default:
      return undefined
  }
}

/** A failed call (rejected promise) as the German text of the contract. */
const offline = () => 'Keine Verbindung'

type Dialog =
  | { kind: 'edit' }
  | { kind: 'notifications' }
  | { kind: 'member'; member: MemberView }
  | { kind: 'remove'; member: MemberView }
  | { kind: 'leave' }
  | { kind: 'newModerator' }
  | { kind: 'delete' }

export function GroupInfo({ room }: { room: RoomView }) {
  const composing = useStore((s) => s.composing)
  const [members, setMembers] = useState<MemberView[] | null>(null)
  const [membersFailed, setMembersFailed] = useState(false)
  const [showAll, setShowAll] = useState(false)
  const [adding, setAdding] = useState(false)
  const [dialog, setDialog] = useState<Dialog | null>(null)
  const [busy, setBusy] = useState(false)
  const pane = useRef<HTMLDivElement>(null)
  const token = room.token

  const load = useCallback(() => {
    setMembersFailed(false)
    window.talk
      .members(token)
      .then((list) => {
        setMembers(list)
      })
      .catch(() => setMembersFailed(true))
  }, [token])

  useEffect(() => {
    load()
    return onParticipantsChanged((t) => t === token && load())
  }, [token, load])

  useEscape(
    pane,
    () => {
      if (adding) setAdding(false)
      else closeGroupInfo()
    },
    !composing,
  )

  const closeDialog = useCallback(() => {
    if (!busy) setDialog(null)
  }, [busy])

  /** Runs a change; on an error the toast shows it. Resolves to success. */
  async function run(action: () => Promise<string | null>): Promise<boolean> {
    setBusy(true)
    const error = await action().catch(offline)
    setBusy(false)
    if (error) toast(error)
    return !error
  }

  async function messageTo(m: MemberView) {
    setDialog(null)
    const res = await window.talk.openDirect(m.actorId).catch(() => ({ token: null, error: offline() }))
    if (res.token) void openRoom(res.token)
    else toast(res.error ?? 'Chat konnte nicht geöffnet werden')
  }

  async function leave() {
    setBusy(true)
    const error = await leaveOrDelete(token, false)
    setBusy(false)
    if (error) toast(error)
    else setDialog(null)
  }

  async function promoteAndLeave(m: MemberView) {
    setBusy(true)
    const error = (await window.talk.setModerator(token, m.attendeeId, true).catch(offline)) ?? (await leaveOrDelete(token, false))
    setBusy(false)
    if (error) toast(error)
    else setDialog(null)
  }

  async function remove() {
    setBusy(true)
    const error = await leaveOrDelete(token, true)
    setBusy(false)
    if (error) toast(error)
    else setDialog(null)
  }

  if (adding) {
    return (
      <section className="chat group-info" ref={pane}>
        <AddMembers token={token} onDone={() => setAdding(false)} />
      </section>
    )
  }

  const count = members?.length
  // The server still reports „can leave“ for the last moderator (the leave then fails), so the
  // member list decides as well: nobody else is a moderator, but there are others to promote.
  const lastModerator =
    !room.canLeave ||
    (room.isModerator && !!members && members.length > 1 && !members.some((m) => !m.isSelf && m.isModerator))
  const subtitle =
    count === undefined
      ? 'Gruppe'
      : `${room.isPublic ? 'Gruppe mit Link' : 'Gruppe'} · ${count === 1 ? '1 Mitglied' : `${count} Mitglieder`}`
  const shown = members && !showAll ? members.slice(0, FIRST_MEMBERS) : members

  return (
    <section className="chat group-info" ref={pane}>
      <PaneHeader title="Gruppeneinstellungen" onBack={closeGroupInfo} backLabel="Zurück zum Chat" />
      <div className="settings-scroll">
        <div className="settings">
          <div className="group-head">
            <Avatar key={room.avatarVersion} name={room.name} token={token} version={room.avatarVersion} size={80} />
            <h1 className="group-name">{room.name}</h1>
            <p className="muted group-sub">{subtitle}</p>
            {room.description ? (
              <Description text={room.description} />
            ) : (
              room.isModerator && (
                <button className="text-btn" onClick={() => setDialog({ kind: 'edit' })}>
                  Beschreibung hinzufügen
                </button>
              )
            )}
            {room.isModerator && (
              <button className="pill-btn" onClick={() => setDialog({ kind: 'edit' })}>
                Bearbeiten
              </button>
            )}
          </div>

          <Section>
            <SettingsRow icon={<BellIcon />} label="Benachrichtigungen" value={levelLabel(room.notificationLevel)} onClick={() => setDialog({ kind: 'notifications' })} />
          </Section>

          <Section title={count === undefined ? 'Mitglieder' : count === 1 ? '1 Mitglied' : `${count} Mitglieder`}>
            {room.isModerator && (
              <button className="person-row" onClick={() => setAdding(true)}>
                <RoundIcon>
                  <PlusIcon />
                </RoundIcon>
                <span className="person-text">
                  <span className="person-name">Mitglieder hinzufügen</span>
                </span>
              </button>
            )}
            {membersFailed ? (
              <LoadError text="Konnte nicht geladen werden" retry={load} />
            ) : shown === null ? (
              <PlaceholderRows />
            ) : (
              shown.map((m) => (
                <PersonRow
                  key={m.attendeeId}
                  name={m.isSelf ? 'Du' : m.name}
                  userId={m.actorType === 'users' ? m.actorId : undefined}
                  subline={memberSubline(m)}
                  label={m.isModerator ? `${m.isSelf ? 'Du' : m.name}, Moderator` : m.isSelf ? 'Du' : m.name}
                  onClick={m.isSelf ? undefined : () => setDialog({ kind: 'member', member: m })}
                  trailing={m.isModerator ? <span className="role-badge">Moderator</span> : undefined}
                />
              ))
            )}
            {members && !showAll && members.length > FIRST_MEMBERS && (
              <button className="settings-row center-row" onClick={() => setShowAll(true)}>
                Alle anzeigen
              </button>
            )}
          </Section>

          {(room.isModerator || room.isPublic) && <GroupLinkSection room={room} busy={busy} run={run} />}

          <Section>
            <SettingsRow
              icon={<LeaveIcon />}
              label="Gruppe verlassen"
              danger
              onClick={() => setDialog({ kind: lastModerator ? 'newModerator' : 'leave' })}
            />
            {room.canDelete && <SettingsRow icon={<TrashIcon />} label="Gruppe löschen" danger onClick={() => setDialog({ kind: 'delete' })} />}
          </Section>
        </div>
      </div>

      {dialog?.kind === 'edit' && <EditGroup room={room} onClose={() => setDialog(null)} />}
      {dialog?.kind === 'notifications' && (
        <Modal label="Benachrichtigungen" className="sheet" onClose={closeDialog}>
          <h2 className="dialog-title">Benachrichtigungen</h2>
          <div className="choice-list" role="radiogroup" aria-label="Benachrichtigungen">
            {LEVELS.map((l) => {
              const on = (room.notificationLevel || 1) === l.level
              return (
                <button
                  key={l.level}
                  role="radio"
                  aria-checked={on}
                  className="choice"
                  data-autofocus={on ? true : undefined}
                  disabled={busy}
                  onClick={async () => {
                    if (on || (await run(() => window.talk.setNotificationLevel(token, l.level)))) setDialog(null)
                  }}
                >
                  <span className={`radio${on ? ' on' : ''}`} aria-hidden />
                  {l.label}
                </button>
              )
            })}
          </div>
        </Modal>
      )}
      {dialog?.kind === 'member' && (
        <MemberSheet
          member={dialog.member}
          canManage={room.isModerator && !dialog.member.isOwner}
          busy={busy}
          onClose={closeDialog}
          onMessage={() => void messageTo(dialog.member)}
          onModerator={async () => {
            const m = dialog.member
            if (await run(() => window.talk.setModerator(token, m.attendeeId, !m.isModerator))) setDialog(null)
          }}
          onRemove={() => setDialog({ kind: 'remove', member: dialog.member })}
        />
      )}
      {dialog?.kind === 'remove' && (
        <ConfirmDialog
          title={`${dialog.member.name} aus der Gruppe entfernen?`}
          confirm="Entfernen"
          danger
          busy={busy}
          onCancel={closeDialog}
          onConfirm={async () => {
            if (await run(() => window.talk.removeMember(token, dialog.member.attendeeId))) setDialog(null)
          }}
        />
      )}
      {dialog?.kind === 'leave' && (
        <ConfirmDialog
          title="Gruppe verlassen?"
          text="Du erhältst keine Nachrichten mehr aus dieser Gruppe."
          confirm="Verlassen"
          danger
          busy={busy}
          onCancel={closeDialog}
          onConfirm={() => void leave()}
        />
      )}
      {dialog?.kind === 'newModerator' && (
        <Modal label="Neuen Moderator wählen" className="sheet" onClose={closeDialog}>
          <h2 className="dialog-title">Neuen Moderator wählen</h2>
          <p className="dialog-text">Bevor du gehst, ernenne jemanden zum Moderator.</p>
          <div className="sheet-list">
            {members === null ? (
              <PlaceholderRows count={2} />
            ) : members.filter((m) => !m.isSelf).length === 0 ? (
              <p className="muted center pad">Es gibt keine anderen Mitglieder. Du kannst die Gruppe stattdessen löschen.</p>
            ) : (
              members
                .filter((m) => !m.isSelf)
                .map((m) => (
                  <PersonRow
                    key={m.attendeeId}
                    name={m.name}
                    userId={m.actorType === 'users' ? m.actorId : undefined}
                    subline={memberSubline(m)}
                    disabled={busy}
                    onClick={() => void promoteAndLeave(m)}
                  />
                ))
            )}
          </div>
          <div className="dialog-buttons">
            <button className="dialog-btn" onClick={closeDialog} disabled={busy} data-autofocus>
              Abbrechen
            </button>
          </div>
        </Modal>
      )}
      {dialog?.kind === 'delete' && (
        <ConfirmDialog
          title="Gruppe für alle löschen?"
          text="Alle Nachrichten gehen verloren."
          confirm="Löschen"
          danger
          busy={busy}
          onCancel={closeDialog}
          onConfirm={() => void remove()}
        />
      )}
    </section>
  )
}

function Section({ title, children }: { title?: string; children: ReactNode }) {
  return (
    <section className="settings-section">
      {title && <h3 className="settings-heading">{title}</h3>}
      {children}
    </section>
  )
}

function SettingsRow({
  icon,
  label,
  sub,
  value,
  danger,
  onClick,
  trailing,
  role,
  checked,
  disabled,
}: {
  icon?: ReactNode
  label: string
  sub?: string
  value?: string
  danger?: boolean
  onClick: () => void
  trailing?: ReactNode
  role?: 'switch'
  checked?: boolean
  disabled?: boolean
}) {
  return (
    <button
      className={`settings-row${danger ? ' danger' : ''}`}
      onClick={onClick}
      role={role}
      aria-checked={checked}
      disabled={disabled}
    >
      {icon && <span className="settings-icon">{icon}</span>}
      <span className="settings-text">
        <span>{label}</span>
        {sub && <span className="person-sub">{sub}</span>}
      </span>
      {value && <span className="settings-value">{value}</span>}
      {trailing}
    </button>
  )
}

/** Up to three lines with „Mehr“. */
function Description({ text }: { text: string }) {
  const ref = useRef<HTMLParagraphElement>(null)
  const [open, setOpen] = useState(false)
  const [overflows, setOverflows] = useState(false)
  useLayoutEffect(() => {
    const el = ref.current
    if (el && !open) setOverflows(el.scrollHeight > el.clientHeight + 1)
  }, [text, open])
  return (
    <div className="group-desc">
      <p ref={ref} className={open ? '' : 'clamp3'}>
        {text}
      </p>
      {overflows && !open && (
        <button className="text-btn" onClick={() => setOpen(true)}>
          Mehr
        </button>
      )}
    </div>
  )
}

function GroupLinkSection({
  room,
  busy,
  run,
}: {
  room: RoomView
  busy: boolean
  run: (action: () => Promise<string | null>) => Promise<boolean>
}) {
  const [link, setLink] = useState<string | null>(null)
  useEffect(() => {
    if (!room.isPublic) return setLink(null)
    let alive = true
    window.talk
      .groupLink(room.token)
      .then((l) => alive && setLink(l))
      .catch(() => {})
    return () => {
      alive = false
    }
  }, [room.isPublic, room.token])

  return (
    <Section title="Gruppenlink">
      {room.isModerator && (
        <SettingsRow
          icon={<LinkIcon />}
          label="Gruppenlink"
          sub="Jeder mit dem Link kann beitreten"
          role="switch"
          checked={room.isPublic}
          disabled={busy}
          onClick={() => void run(() => window.talk.setGroupLink(room.token, !room.isPublic))}
          trailing={<span className={`switch${room.isPublic ? ' on' : ''}`} aria-hidden />}
        />
      )}
      {room.isPublic && (
        <>
          <div className="settings-row static">
            <span className="settings-icon" />
            <span className="settings-text">
              <span className="ellipsis link-text" title={link ?? undefined}>
                {link ?? '…'}
              </span>
            </span>
          </div>
          <SettingsRow
            icon={<CopyIcon />}
            label="Link kopieren"
            disabled={!link}
            onClick={() => {
              if (!link) return
              navigator.clipboard.writeText(link).then(
                () => toast('Link kopiert'),
                () => toast('Kopieren nicht möglich'),
              )
            }}
          />
        </>
      )}
    </Section>
  )
}

function MemberSheet({
  member,
  canManage,
  busy,
  onClose,
  onMessage,
  onModerator,
  onRemove,
}: {
  member: MemberView
  canManage: boolean
  busy: boolean
  onClose: () => void
  onMessage: () => void
  onModerator: () => void
  onRemove: () => void
}) {
  return (
    <Modal label={member.name} className="sheet member-sheet" onClose={onClose}>
      <div className="member-head">
        <Avatar name={member.name} userId={member.actorType === 'users' ? member.actorId : undefined} size={64} />
        <h2>{member.name}</h2>
        {member.isModerator && <span className="role-badge">Moderator</span>}
      </div>
      <div className="sheet-list">
        {member.actorType === 'users' && (
          <SettingsRow icon={<ChatIcon />} label="Nachricht senden" onClick={onMessage} disabled={busy} />
        )}
        {canManage && (
          <SettingsRow
            icon={<ShieldIcon />}
            label={member.isModerator ? 'Moderatorrechte entziehen' : 'Zum Moderator machen'}
            onClick={onModerator}
            disabled={busy}
          />
        )}
        {canManage && <SettingsRow icon={<RemoveIcon />} label="Aus der Gruppe entfernen" danger onClick={onRemove} disabled={busy} />}
      </div>
    </Modal>
  )
}

/** „Mitglieder hinzufügen“ from the settings: the picker without the second step. */
function AddMembers({ token, onDone }: { token: string; onDone: () => void }) {
  const [selected, setSelected] = useState<UserView[]>([])
  const [busy, setBusy] = useState(false)
  async function add() {
    setBusy(true)
    const error = await window.talk
      .addMembers(
        token,
        selected.map((u) => u.id),
      )
      .catch(offline)
    setBusy(false)
    if (error) toast(error)
    else onDone()
  }
  return (
    <>
      <PaneHeader title="Mitglieder hinzufügen" onBack={() => !busy && onDone()} />
      <div className="picker-pane">
        <MemberPicker
          excludeToken={token}
          selected={selected}
          onChange={setSelected}
          action={selected.length > 0 ? `Hinzufügen (${selected.length})` : 'Hinzufügen'}
          actionDisabled={selected.length === 0}
          busy={busy}
          onAction={() => void add()}
        />
      </div>
    </>
  )
}

/** „Gruppe bearbeiten“: photo, name, description; saves the changed fields one after another. */
function EditGroup({ room, onClose }: { room: RoomView; onClose: () => void }) {
  const [name, setName] = useState(room.name)
  const [description, setDescription] = useState(room.description)
  /** undefined: unchanged, null: remove, else the new picture. */
  const [photo, setPhoto] = useState<Photo | null | undefined>(undefined)
  const [busy, setBusy] = useState(false)
  const input = usePhotoInput((file) => void readPhoto(file).then(setPhoto))
  useRevokeOnChange(photo)

  const hasPhoto = photo === undefined ? room.hasCustomAvatar : photo !== null
  const nameChanged = name.trim() !== room.name
  const descChanged = description.trim() !== room.description.trim()

  async function save() {
    if (busy) return
    if (!name.trim()) {
      toast('Bitte einen Gruppennamen eingeben')
      return
    }
    setBusy(true)
    const steps: (() => Promise<string | null>)[] = []
    if (nameChanged) steps.push(() => window.talk.renameGroup(room.token, name.trim()))
    if (descChanged) steps.push(() => window.talk.setDescription(room.token, description.trim()))
    if (photo !== undefined) steps.push(() => window.talk.setGroupPhoto(room.token, photo?.bytes ?? null))
    for (const step of steps) {
      const error = await step().catch(offline)
      if (error) {
        setBusy(false)
        toast(error)
        return
      }
    }
    setBusy(false)
    onClose()
  }

  // The current picture of the group (cache-busted by the avatar version).
  const current = window.talk.avatarUrl(room.token, room.avatarVersion)

  return (
    <Modal label="Gruppe bearbeiten" className="sheet edit-group" onClose={() => !busy && onClose()}>
      <h2 className="dialog-title">Gruppe bearbeiten</h2>
      <div className="photo-pick-wrap">
        <button className="photo-pick" onClick={input.pick} aria-label="Gruppenbild wählen" title="Gruppenbild wählen" disabled={busy}>
          {photo ? (
            <img src={photo.url} alt="" />
          ) : photo === undefined && room.hasCustomAvatar ? (
            <img src={current} alt="" />
          ) : (
            <CameraIcon />
          )}
        </button>
        {input.element}
        <div className="photo-actions">
          <button className="text-btn" onClick={input.pick} disabled={busy}>
            Ändern
          </button>
          {hasPhoto && (
            <button className="text-btn" onClick={() => setPhoto(null)} disabled={busy}>
              Entfernen
            </button>
          )}
        </div>
      </div>
      <label className="field">
        <span className="field-label">Name</span>
        <input
          className="input"
          maxLength={255}
          value={name}
          disabled={busy}
          data-autofocus
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && void save()}
        />
      </label>
      <label className="field">
        <span className="field-label">Beschreibung</span>
        <textarea
          className="input textarea"
          rows={4}
          maxLength={2000}
          value={description}
          disabled={busy}
          onChange={(e) => setDescription(e.target.value)}
        />
        {description.length >= 1800 && <span className="field-count">{description.length} / 2000</span>}
      </label>
      <div className="dialog-buttons">
        <button className="dialog-btn" onClick={onClose} disabled={busy}>
          Abbrechen
        </button>
        <button className="dialog-btn primary" onClick={() => void save()} disabled={busy || !name.trim()}>
          {busy ? <span className="spinner tiny light" aria-label="Wird gespeichert" /> : 'Speichern'}
        </button>
      </div>
    </Modal>
  )
}

// --- icons (own drawings) ---------------------------------------------------------

function Icon({ d, children }: { d?: string; children?: ReactNode }) {
  return (
    <svg viewBox="0 0 24 24" width="20" height="20" aria-hidden fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
      {d && <path d={d} />}
      {children}
    </svg>
  )
}
const BellIcon = () => <Icon d="M6 16V11a6 6 0 0 1 12 0v5l1.5 2h-15zM10 20.5a2.2 2.2 0 0 0 4 0" />
const LeaveIcon = () => <Icon d="M10 5H5v14h5M14 8l4 4-4 4M18 12H9" />
const TrashIcon = () => <Icon d="M4.5 7h15M9 7V4.5h6V7M6.5 7l1 13h9l1-13M10 11v5.5M14 11v5.5" />
const LinkIcon = () => <Icon d="M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1" />
const CopyIcon = () => (
  <Icon d="M15 8V5H5v10h3">
    <rect x="9" y="9" width="10" height="10" rx="1.5" />
  </Icon>
)
const ChatIcon = () => <Icon d="M4.5 5h15v11h-9l-4.5 3.5V16h-1.5z" />
const ShieldIcon = () => <Icon d="M12 3.5l7 2.5v5.5c0 4.3-3 7.6-7 9-4-1.4-7-4.7-7-9V6z" />
const RemoveIcon = () => (
  <Icon d="M3.5 19c.6-3.3 3-5 6-5s5.4 1.7 6 5M16 10.5h5">
    <circle cx="9.5" cy="8.5" r="3.2" />
  </Icon>
)
