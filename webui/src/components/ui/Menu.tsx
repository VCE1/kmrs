import * as RadixMenu from '@radix-ui/react-dropdown-menu'
import { useRef, type ReactNode } from 'react'
import { cn } from '@/lib/utils/cn'

interface MenuProps {
  trigger: ReactNode
  children: ReactNode
  align?: 'start' | 'center' | 'end'
  side?: 'top' | 'bottom' | 'left' | 'right'
  /** size the popup to the trigger's width (e.g. the sidebar account button) */
  matchTriggerWidth?: boolean
  open?: boolean
  onOpenChange?: (open: boolean) => void
}

export function Menu({ trigger, children, align = 'end', side = 'bottom', matchTriggerWidth, open, onOpenChange }: MenuProps) {
  // Radix returns focus to the trigger on close, which leaves a :focus-visible ring after
  // pointer use; skip the restore for pointer-opened menus, keep it for keyboard
  const pointerOpened = useRef(false)
  return (
    <RadixMenu.Root open={open} onOpenChange={onOpenChange}>
      <RadixMenu.Trigger asChild onPointerDown={() => (pointerOpened.current = true)}>
        {trigger}
      </RadixMenu.Trigger>
      <RadixMenu.Portal>
        <RadixMenu.Content
          align={align}
          side={side}
          sideOffset={6}
          onCloseAutoFocus={(e) => {
            if (pointerOpened.current) {
              e.preventDefault()
              pointerOpened.current = false
            }
          }}
          className={cn(
            // menus can open inside dialogs (z-40), so the popup must stack above them
            'z-50 min-w-44 overflow-hidden rounded-xl border border-line bg-raised p-1 shadow-pop',
            matchTriggerWidth && 'w-[var(--radix-dropdown-menu-trigger-width)]',
            'origin-[var(--radix-dropdown-menu-content-transform-origin)] data-[state=open]:animate-menu-in data-[state=closed]:animate-menu-out',
          )}
        >
          {children}
        </RadixMenu.Content>
      </RadixMenu.Portal>
    </RadixMenu.Root>
  )
}

export function MenuItem({
  children,
  onSelect,
  danger,
  disabled,
}: {
  children: ReactNode
  onSelect?: () => void
  danger?: boolean
  disabled?: boolean
}) {
  return (
    <RadixMenu.Item
      onSelect={onSelect}
      disabled={disabled}
      className={cn(
        'flex cursor-pointer items-center gap-2.5 rounded-lg px-2.5 py-2 text-[13px] transition-colors outline-none select-none',
        danger
          ? 'text-danger data-[highlighted]:bg-danger/10'
          : 'text-ink data-[highlighted]:bg-accent-soft data-[highlighted]:text-accent-strong',
        'data-[disabled]:pointer-events-none data-[disabled]:opacity-40',
      )}
    >
      {children}
    </RadixMenu.Item>
  )
}

export function MenuSub({ trigger, children }: { trigger: ReactNode; children: ReactNode }) {
  return (
    <RadixMenu.Sub>
      <RadixMenu.SubTrigger
        className="flex cursor-pointer items-center gap-2.5 rounded-lg px-2.5 py-2 text-[13px] text-ink transition-colors outline-none select-none data-[highlighted]:bg-accent-soft data-[highlighted]:text-accent-strong data-[state=open]:bg-accent-soft data-[state=open]:text-accent-strong"
      >
        {trigger}
      </RadixMenu.SubTrigger>
      <RadixMenu.Portal>
        <RadixMenu.SubContent
          sideOffset={6}
          className="z-50 min-w-44 overflow-hidden rounded-xl border border-line bg-raised p-1 shadow-pop data-[state=open]:animate-menu-in data-[state=closed]:animate-menu-out"
        >
          {children}
        </RadixMenu.SubContent>
      </RadixMenu.Portal>
    </RadixMenu.Sub>
  )
}

export function MenuSeparator() {
  return <RadixMenu.Separator className="mx-1 my-1 h-px bg-line" />
}

export function MenuLabel({ children }: { children: ReactNode }) {
  return (
    <RadixMenu.Label className="px-2.5 pt-1.5 pb-1 text-[11px] font-medium tracking-wide text-ink-3 uppercase">
      {children}
    </RadixMenu.Label>
  )
}
