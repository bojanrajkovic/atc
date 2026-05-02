import { getContext, setContext } from 'svelte'
import type { RunId } from '$lib/types/generated/RunId'
import type { Columns } from './geometry'

/**
 * The full shape of the roving focus context shared between RovingFocusProvider
 * and its descendants (KanbanBoard, RunCard).
 *
 * Consumers must be wrapped in a <RovingFocusProvider> — call getRovingContext()
 * to retrieve the context; it throws with a descriptive message if missing.
 */
export interface RovingFocusContext {
  /** Explicit user-set focus target. Null means "fall back to initial". */
  readonly focusedRunId: bigint | null

  /** First card in first non-empty VISIBLE column, derived from visibleColumns in the provider. */
  readonly initialFocusRunId: bigint | null

  /** Effective target: focusedRunId ?? initialFocusRunId. */
  readonly currentFocusRunId: bigint | null

  /** Toggled by the action's focusin/focusout listeners. */
  readonly kanbanHasFocus: boolean

  /**
   * Snapshot of the visible columns the kanban DOM renders.
   * Computed by the provider from runStore columns filtered by uiStore.activePoolFilter.
   * Callers must not mutate the returned tuple — it is a live snapshot.
   * Both action.ts (geometry resolution) and the eviction $effect consume this
   * so the roving source of truth and the rendered DOM cannot diverge under a filter.
   */
  getVisibleColumns(): Columns

  /** Set the explicitly focused run id. */
  setFocus(id: RunId | null): void

  /** Toggle the kanban-has-focus flag. */
  setKanbanHasFocus(value: boolean): void

  /** Reset focusedRunId to null so currentFocusRunId falls back to initialFocusRunId. */
  restoreFocusToInitial(): Promise<void>
}

/**
 * Unique symbol used as the Svelte context key. Using a symbol rather than a
 * string prevents accidental collisions with other context providers in the
 * component tree and provides type-safe context retrieval.
 */
export const ROVING_CONTEXT_KEY: unique symbol = Symbol('RovingFocusContext')

/**
 * Register a RovingFocusContext in the current component's Svelte context.
 * Must be called during component initialization (inside <script> or a
 * lifecycle hook that runs at init time).
 */
export function setRovingContext(ctx: RovingFocusContext): void {
  setContext(ROVING_CONTEXT_KEY, ctx)
}

/**
 * Retrieve the RovingFocusContext from the current Svelte context tree.
 *
 * Throws if no context has been registered under ROVING_CONTEXT_KEY, giving
 * a descriptive message that names the missing provider so developers can
 * quickly identify the integration mistake.
 *
 * @throws {Error} When called outside a <RovingFocusProvider> component tree.
 */
export function getRovingContext(): RovingFocusContext {
  let ctx: RovingFocusContext | undefined

  try {
    ctx = getContext<RovingFocusContext>(ROVING_CONTEXT_KEY)
  } catch {
    // getContext throws a Svelte lifecycle error when called outside a component
    // init scope. We absorb it and re-throw our own descriptive message so the
    // developer sees the RovingFocusProvider hint rather than a Svelte internal.
    throw new Error(
      'getRovingContext: no RovingFocusContext in scope. Did you forget to wrap a parent in <RovingFocusProvider>?',
    )
  }

  if (ctx === undefined) {
    throw new Error(
      'getRovingContext: no RovingFocusContext in scope. Did you forget to wrap a parent in <RovingFocusProvider>?',
    )
  }

  return ctx
}
