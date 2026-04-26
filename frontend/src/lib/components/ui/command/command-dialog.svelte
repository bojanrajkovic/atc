<script lang="ts">
  import type { Command as CommandPrimitive, Dialog as DialogPrimitive } from 'bits-ui'
  import type { Snippet } from 'svelte'
  import Command from './command.svelte'
  import * as Dialog from '$lib/components/ui/dialog/index.js'
  import { cn, type WithoutChildrenOrChild } from '$lib/utils.js'

  // API-surface extension — NOT theming (see project guidance Rule 7).
  // DialogPrimitive.RootProps does not include content-level dismissal props;
  // they must be forwarded explicitly to <Dialog.Content> so callers
  // (Phase 6 focus-restoration) can pass onCloseAutoFocus etc. The four new
  // fields below are extracted from the destructure so they do NOT bleed
  // into restProps (which spreads onto <Dialog.Root> and <Command>, neither
  // of which accepts these content-level props).
  let {
    open = $bindable(false),
    ref = $bindable(null),
    value = $bindable(''),
    title = 'Command Palette',
    description = 'Search for a command to run...',
    showCloseButton = false,
    portalProps,
    children,
    class: className,
    escapeKeydownBehavior,
    interactOutsideBehavior,
    onCloseAutoFocus,
    onOpenAutoFocus,
    ...restProps
  }: WithoutChildrenOrChild<DialogPrimitive.RootProps> &
    WithoutChildrenOrChild<CommandPrimitive.RootProps> & {
      portalProps?: DialogPrimitive.PortalProps
      children: Snippet
      title?: string
      description?: string
      showCloseButton?: boolean
      class?: string
      escapeKeydownBehavior?: DialogPrimitive.ContentProps['escapeKeydownBehavior']
      interactOutsideBehavior?: DialogPrimitive.ContentProps['interactOutsideBehavior']
      onCloseAutoFocus?: DialogPrimitive.ContentProps['onCloseAutoFocus']
      onOpenAutoFocus?: DialogPrimitive.ContentProps['onOpenAutoFocus']
    } = $props()
</script>

<Dialog.Root bind:open {...restProps}>
  <Dialog.Header class="sr-only">
    <Dialog.Title>{title}</Dialog.Title>
    <Dialog.Description>{description}</Dialog.Description>
  </Dialog.Header>
  <Dialog.Content
    class={cn('rounded-xl! top-1/3 translate-y-0 overflow-hidden p-0', className)}
    {showCloseButton}
    portalProps={portalProps as any}
    escapeKeydownBehavior={escapeKeydownBehavior as any}
    interactOutsideBehavior={interactOutsideBehavior as any}
    onCloseAutoFocus={onCloseAutoFocus as any}
    onOpenAutoFocus={onOpenAutoFocus as any}
  >
    <Command {...restProps} bind:value bind:ref {children} />
  </Dialog.Content>
</Dialog.Root>
